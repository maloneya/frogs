//! A control socket for driving the game from outside, for testing.
//!
//! Automating a game through the OS input stack — synthetic keystrokes plus a
//! desktop screenshot tool — turns out to be unusable for evidence. It needs the
//! window frontmost, the display awake, and accessibility permission, and when
//! any of those is not true it does not fail: the keys go to whatever *is*
//! focused and the screenshot comes back black. Both look exactly like the game
//! being broken — and a stray keypress landing in another window can change
//! the game's state while apparently testing something else.
//!
//! A socket has none of those dependencies. It works on an unfocused window
//! behind other windows on a sleeping display, it reports its own failures, and
//! every command replies so the caller knows when the effect has actually
//! landed rather than guessing with a sleep.
//!
//! Off unless `ARPG_HARNESS` names a socket path, so an ordinary run has no
//! listener, no thread, and no way in.
//!
//! ```text
//! ARPG_HARNESS=/tmp/arpg.sock cargo run --release
//! echo 'hold d 400' | nc -U /tmp/arpg.sock
//! echo 'shot /tmp/f.png' | nc -U /tmp/arpg.sock
//! echo state | nc -U /tmp/arpg.sock
//! ```

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use winit::keyboard::KeyCode;

use arpg_sim::{SourceId, SourceSpec, Template};

use crate::input::{key_named, key_names};

/// A command plus the channel its reply goes back on.
pub(crate) struct Request {
    pub(crate) command: Command,
    pub(crate) reply: Sender<String>,
}

pub(crate) enum Command {
    TuningEdit(crate::tuning::Edit),
    Cursor(glam::Vec2),
    /// Atomically load and select one app-global player character.
    ShowCharacter(PathBuf),
    /// Restore the default player asset.
    ClearCharacter,
    /// Atomically load and select one app-global horde character.
    ShowHorde(PathBuf),
    /// Restore the default horde asset.
    ClearHorde,
    /// Reconstruct the complete playtest from a scene file.
    StartScene(PathBuf),
    /// Reuse the selected content snapshot, even if its file has since changed.
    RestartScene,
    /// Add disposable content without resetting the current playtest.
    AddScene(PathBuf),
    EvictScene(arpg_sim::SceneId),
    ListScenes,
    /// Hold a key down until told otherwise.
    Press(KeyCode),
    Release(KeyCode),
    /// Down now, up on the next event-loop turn; action edges wait for a tick.
    Tap(KeyCode),
    /// Down now, up after this many milliseconds; the reply waits for the release.
    Hold(KeyCode, u64),
    /// Reply after this many milliseconds of wall-clock time.
    Wait(u64),
    /// Write the next rendered frame to a PNG; the reply waits for the file.
    Shot(PathBuf),
    /// Report simulation and camera state.
    State,
    /// Bounded timing summary, optionally including individual frames.
    Perf { samples: bool },
    /// Discard the timing window without changing playable state.
    ResetPerf,
    /// Restart the selected scene and reply after a bounded tick-scheduled trial.
    PerfTrial(crate::trial::Spec),
    /// Every trace event from this tick onward.
    ///
    /// The counterpart to `state`, and the reason both exist: `state` is a
    /// point sample and cannot see what happened between two of them. Anything
    /// with a window — an attack, hitstop, a buffered input — lives entirely in
    /// that gap.
    TraceSince(u64),
    /// Add sim-validated momentum to a live body or the player.
    Impulse {
        target: String,
        value: arpg_sim::Impulse,
    },
    /// Ask for one body at a world-space `(x, z)`, and what to grant it.
    ///
    /// Goes through the spawn queue rather than placing directly — the same
    /// door anything inside the simulation uses — so what a shell drives here
    /// is the real path, latency included. The body exists after the next tick.
    Spawn {
        x: f32,
        z: f32,
        what: Template,
    },
    /// Add a source decoded from the simulation-owned content description.
    Source(SourceSpec),
    RemoveSource(SourceId),
    SetSourceEnabled {
        id: SourceId,
        enabled: bool,
    },
    /// Toggle read-only collision-disc outlines.
    SetCollisionDebug(bool),
    SetVsync(bool),
    Quit,
}

/// Game and UI keys are injected as `KeyCode`s into `Controls::on_key`, so a
/// test exercises the real binding table and context routing — only winit's
/// delivery is skipped.
///
/// The names come from `BINDINGS` itself rather than a table kept here, so a
/// newly bound key is drivable immediately and this file cannot fall behind the
/// game it drives.
fn key(arg: Option<&str>) -> Result<KeyCode, String> {
    let name = arg.ok_or_else(|| "expected a key name".to_string())?;
    key_named(name).ok_or_else(|| {
        format!(
            "unknown key {name:?}; bound keys are {}",
            key_names().collect::<Vec<_>>().join(" ")
        )
    })
}

fn parse(line: &str) -> Result<Command, String> {
    if let Some(rest) = line.trim().strip_prefix("character")
        && (rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        return parse_character(rest.trim());
    }
    if let Some(rest) = line.trim().strip_prefix("horde")
        && (rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        return parse_horde(rest.trim());
    }
    if let Some(rest) = line.trim().strip_prefix("scene")
        && (rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        return parse_scene(rest.trim());
    }
    let mut it = line.split_whitespace();
    let verb = it.next().unwrap_or("");
    let arg = it.next();

    let number = |arg: Option<&str>| {
        arg.ok_or_else(|| "expected a number".to_string())
            .and_then(|v| v.parse::<u64>().map_err(|e| e.to_string()))
    };

    Ok(match verb {
        "tuning" => {
            use crate::tuning::Edit;
            let usage = "tuning save|revert|defaults|restart|set <key> <value>";
            let command = match arg {
                Some("save") => Command::TuningEdit(Edit::Save),
                Some("revert") => Command::TuningEdit(Edit::Revert),
                Some("defaults") => Command::TuningEdit(Edit::Defaults),
                Some("restart") => Command::TuningEdit(Edit::Restart),
                Some("set") => {
                    let field = it.next().and_then(arpg_sim::TuningField::named).ok_or("unknown tuning field")?;
                    let value = it.next().ok_or(usage)?.parse::<f32>().map_err(|_| usage)?;
                    arpg_sim::Tuning::default().with(field, value)?;
                    Command::TuningEdit(Edit::Set(field, value))
                }
                _ => return Err(usage.into()),
            };
            if it.next().is_some() { return Err(usage.into()); }
            command
        }
        "cursor" => {
            let usage = "expected: cursor <finite physical x> <finite physical y>";
            let x = arg.ok_or(usage)?.parse::<f32>().map_err(|_| usage)?;
            let y = it.next().ok_or(usage)?.parse::<f32>().map_err(|_| usage)?;
            let pixel = glam::Vec2::new(x, y);
            if !pixel.is_finite() || it.next().is_some() { return Err(usage.into()); }
            Command::Cursor(pixel)
        }
        "press" => Command::Press(key(arg)?),
        "release" => Command::Release(key(arg)?),
        "tap" => Command::Tap(key(arg)?),
        "hold" => Command::Hold(key(arg)?, number(it.next())?),
        "wait" => Command::Wait(number(arg)?),
        "shot" => Command::Shot(PathBuf::from(
            arg.ok_or_else(|| "expected a path".to_string())?,
        )),
        "state" => Command::State,
        "perf" => {
            let command = match arg {
                None => Command::Perf { samples: false },
                Some("samples") => Command::Perf { samples: true },
                Some("reset") => Command::ResetPerf,
                Some("trial") => return crate::trial::Spec::parse(it)
                    .map(Command::PerfTrial),
                _ => return Err("expected: perf [samples|reset]".into()),
            };
            if it.next().is_some() { return Err("expected: perf [samples|reset]".into()); }
            command
        }
        // `trace since <tick>` rather than `trace <tick>`, so the reply cannot
        // be misread as "the trace at tick N".
        "trace" => match (arg, it.next()) {
            (Some("since"), Some(tick)) => Command::TraceSince(number(Some(tick))?),
            (Some("since"), None) => return Err("expected a tick: trace since <tick>".into()),
            _ => return Err("expected: trace since <tick>".into()),
        },
        "spawn" => {
            let placed = arpg_content::parse_placement(line.trim()["spawn".len()..].trim())
                .map_err(|error| error.to_string())?;
            Command::Spawn { x: placed.pos.0, z: placed.pos.1, what: placed.what }
        }
        "source" => {
            if matches!(arg, Some("remove" | "enable" | "disable")) {
                let usage = "expected: source remove|enable|disable <id>";
                let id = it.next().and_then(SourceId::parse).ok_or(usage)?;
                if it.next().is_some() {
                    return Err(usage.into());
                }
                if arg == Some("remove") {
                    Command::RemoveSource(id)
                } else {
                    Command::SetSourceEnabled { id, enabled: arg == Some("enable") }
                }
            } else {
                Command::Source(arpg_content::parse_source(line.trim()["source".len()..].trim())
                    .map_err(|error| error.to_string())?)
            }
        }
        "impulse" => {
            let usage = "expected: impulse <player|#id> <x> <z>";
            let target = arg.ok_or(usage)?.to_owned();
            let mut number = || it.next().ok_or(usage)?.parse::<f32>().map_err(|_| usage);
            let value = arpg_sim::Impulse::try_from((number()?, number()?))?;
            if it.next().is_some() {
                return Err(usage.into());
            }
            Command::Impulse { target, value }
        }
        "debug" => {
            if arg != Some("collision") { return Err("expected: debug collision on|off".into()); }
            let enabled = match it.next() {
                Some("on") => true,
                Some("off") => false,
                _ => return Err("expected: debug collision on|off".into()),
            };
            if it.next().is_some() { return Err("expected: debug collision on|off".into()); }
            Command::SetCollisionDebug(enabled)
        }
        "vsync" => Command::SetVsync(matches!(arg, Some("on") | Some("1"))),
        "quit" => Command::Quit,
        "" => return Err("empty command".into()),
        other => return Err(format!("unknown command {other:?}")),
    })
}

fn parse_character(text: &str) -> Result<Command, String> {
    parse_selection(
        text,
        "character",
        Command::ShowCharacter,
        Command::ClearCharacter,
    )
}

fn parse_horde(text: &str) -> Result<Command, String> {
    parse_selection(text, "horde", Command::ShowHorde, Command::ClearHorde)
}

fn parse_selection(
    text: &str,
    noun: &str,
    show: fn(PathBuf) -> Command,
    clear: Command,
) -> Result<Command, String> {
    let (verb, value) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let value = value.trim();
    match (verb, value) {
        ("show", path) if !path.is_empty() => Ok(show(path.into())),
        ("clear", "") => Ok(clear),
        _ => Err(format!("expected: {noun} show <path>, {noun} clear")),
    }
}

fn parse_scene(text: &str) -> Result<Command, String> {
    let (verb, value) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let value = value.trim();
    let usage = "expected: scene start <path>, scene restart, scene add <path>, scene evict <id>, scene list";
    match (verb, value) {
        ("start", path) if !path.is_empty() => Ok(Command::StartScene(path.into())),
        ("add", path) if !path.is_empty() => Ok(Command::AddScene(path.into())),
        ("restart", "") => Ok(Command::RestartScene),
        ("list", "") => Ok(Command::ListScenes),
        ("evict", id) => arpg_sim::SceneId::parse(id)
            .map(Command::EvictScene)
            .ok_or_else(|| usage.into()),
        _ => Err(usage.into()),
    }
}

/// Starts the listener if `ARPG_HARNESS` names a socket path.
pub(crate) fn start() -> Option<Receiver<Request>> {
    let path = std::env::var_os("ARPG_HARNESS")?;
    let path = PathBuf::from(path);

    // A socket file outlives the process that made it, so a previous run's
    // corpse would make bind fail with EADDRINUSE.
    let _ = std::fs::remove_file(&path);

    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            log::error!("harness: cannot bind {}: {e}", path.display());
            return None;
        }
    };
    log::info!("harness listening on {}", path.display());

    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => serve(s, &tx),
                Err(e) => log::warn!("harness: accept failed: {e}"),
            }
        }
    });
    Some(rx)
}

/// One connection, one command, one reply. Connection-per-command keeps this
/// trivially usable from a shell — `echo … | nc -U …` — with no framing
/// protocol and no partial-line state to get wrong.
fn serve(stream: UnixStream, tx: &Sender<Request>) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(e) => return log::warn!("harness: {e}"),
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }

    let mut out = stream;
    let response = match parse(line.trim()) {
        Err(e) => format!("error: {e}\n"),
        Ok(command) => {
            let (reply_tx, reply_rx) = channel();
            if tx
                .send(Request {
                    command,
                    reply: reply_tx,
                })
                .is_err()
            {
                "error: game is shutting down\n".to_string()
            } else {
                // Blocks until the game loop has actually applied it, which is
                // the point: the caller learns when the effect landed instead
                // of sleeping and hoping.
                match reply_rx.recv() {
                    Ok(r) => format!("{r}\n"),
                    Err(_) => "error: no reply\n".to_string(),
                }
            }
        }
    };
    let _ = out.write_all(response.as_bytes());
}

#[cfg(test)]
mod tests {
    #[test]
    fn performance_commands_reject_unknown_operations_and_extra_arguments() {
        assert!(matches!(super::parse("perf"), Ok(super::Command::Perf { samples: false })));
        assert!(matches!(super::parse("perf samples"), Ok(super::Command::Perf { samples: true })));
        assert!(matches!(super::parse("perf reset"), Ok(super::Command::ResetPerf)));
        assert!(matches!(super::parse("perf trial 120 180 idle"), Ok(super::Command::PerfTrial(_))));
        assert!(matches!(super::parse("perf\ttrial 120 180 combat"), Ok(super::Command::PerfTrial(_))));
        for bad in ["perf start", "perf reset extra", "perf samples extra", "perf 2000", "perf trial 0 60 idle", "perf trial 1 60 idle extra"] {
            assert!(super::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn collision_debug_requires_an_explicit_valid_setting() {
        assert!(matches!(super::parse("debug collision on"), Ok(super::Command::SetCollisionDebug(true))));
        assert!(matches!(super::parse("debug collision off"), Ok(super::Command::SetCollisionDebug(false))));
        for invalid in ["debug", "debug collision", "debug collision yes", "debug collision on extra", "debug contacts on"] {
            assert!(super::parse(invalid).is_err());
        }
    }

    use super::*;

    #[test]
    fn character_commands_are_explicit_and_preserve_paths_with_spaces() {
        assert!(
            matches!(parse("character show assets/my rig.glb"), Ok(Command::ShowCharacter(path)) if path == std::path::Path::new("assets/my rig.glb"))
        );
        assert!(matches!(parse("character clear"), Ok(Command::ClearCharacter)));
        for bad in [
            "character",
            "character show",
            "character clear extra",
            "character hide file.glb",
        ] {
            assert!(parse(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn horde_commands_are_explicit_and_preserve_paths_with_spaces() {
        assert!(
            matches!(parse("horde show assets/my crowd.glb"), Ok(Command::ShowHorde(path)) if path == std::path::Path::new("assets/my crowd.glb"))
        );
        assert!(matches!(parse("horde clear"), Ok(Command::ClearHorde)));
        for bad in [
            "horde",
            "horde show",
            "horde clear extra",
            "horde hide file.glb",
        ] {
            assert!(parse(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn scene_commands_are_explicit_and_preserve_paths_with_spaces() {
        assert!(
            matches!(parse("scene start scenes/my pair.ron"), Ok(Command::StartScene(path)) if path == std::path::Path::new("scenes/my pair.ron"))
        );
        assert!(matches!(parse("scene restart"), Ok(Command::RestartScene)));
        assert!(matches!(parse("scene list"), Ok(Command::ListScenes)));
        assert!(matches!(parse("scene add scenes/seekers.ron"), Ok(Command::AddScene(_))));
        assert!(matches!(parse("scene evict c0"), Ok(Command::EvictScene(_))));
        for bad in [
            "scene",
            "scene start",
            "scene restart extra",
            "scene list extra",
            "scene evict",
            "scene evict s0",
            "scene evict c0 extra",
        ] {
            assert!(parse(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn parses_the_command_vocabulary() {
        assert!(matches!(parse("press w"), Ok(Command::Press(KeyCode::KeyW))));
        assert!(matches!(parse("hold d 250"), Ok(Command::Hold(KeyCode::KeyD, 250))));
        assert!(matches!(parse("  state  "), Ok(Command::State)));
        assert!(matches!(parse("vsync on"), Ok(Command::SetVsync(true))));
        assert!(matches!(parse("vsync off"), Ok(Command::SetVsync(false))));
    }

    #[test]
    fn source_commands_use_authored_content_and_strict_identity_operations() {
        let Ok(Command::Source(spec)) = parse(
            "source (pos: (30.0, -4.0), every: 10, radius: 3.0, when: FewerThan(8), what: (seeks: true), enabled: false)"
        ) else { panic!("source description must parse") };
        assert_eq!(spec.pos, (30.0, -4.0));
        assert_eq!(spec.radius, 3.0);
        assert_eq!(spec.every, 10);
        assert_eq!(spec.when, arpg_sim::Condition::FewerThan(8));
        assert!(spec.what.seeks());
        assert!(!spec.enabled);
        for (verb, expected) in [("enable", true), ("disable", false)] {
            let Ok(Command::SetSourceEnabled { id, enabled }) = parse(&format!("source {verb} s3")) else {
                panic!("enablement command must parse");
            };
            assert_eq!(id, SourceId::parse("s3").unwrap());
            assert_eq!(enabled, expected);
        }
        assert!(matches!(parse("source remove s3"), Ok(Command::RemoveSource(_))));
        for verb in ["enable", "disable", "remove"] {
            for suffix in ["", "c0", "s3 extra", "s-1"] {
                assert!(parse(&format!("source {verb} {suffix}")).is_err());
            }
        }
        for bad in [
            "source 30 -4 seek every 10", "source (pos: (0.0, 0.0), near: 3.0)",
            "source (pos: (0.0, 0.0), every: 4294967296)",
            "source (pos: (0.0, 0.0), every: 2, every: 3)",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn spawn_commands_use_the_authored_placement_schema() {
        let Ok(Command::Spawn { x, z, what }) = parse(
            "spawn (pos: (5.0, -5.0), what: (fixed: true, interactable: true))"
        ) else { panic!("placement must parse") };
        assert_eq!((x, z), (5.0, -5.0));
        assert_eq!(what, Template::BLOCK.interactive());
        let Ok(Command::Spawn { what, .. }) = parse("spawn (pos: (0.0, 2.0))") else {
            panic!("default template must parse");
        };
        assert_eq!(what, Template::BODY);
        for bad in ["spawn 0 2 seek", "spawn (pos: (0.0, 2.0), what: (interactble: true))", "enemies 3", "seekers 3", "asset show mesh.glb"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    /// A malformed command must come back as an error the caller can read, not
    /// be silently dropped — silent no-ops are what made the OS-level approach
    /// untrustworthy in the first place.
    #[test]
    fn bad_input_is_reported_rather_than_ignored() {
        for bad in [
            "",
            "fly",
            "press",
            "press q",
            "hold d",
            "hold d soon",
            "wait",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
    }
}

#[cfg(test)]
mod cursor_tests {
    #[test]
    fn cursor_commands_reject_invalid_coordinates_and_extra_fields() {
        assert!(matches!(super::parse("cursor 100 200"), Ok(super::Command::Cursor(_))));
        for bad in ["cursor NaN 0", "cursor 0 inf", "cursor 1", "cursor 1 2 3"] {
            assert!(super::parse(bad).is_err(), "{bad}");
        }
    }
}
