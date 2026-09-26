//! One pending screenshot, including its destination, deadline and caller.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// A wall-clock bound: skipped redraws may run much faster than refresh rate.
const TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Default)]
pub(super) struct Capture {
    pending: Option<Pending>,
    completed: Option<Result<PathBuf, String>>,
}

struct Pending {
    path: PathBuf,
    requested: Instant,
    reply: Option<Sender<String>>,
}

impl Capture {
    /// Reject overlap rather than silently replacing another caller's path.
    pub(super) fn request(&mut self, path: PathBuf, reply: Option<Sender<String>>, now: Instant) -> Result<(), String> {
        if self.pending.is_some() {
            let error = "a screenshot is already pending".to_string();
            Self::answer(reply, Err(error.clone()));
            return Err(error);
        }
        self.completed = None;
        self.pending = Some(Pending {
            path,
            requested: now,
            reply,
        });
        Ok(())
    }

    pub(super) fn path(&self) -> Option<&Path> {
        self.pending.as_ref().map(|pending| pending.path.as_path())
    }

    /// Called only after the renderer reports the PNG write result, or on cancellation.
    pub(super) fn finish(&mut self, result: Result<(), String>) {
        if let Some(pending) = self.pending.take() {
            if result.is_ok() {
                log::info!("captured frame to {}", pending.path.display());
            }
            self.completed = Some(result.clone().map(|()| pending.path));
            Self::answer(pending.reply, result);
        }
    }

    /// Serviced by the event loop even when no redraw is delivered.
    pub(super) fn expire(&mut self, now: Instant) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| now.duration_since(pending.requested) >= TIMEOUT)
        {
            self.finish(Err("no frame was captured within four seconds; the window may be occluded or minimised".into()));
        }
    }

    pub(super) fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "pending":self.path().is_some(),
            "path":self.path().or_else(|| self.completed.as_ref().and_then(|result| result.as_ref().ok()).map(PathBuf::as_path)),
            "error":self.completed.as_ref().and_then(|result| result.as_ref().err())
        })
    }

    fn answer(reply: Option<Sender<String>>, result: Result<(), String>) {
        let answer = match result {
            Ok(()) => "ok".to_string(),
            Err(error) => {
                log::error!("capture failed: {error}");
                format!("error: {error}")
            }
        };
        if let Some(reply) = reply {
            let _ = reply.send(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{TryRecvError, channel};

    #[test]
    fn overlapping_requests_cannot_replace_a_path_or_receive_false_success() {
        let mut capture = Capture::default();
        let now = Instant::now();
        let (first, first_reply) = channel();
        let (second, second_reply) = channel();
        capture.request("first.png".into(), Some(first), now).unwrap();
        assert!(capture.request("second.png".into(), Some(second), now).is_err());
        assert_eq!(capture.path(), Some(Path::new("first.png")));
        assert_eq!(capture.report()["pending"], true);
        assert_eq!(capture.report()["path"], "first.png");
        assert!(second_reply.try_recv().unwrap().starts_with("error:"));
        assert_eq!(first_reply.try_recv(), Err(TryRecvError::Empty));
        capture.finish(Ok(()));
        assert_eq!(first_reply.try_recv().unwrap(), "ok");
        assert_eq!(capture.report()["pending"], false);
        assert_eq!(capture.report()["path"], "first.png");
        assert!(capture.report()["error"].is_null());
        assert!(capture.path().is_none());
    }

    #[test]
    fn write_failure_reaches_the_caller_and_releases_the_slot() {
        let mut capture = Capture::default();
        let (reply, response) = channel();
        capture.request("failed.png".into(), Some(reply), Instant::now()).unwrap();
        capture.finish(Err("disk full".into()));
        assert_eq!(response.try_recv().unwrap(), "error: disk full");
        assert_eq!(capture.report()["error"], "disk full");
        assert_eq!(capture.report()["pending"], false);
        assert!(capture.report()["path"].is_null());
        assert!(capture.path().is_none());
    }

    #[test]
    fn timeout_depends_on_elapsed_time_not_loop_iterations() {
        let mut capture = Capture::default();
        let now = Instant::now();
        let (reply, response) = channel();
        capture.request("late.png".into(), Some(reply), now).unwrap();
        for _ in 0..10_000 {
            capture.expire(now + TIMEOUT - Duration::from_nanos(1));
        }
        assert_eq!(response.try_recv(), Err(TryRecvError::Empty));
        capture.expire(now + TIMEOUT);
        assert!(response.try_recv().unwrap().starts_with("error:"));
        assert!(
            capture.path().is_none(),
            "an expired request cannot write later"
        );

        capture.request("fresh.png".into(), None, now + TIMEOUT).unwrap();
        capture.expire(now + TIMEOUT);
        assert!(
            capture.path().is_some(),
            "the new request gets its own deadline"
        );
        capture.expire(now + TIMEOUT * 2);
        assert!(capture.path().is_none(), "companion captures also expire");
    }
}
