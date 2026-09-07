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

use arpg_sim::{Condition, SourceId, SourceSpec, Template};

use crate::input::{key_named, key_names};

/// A command plus the channel its reply goes back on.
pub(crate) struct Request {
    pub(crate) command: Command,
    pub(crate) reply: Sender<String>,
}

pub(crate) enum Command {
    /// Hold a key down until told otherwise.
    Press(KeyCode),
    Release(KeyCode),
    /// Down now, up on the next frame — one clean press, whatever the frame rate.
    Tap(KeyCode),
    /// Down now, up after this many milliseconds; the reply waits for the release.
    Hold(KeyCode, u64),
    /// Reply after this many milliseconds of the game's own time.
    Wait(u64),
    /// Write the next rendered frame to a PNG; the reply waits for the file.
    Shot(PathBuf),
    /// Report simulation and camera state.
    State,
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
    SetEnemies(usize),
    SetSeekers(usize),
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
    /// Add something that asks for spawns, or remove one by name.
    ///
    /// The flags after the position are **named, not positional**, because a
    /// source is four independent choices and a column nobody can read is how
    /// three of them end up unused.
    ///
    /// Carries the simulation's own [`SourceSpec`] rather than a copy of its
    /// fields. That copy was the third definition of the same four axes — after
    /// the type itself and the scenario format — and each one had to be taught
    /// separately about an axis the others already had.
    Source(SourceSpec),
    RemoveSource(SourceId),
    SetVsync(bool),
    Quit,
}

/// Game keys are injected as `KeyCode`s rather than as actions, so a test
/// exercises the real binding table — only winit's delivery is skipped.
///
/// The names come from `BINDINGS` itself rather than a table kept here, so a
/// newly bound key is drivable immediately and this file cannot fall behind the
/// game it drives.
///
/// Meta commands are deliberately *not* keys. Simulating `[` to halve the horde
/// would be pantomime; `enemies 512` says what it means and cannot drift from
/// whatever key happens to be bound to it today.
fn key(arg: Option<&str>) -> Result<KeyCode, String> {
    let name = arg.ok_or_else(|| "expected a key name".to_string())?;
    key_named(name).ok_or_else(|| {
        format!(
            "unknown key {name:?}; bound keys are {}",
            key_names().collect::<Vec<_>>().join(" ")
        )
    })
}

/// Named once, because it is quoted from four error paths and a usage message
/// that disagrees with the parser is worse than none.
const USAGE: &str = "expected: source <x> <z> [seek] [every <n>] [ring <r>] [near <r>] [fewer <n>], \
     or source remove <id>";

fn parse(line: &str) -> Result<Command, String> {
    let mut it = line.split_whitespace();
    let verb = it.next().unwrap_or("");
    let arg = it.next();

    let number = |arg: Option<&str>| {
        arg.ok_or_else(|| "expected a number".to_string())
            .and_then(|v| v.parse::<u64>().map_err(|e| e.to_string()))
    };

    Ok(match verb {
        "press" => Command::Press(key(arg)?),
        "release" => Command::Release(key(arg)?),
        "tap" => Command::Tap(key(arg)?),
        "hold" => Command::Hold(key(arg)?, number(it.next())?),
        "wait" => Command::Wait(number(arg)?),
        "shot" => Command::Shot(PathBuf::from(arg.ok_or_else(|| "expected a path".to_string())?)),
        "state" => Command::State,
        // `trace since <tick>` rather than `trace <tick>`, so the reply cannot
        // be misread as "the trace at tick N".
        "trace" => match (arg, it.next()) {
            (Some("since"), Some(tick)) => Command::TraceSince(number(Some(tick))?),
            (Some("since"), None) => return Err("expected a tick: trace since <tick>".into()),
            _ => return Err("expected: trace since <tick>".into()),
        },
        "spawn" => {
            let coord = |arg: Option<&str>| {
                arg.ok_or_else(|| "expected: spawn <x> <z> [seek]".to_string())
                    .and_then(|v| v.parse::<f32>().map_err(|e| e.to_string()))
            };
            let x = coord(arg)?;
            let z = coord(it.next())?;
            // Behaviours are named, not positional, so the next one is another
            // word here rather than another column nobody can read.
            let what = match it.next() {
                Some("seek") => Template::BODY.seeking(),
                _ => Template::BODY,
            };
            Command::Spawn { x, z, what }
        }
        "source" => {
            if arg == Some("remove") {
                // Named in the form the trace prints — `s0`, not `0` — so an id
                // read out of `trace since` can be handed straight back.
                let name = it.next().unwrap_or("");
                let id = SourceId::parse(name)
                    .ok_or_else(|| format!("expected a source name like s0, got {name:?}"))?;
                return Ok(Command::RemoveSource(id));
            }

            // Both failures quote the usage: `source bogus` otherwise replies
            // "invalid float literal", which is true and tells nobody what to
            // type instead.
            let coord = |arg: Option<&str>| {
                arg.ok_or_else(|| USAGE.to_string())
                    .and_then(|v| v.parse::<f32>().map_err(|e| format!("{e}; {USAGE}")))
            };
            let x = coord(arg)?;
            let z = coord(it.next())?;

            let (mut every, mut radius, mut seeks) = (1, 0.0, false);
            let (mut near, mut fewer) = (None, None);

            // Flags in any order, each either a word or a word and a number.
            // An unknown one is an error rather than a shrug: a typo that
            // silently produced a source with no gate would look exactly like
            // the gate not working.
            while let Some(flag) = it.next() {
                match flag {
                    "seek" => seeks = true,
                    "every" => every = number(it.next())? as u32,
                    "ring" => radius = coord(it.next())?,
                    "near" => near = Some(coord(it.next())?),
                    "fewer" => fewer = Some(number(it.next())? as usize),
                    other => return Err(format!("unknown source flag {other:?}; {USAGE}")),
                }
            }

            // One gate, never two combined into something nobody wrote.
            // `near` beats `fewer` when both are given, and a flag repeated
            // takes its later value.
            let when = match (near, fewer) {
                (Some(radius), _) => Condition::PlayerWithin(radius),
                (None, Some(n)) => Condition::FewerThan(n),
                (None, None) => Condition::Always,
            };

            // Filled in field by field on purpose. `SourceSpec` is the one
            // definition of a source's axes, so an axis added there stops this
            // file compiling until somebody has decided which flag fills it —
            // which is the only check a text protocol can get for free.
            Command::Source(SourceSpec {
                pos: (x, z),
                radius,
                every,
                when,
                what: if seeks { Template::BODY.seeking() } else { Template::BODY },
            })
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
        "enemies" => Command::SetEnemies(number(arg)? as usize),
        "seekers" => Command::SetSeekers(number(arg)? as usize),
        "vsync" => Command::SetVsync(matches!(arg, Some("on") | Some("1"))),
        "quit" => Command::Quit,
        "" => return Err("empty command".into()),
        other => return Err(format!("unknown command {other:?}")),
    })
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
            if tx.send(Request { command, reply: reply_tx }).is_err() {
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
    use super::*;

    #[test]
    fn parses_the_command_vocabulary() {
        assert!(matches!(parse("press w"), Ok(Command::Press(KeyCode::KeyW))));
        assert!(matches!(parse("hold d 250"), Ok(Command::Hold(KeyCode::KeyD, 250))));
        assert!(matches!(parse("  state  "), Ok(Command::State)));
        assert!(matches!(parse("vsync on"), Ok(Command::SetVsync(true))));
        assert!(matches!(parse("vsync off"), Ok(Command::SetVsync(false))));
    }

    /// The parser fills [`SourceSpec`] — the simulation's own description of a
    /// source — rather than a copy of its fields, so the mapping from flags to
    /// axes is the whole of what is left to get wrong here, and this is where
    /// it is checked.
    #[test]
    fn source_flags_fill_the_simulations_own_description() {
        let Ok(Command::Source(spec)) = parse("source 30 -4 seek every 10 ring 3 fewer 8") else {
            panic!("should parse");
        };
        assert_eq!(spec.pos, (30.0, -4.0));
        assert_eq!(spec.radius, 3.0);
        assert_eq!(spec.every, 10);
        assert_eq!(spec.when, Condition::FewerThan(8));
        assert!(spec.what.seeks());

        // One gate rather than both: `near` wins whichever order they arrive in.
        for line in ["source 0 0 fewer 8 near 5", "source 0 0 near 5 fewer 8"] {
            let Ok(Command::Source(spec)) = parse(line) else { panic!("should parse") };
            assert_eq!(spec.when, Condition::PlayerWithin(5.0), "{line:?}");
        }

        // No flags at all is a plain body, every tick, on the point itself.
        let Ok(Command::Source(spec)) = parse("source 1 2") else { panic!("should parse") };
        assert_eq!((spec.radius, spec.every), (0.0, 1));
        assert_eq!(spec.when, Condition::Always);
        assert!(!spec.what.seeks());
    }

    /// `spawn` names a [`Template`] for the same reason: a behaviour added to
    /// the template is drivable from the socket without touching this file.
    #[test]
    fn spawn_names_a_template_rather_than_a_bare_flag() {
        let Ok(Command::Spawn { x, z, what }) = parse("spawn 5 -5 seek") else {
            panic!("should parse");
        };
        assert_eq!((x, z), (5.0, -5.0));
        assert!(what.seeks());

        let Ok(Command::Spawn { what, .. }) = parse("spawn 5 -5") else {
            panic!("should parse");
        };
        assert!(!what.seeks());
    }

    /// A malformed command must come back as an error the caller can read, not
    /// be silently dropped — silent no-ops are what made the OS-level approach
    /// untrustworthy in the first place.
    #[test]
    fn bad_input_is_reported_rather_than_ignored() {
        for bad in ["", "fly", "press", "press q", "hold d", "hold d soon", "wait"] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
    }
}
