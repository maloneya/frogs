//! Local companion transport. This thread decodes messages, never touches Game.
use arpg_game::TuningField;
use serde::Deserialize;
use std::{io::{BufRead, BufReader, Read, Write}, os::unix::{fs::MetadataExt, net::UnixStream},
    path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{Receiver, Sender, SyncSender, channel, sync_channel}}, time::{Duration, Instant}};

#[derive(Clone, Debug)]
pub(crate) enum Edit {
    Set(TuningField, f32), Save, Revert, Defaults, Restart,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ZoomDirection { In, Out }

pub(crate) enum Control {
    Tuning(Edit),
    StartScene(PathBuf),
    Collision(bool),
    Vsync(bool),
    Zoom(ZoomDirection),
    Capture(PathBuf),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Command {
    State {},
    Set { session: u32, key: String, value: f32 },
    Save { session: u32 }, Revert { session: u32 }, Defaults { session: u32 }, Restart { session: u32 },
    StartScene { session: u32, path: PathBuf },
    Collision { session: u32, enabled: bool },
    Vsync { session: u32, enabled: bool },
    Zoom { session: u32, direction: ZoomDirection },
    Capture { session: u32, path: PathBuf },
}

impl Command {
    pub(crate) fn control(self) -> Result<Option<Control>, String> {
        let (session, control) = match self {
            Self::State {} => return Ok(None),
            Self::Set { session, key, value } => (session, Control::Tuning(Edit::Set(TuningField::named(&key).ok_or("Unknown tuning field")?, value))),
            Self::Save { session } => (session, Control::Tuning(Edit::Save)),
            Self::Revert { session } => (session, Control::Tuning(Edit::Revert)),
            Self::Defaults { session } => (session, Control::Tuning(Edit::Defaults)),
            Self::Restart { session } => (session, Control::Tuning(Edit::Restart)),
            Self::StartScene { session, path } => (session, Control::StartScene(path)),
            Self::Collision { session, enabled } => (session, Control::Collision(enabled)),
            Self::Vsync { session, enabled } => (session, Control::Vsync(enabled)),
            Self::Zoom { session, direction } => (session, Control::Zoom(direction)),
            Self::Capture { session, path } => (session, Control::Capture(path)),
        };
        if session != std::process::id() { return Err("Game restarted; review current values before editing".into()); }
        Ok(Some(control))
    }
}

pub(crate) struct Request {
    pub(crate) command: Command,
    pub(crate) reply: Sender<String>,
    pub(crate) expires: Instant,
}

pub(crate) fn socket_path() -> PathBuf {
    std::env::var_os("ARPG_COMPANION_SOCKET").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/companion.sock"))
}

/// One bounded request at a time. A stalled companion cannot block the game loop.
/// Ownership of the socket path lasts exactly as long as this server.
pub(crate) struct Server {
    pub(crate) requests: Receiver<Request>,
    path: PathBuf,
    inode: u64,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    pub(crate) fn start(path: PathBuf) -> Result<Self, String> {
        let path = std::path::absolute(path).map_err(|e| e.to_string())?;
        let listener = crate::local_socket::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let inode = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?.ino();
        let (tx, requests) = sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &tx),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
                    Err(e) => { log::warn!("companion accept: {e}"); break; }
                }
            }
        });
        log::info!("companion listening on {}", path.display());
        Ok(Self { requests, path, inode, stop, worker: Some(worker) })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        if std::fs::symlink_metadata(&self.path).is_ok_and(|meta| meta.ino() == self.inode) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn serve(mut stream: UnixStream, tx: &SyncSender<Request>) {
    let timeout = Duration::from_secs(2);
    // macOS accepted sockets inherit the listener's nonblocking mode. The
    // dedicated worker uses bounded blocking reads, including fragmented lines.
    if stream.set_nonblocking(false).is_err() { return; }
    if stream.set_read_timeout(Some(timeout)).is_err() || stream.set_write_timeout(Some(timeout)).is_err() { return; }
    let read = || -> Result<Command, String> {
        let mut line = String::new();
        BufReader::new((&stream).take(8 * 1024 * 1024 + 1)).read_line(&mut line).map_err(|e| e.to_string())?;
        if line.len() > 8 * 1024 * 1024 || !line.ends_with('\n') { return Err("Expected a JSON line of at most 8 MiB".into()); }
        serde_json::from_str(&line).map_err(|e| e.to_string())
    };
    let result = read().and_then(|command| {
        let (reply, response) = channel();
        tx.try_send(Request { command, reply, expires: Instant::now() + timeout })
            .map_err(|_| "Game is busy or stopped; refresh before retrying")?;
        response.recv_timeout(timeout).map_err(|_| "Game did not reply; refresh before retrying".into())
    });
    let response = result.unwrap_or_else(|error| serde_json::json!({"ok":false, "error":error}).to_string());
    let _ = writeln!(stream, "{response}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};

    #[test]
    fn strict_wire_commands_reject_unknown_duplicate_and_nonfinite_data() {
        for bad in [r#"{"op":"state","extra":1}"#, r#"{"op":"quit"}"#, r#"{"op":"refresh_scenes","session":1}"#,
            r#"{"op":"set","key":"player_speed"}"#, r#"{"op":"state","op":"save"}"#,
            r#"{"op":"set","key":"player_speed","value":NaN}"#] {
            assert!(serde_json::from_str::<Command>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn socket_ownership_protects_active_servers_and_regular_files() {
        let path = std::env::temp_dir().join(format!("arpg-socket-test-{}.sock", std::process::id()));
        std::fs::write(&path, b"do not remove").unwrap();
        assert!(Server::start(path.clone()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"do not remove");
        std::fs::remove_file(&path).unwrap();
        let stale = UnixListener::bind(&path).unwrap();
        drop(stale);
        let server = Server::start(path.clone()).unwrap();
        assert!(Server::start(path.clone()).is_err());
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        drop(server);
        assert!(!path.exists());
        let server = Server::start(path).unwrap();
        drop(server);
    }

    #[test]
    fn transport_delivers_only_complete_requests_and_replies_after_admission() {
        let path = std::env::temp_dir().join(format!("arpg-wire-test-{}.sock", std::process::id()));
        let server = Server::start(path.clone()).unwrap();
        let client = std::thread::spawn(move || {
            let mut stream = UnixStream::connect(path).unwrap();
            stream.write_all(b"{\"op\":").unwrap();
            std::thread::sleep(Duration::from_millis(30));
            stream.write_all(b"\"state\"}\n").unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        });
        let request = server.requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(request.command, Command::State {}));
        request.reply.send("accepted".into()).unwrap();
        assert_eq!(client.join().unwrap(), "accepted\n");
    }
}
