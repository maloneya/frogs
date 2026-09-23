//! Fixed navigation benchmark. Reports are computed from maps and actual play traces.
use crate::{
    generation::{self, Biome, Outcome, Request},
    play::{self, Profile, Session},
    recipe::Recipe,
};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
};

pub(crate) const CONTRACT: &str = "navigation-v1";
const SEEKER_BAND: (f64, f64) = (1.25, 3.0);
const EXPLORER_BAND: (f64, f64) = (0.15, 0.55);
const SEEKER_PENALTY: f64 = 200.0;
const EXPLORER_PENALTY: f64 = 500.0;

fn objective_json() -> String {
    format!(
        "{{\"scale\":[0,1000],\"seeker_steps_over_shortest_target\":[{},{}],\"explorer_revisit_fraction_target\":[{},{}],\"seeker_penalty_per_unit\":{},\"explorer_penalty_per_unit\":{},\"invalid_or_incomplete_score\":0}}",
        SEEKER_BAND.0,
        SEEKER_BAND.1,
        EXPLORER_BAND.0,
        EXPLORER_BAND.1,
        SEEKER_PENALTY,
        EXPLORER_PENALTY
    )
}

fn seeds(suite: &str) -> Result<std::ops::Range<u64>, String> {
    match suite {
        "development" => Ok(0..8),
        "validation" => Ok(100..108),
        "holdout" => Ok(10_000..10_008),
        _ => Err("suite must be development, validation or holdout".into()),
    }
}

/// A proxy for moderate discovery overhead and bounded repetition, not a fun score.
fn score(route: usize, seeker: &play::PlayResult, explorer: &play::PlayResult) -> f64 {
    if !seeker.completed || !explorer.completed {
        return 0.0;
    }
    let stretch = seeker.steps as f64 / route.max(1) as f64;
    let repeat = explorer.revisits as f64 / explorer.steps.max(1) as f64;
    let distance_to_band = |n: f64, lo: f64, hi: f64| (lo - n).max(0.0) + (n - hi).max(0.0);
    (1000.0
        - SEEKER_PENALTY * distance_to_band(stretch, SEEKER_BAND.0, SEEKER_BAND.1)
        - EXPLORER_PENALTY * distance_to_band(repeat, EXPLORER_BAND.0, EXPLORER_BAND.1))
    .clamp(0.0, 1000.0)
}

fn write(path: impl AsRef<Path>, text: impl AsRef<[u8]>) -> Result<(), String> {
    fs::write(path, text).map_err(|e| e.to_string())
}
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn evaluate(suite: &str, recipe: &Recipe, output: &Path) -> Result<(), String> {
    let suite_seeds = seeds(suite)?;
    // An existing run is evidence. Never overwrite it, including partial runs.
    fs::create_dir(output).map_err(|e| format!("create fresh output directory: {e}"))?;
    write(output.join("recipe.txt"), recipe.text())?;
    let mut cases = Vec::new();
    let mut sum = 0.0;
    let mut minimum: f64 = 1000.0;
    let mut failures = 0;
    let mut incomplete_plays = 0;
    for seed in suite_seeds {
        for biome in [Biome::Cave, Biome::Tunnels] {
            let request = Request {
                seed,
                biome,
                width: if seed % 2 == 0 { 40 } else { 48 },
                height: if seed % 2 == 0 { 24 } else { 28 },
                min_route: 20,
                attempts: 8,
                ..Request::default()
            };
            let (candidate, accepted) = match generation::generate(&request, recipe)? {
                Outcome::Accepted(c) => (c, true),
                Outcome::Rejected(c) => (c, false),
            };
            let label = format!(
                "{}-{seed}",
                match biome {
                    Biome::Cave => "cave",
                    Biome::Tunnels => "tunnels",
                }
            );
            write(
                output.join(format!("{label}.map.txt")),
                candidate.map.ascii(candidate.endpoints),
            )?;
            let mut players = Vec::new();
            let value = if accepted {
                let ends = candidate.endpoints.expect("accepted map has endpoints");
                let seeker = play::autoplay(&candidate.map, ends, Profile::Seeker);
                let explorer = play::autoplay(&candidate.map, ends, Profile::Explorer);
                for result in [&seeker, &explorer] {
                    if !result.completed {
                        incomplete_plays += 1;
                    }
                    let name = result.profile.name();
                    write(
                        output.join(format!("{label}.{name}.trace.tsv")),
                        &result.trace,
                    )?;
                    write(
                        output.join(format!("{label}.{name}.route.txt")),
                        &result.overlay,
                    )?;
                    players.push(result.json());
                }
                score(candidate.route, &seeker, &explorer)
            } else {
                failures += 1;
                0.0
            };
            sum += value;
            minimum = minimum.min(value);
            let errors = candidate
                .violations
                .iter()
                .map(|e| quoted(e))
                .collect::<Vec<_>>()
                .join(",");
            cases.push(format!("{{\"case\":\"{label}\",\"seed\":{seed},\"width\":{},\"height\":{},\"accepted\":{accepted},\"attempts\":{},\"open\":{},\"shortest_route\":{},\"score\":{value:.6},\"violations\":[{errors}],\"players\":[{}]}}",
                request.width,request.height,candidate.attempt,candidate.open,candidate.route,players.join(",")));
        }
    }
    let count = cases.len();
    let objective = objective_json();
    let json = format!(
        "{{\"contract\":\"{CONTRACT}\",\"objective\":{objective},\"suite\":\"{suite}\",\"cases\":{count},\"failed_maps\":{failures},\"incomplete_plays\":{incomplete_plays},\"mean_score\":{:.6},\"worst_score\":{minimum:.6},\"results\":[{}]}}\n",
        sum / count as f64,
        cases.join(",\n")
    );
    write(output.join("summary.json"), &json)?;
    io::stdout()
        .write_all(json.as_bytes())
        .map_err(|e| e.to_string())
}

pub(crate) fn command(args: &[String]) -> Result<u8, String> {
    let mut args = args.to_vec();
    let mode = args.remove(0);
    let recipe = crate::take_recipe(&mut args)?;
    if mode == "evaluate" {
        let mut suite = None;
        let mut output = None;
        let mut options = args.iter();
        while let Some(flag) = options.next() {
            let value = options.next().ok_or("evaluate options require values")?;
            match flag.as_str() {
                "--suite" if suite.is_none() => suite = Some(value.as_str()),
                "--output" if output.is_none() => output = Some(value.as_str()),
                _ => return Err(format!("unknown or repeated evaluate option {flag}")),
            }
        }
        evaluate(
            suite.ok_or("evaluate requires --suite")?,
            &recipe,
            Path::new(output.ok_or("evaluate requires --output (a new directory)")?),
        )?;
        return Ok(0);
    }
    let manual = if let Some(i) = args.iter().position(|a| a == "--manual") {
        args.remove(i);
        true
    } else {
        false
    };
    let (request, stages) = crate::parse(&args)?;
    if stages {
        return Err("play shows routes or fog; use generation for --stages".into());
    }
    let candidate = match generation::generate(&request, &recipe)? {
        Outcome::Accepted(c) => c,
        Outcome::Rejected(c) => {
            return Err(format!("generation rejected: {}", c.violations.join("; ")));
        }
    };
    let (start, exit) = candidate.endpoints.expect("accepted endpoints");
    let mut out = io::stdout().lock();
    if manual {
        let mut session = Session::new(
            candidate.map,
            start,
            exit,
            Profile::Seeker,
            request.width * request.height * 6,
        )?;
        writeln!(out,"Commands: north south west east observe quit. @ player, ? unknown. Local sensing: 3 floor steps.").map_err(|e|e.to_string())?;
        writeln!(out, "{}", session.observe().ascii()).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
        for line in io::stdin().lock().lines() {
            let line = line.map_err(|e| e.to_string())?;
            match line.trim() {
                "quit" => return Ok(0),
                "observe" => {}
                text => session.step(play::Action::parse(text)?)?,
            }
            writeln!(
                out,
                "step={} completed={}\n{}",
                session.steps(),
                session.completed(),
                session.observe().ascii()
            )
            .map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
            if session.finished() {
                return Ok(u8::from(!session.completed()));
            }
        }
        return Ok(u8::from(!session.completed()));
    }
    writeln!(
        out,
        "Reference shortest path: {} cell steps. * visited, + revisited.\n",
        candidate.route
    )
    .map_err(|e| e.to_string())?;
    let mut completed = true;
    for profile in [Profile::Seeker, Profile::Explorer] {
        let result = play::autoplay(&candidate.map, (start, exit), profile);
        completed &= result.completed;
        writeln!(out, "{}\n{}", result.json(), result.overlay).map_err(|e| e.to_string())?;
    }
    Ok(u8::from(!completed))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failures_cannot_receive_navigation_credit() {
        let map = crate::grid::Grid {
            width: 3,
            height: 1,
            cells: vec![true, false, true],
        };
        let result = play::autoplay(&map, (0, 2), Profile::Seeker);
        assert!(!result.completed);
        assert_eq!(score(1, &result, &result), 0.0);
    }
    #[test]
    fn suites_are_disjoint_and_unknown_names_are_refused() {
        let dev = seeds("development").unwrap().collect::<Vec<_>>();
        let validation = seeds("validation").unwrap().collect::<Vec<_>>();
        let holdout = seeds("holdout").unwrap().collect::<Vec<_>>();
        assert!(
            dev.iter()
                .all(|s| !validation.contains(s) && !holdout.contains(s))
        );
        assert!(validation.iter().all(|s| !holdout.contains(s)));
        assert!(seeds("other").is_err());
    }
}
