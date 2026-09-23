//! A terminal laboratory for deterministic level generation, with no game dependencies.

mod evaluation;
mod generation;
mod grid;
mod play;
mod recipe;

use generation::{Biome, Candidate, Outcome, Request};
use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};

fn parse(args: &[String]) -> Result<(Request, bool), String> {
    let mut request = Request::default();
    let mut stages = false;
    let mut seen = Vec::new();
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        if seen.contains(&flag.as_str()) {
            return Err(format!("duplicate option {flag}"));
        }
        seen.push(flag.as_str());
        if flag == "--stages" {
            stages = true;
            continue;
        }
        if !matches!(
            flag.as_str(),
            "--seed"
                | "--width"
                | "--height"
                | "--biome"
                | "--passage-width"
                | "--open-min"
                | "--open-max"
                | "--min-route"
                | "--attempts"
        ) {
            return Err(format!("unknown option {flag}; use --help"));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        let number = || {
            value
                .parse::<usize>()
                .map_err(|_| format!("{flag} needs a nonnegative integer"))
        };
        match flag.as_str() {
            "--seed" => request.seed = value.parse().map_err(|_| "--seed needs a u64 integer")?,
            "--width" => request.width = number()?,
            "--height" => request.height = number()?,
            "--passage-width" => request.passage_width = number()?,
            "--open-min" => request.open_min = number()?,
            "--open-max" => request.open_max = number()?,
            "--min-route" => request.min_route = number()?,
            "--attempts" => request.attempts = number()?,
            "--biome" => {
                request.biome = match value.as_str() {
                    "cave" => Biome::Cave,
                    "tunnels" => Biome::Tunnels,
                    _ => return Err("--biome must be cave or tunnels".into()),
                }
            }
            _ => unreachable!("options were checked before reading their values"),
        }
    }
    Ok((request, stages))
}

fn help(out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "Commands: evaluate --suite development|validation|holdout --output DIR [--recipe FILE]\n          play [generation options] [--recipe FILE]\n          recipe (print default recipe)\n"
    )?;
    writeln!(
        out,
        "Generation accepts --recipe FILE in addition to the options below."
    )?;
    let d = Request::default();
    writeln!(
        out,
        "levelgen — seeded ASCII map experiments\n\nOptions (default):"
    )?;
    writeln!(
        out,
        "  --seed N             Repeatable u64 seed ({})",
        d.seed
    )?;
    writeln!(
        out,
        "  --biome cave|tunnels Generation style ({:?})",
        d.biome
    )?;
    writeln!(
        out,
        "  --width N            Output columns, 16..160 ({})",
        d.width
    )?;
    writeln!(
        out,
        "  --height N           Output rows, 16..100 ({})",
        d.height
    )?;
    writeln!(
        out,
        "  --passage-width N    Full-square carving width, 1..8 ({})",
        d.passage_width
    )?;
    writeln!(
        out,
        "  --open-min N         Minimum open percentage, including border ({})",
        d.open_min
    )?;
    writeln!(
        out,
        "  --open-max N         Maximum open percentage, including border ({})",
        d.open_max
    )?;
    writeln!(
        out,
        "  --min-route N        Minimum S-to-E shortest path in cell steps ({})",
        d.min_route
    )?;
    writeln!(
        out,
        "  --attempts N         Maximum candidates, 1..128 ({})",
        d.attempts
    )?;
    writeln!(
        out,
        "  --stages             Also print selected candidate's coarse stages"
    )?;
    writeln!(
        out,
        "  --help               Show this help\n\nExit codes: 0 accepted/help, 1 generation exhausted, 2 invalid input/I/O error."
    )
}

fn report(
    out: &mut impl Write,
    request: &Request,
    candidate: &Candidate,
    accepted: bool,
    stages: bool,
) -> io::Result<()> {
    writeln!(
        out,
        "{} | seed={} biome={:?} size={}x{} attempt={}/{}",
        if accepted {
            "ACCEPTED"
        } else {
            "REJECTED (last candidate)"
        },
        request.seed,
        request.biome,
        request.width,
        request.height,
        candidate.attempt,
        request.attempts
    )?;
    if stages {
        for (name, grid) in &candidate.stages {
            writeln!(
                out,
                "\n{name} ({}x{} logical cells):\n{}",
                grid.width,
                grid.height,
                grid.ascii(None)
            )?;
        }
    }
    writeln!(out, "\n{}", candidate.map.ascii(candidate.endpoints))?;
    writeln!(out, "# rock  . floor  S start  E exit")?;
    writeln!(
        out,
        "Open: {}/{} = {:.2}% (required {}..{}%)",
        candidate.open,
        candidate.map.cells.len(),
        candidate.open as f64 * 100.0 / candidate.map.cells.len() as f64,
        request.open_min,
        request.open_max
    )?;
    writeln!(
        out,
        "Reachable floor: {}/{} (required all, nonempty)",
        candidate.reached, candidate.open
    )?;
    writeln!(
        out,
        "S-to-E shortest route: {} cell steps (required >= {})",
        candidate.route, request.min_route
    )?;
    writeln!(
        out,
        "Passage construction: {}x{} floor blocks; full-edge connections",
        request.passage_width, request.passage_width
    )?;
    for violation in &candidate.violations {
        writeln!(out, "FAIL: {violation}")?;
    }
    Ok(())
}

fn take_recipe(args: &mut Vec<String>) -> Result<recipe::Recipe, String> {
    let Some(index) = args.iter().position(|a| a == "--recipe") else {
        return Ok(recipe::Recipe::default());
    };
    let path = args.get(index + 1).ok_or("--recipe needs a file")?;
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let recipe = recipe::Recipe::parse(&text)?;
    args.drain(index..index + 2);
    if args.iter().any(|a| a == "--recipe") {
        return Err("duplicate --recipe".into());
    }
    Ok(recipe)
}

fn run() -> Result<u8, String> {
    let mut args: Vec<_> = env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "evaluate" || a == "play") {
        return evaluation::command(&args);
    }
    if args == ["recipe", "--schema"] {
        io::stdout()
            .write_all(recipe::Recipe::schema().as_bytes())
            .map_err(|e| e.to_string())?;
        return Ok(0);
    }
    if args == ["recipe"] {
        io::stdout()
            .write_all(recipe::Recipe::default().text().as_bytes())
            .map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let recipe = take_recipe(&mut args)?;
    let mut out = io::stdout().lock();
    if args == ["--help"] {
        help(&mut out).map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let (request, stages) = parse(&args)?;
    let (candidate, accepted) = match generation::generate(&request, &recipe)? {
        Outcome::Accepted(candidate) => (candidate, true),
        Outcome::Rejected(candidate) => (candidate, false),
    };
    report(&mut out, &request, &candidate, accepted, stages).map_err(|e| e.to_string())?;
    Ok(u8::from(!accepted))
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "levelgen: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_cli_is_not_silently_ignored() {
        for input in [
            "--seed",
            "--seed -1",
            "--biome forest",
            "--wat 3",
            "--seed 2 --seed 3",
            "--open-min NaN",
            "--stages --stages",
        ] {
            let args = input
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            assert!(parse(&args).is_err(), "{input}");
        }
    }
}
