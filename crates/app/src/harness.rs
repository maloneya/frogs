//! A control socket for driving the game from outside, for testing.
//!
//! Automating a game through the OS input stack — synthetic keystrokes plus a
//! desktop screenshot tool — turns out to be unusable for evidence. It needs the
//! window frontmost, the display awake, and accessibility permission, and when
//! any of those is not true it does not fail: the keys go to whatever *is*
//! focused and the screenshot comes back black. Both look exactly like the game
//! being broken. This session lost a stray keypress into another window and
//! silently changed the horde size while apparently testing something else.
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
use std::sync::mpsc::{channel, Receiver, Sender};

use winit::keyboard::KeyCode;

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
    SetEnemies(usize),
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
        format!("unknown key {name:?}; bound keys are {}", key_names().collect::<Vec<_>>().join(" "))
    })
}

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
        "shot" => Command::Shot(PathBuf::from(
            arg.ok_or_else(|| "expected a path".to_string())?,
        )),
        "state" => Command::State,
        "enemies" => Command::SetEnemies(number(arg)? as usize),
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
