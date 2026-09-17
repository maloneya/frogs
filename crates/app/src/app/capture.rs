//! One pending screenshot, including its destination, deadline and caller.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// A wall-clock bound: skipped redraws may run much faster than refresh rate.
const TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Default)]
pub(super) struct Capture {
    pending: Option<Pending>,
}

struct Pending {
    path: PathBuf,
    requested: Instant,
    reply: Option<Sender<String>>,
}

impl Capture {
    /// Reject overlap rather than silently replacing another caller's path.
    pub(super) fn request(&mut self, path: PathBuf, reply: Option<Sender<String>>, now: Instant) {
        if self.pending.is_some() {
            Self::answer(reply, Err("a screenshot is already pending".into()));
            return;
        }
        self.pending = Some(Pending {
            path,
            requested: now,
            reply,
        });
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
        capture.request("first.png".into(), Some(first), now);
        capture.request("second.png".into(), Some(second), now);
        capture.request("keyboard.png".into(), None, now);
        assert_eq!(capture.path(), Some(Path::new("first.png")));
        assert!(second_reply.try_recv().unwrap().starts_with("error:"));
        assert_eq!(first_reply.try_recv(), Err(TryRecvError::Empty));
        capture.finish(Ok(()));
        assert_eq!(first_reply.try_recv().unwrap(), "ok");
        assert!(capture.path().is_none());
    }

    #[test]
    fn write_failure_reaches_the_caller_and_releases_the_slot() {
        let mut capture = Capture::default();
        let (reply, response) = channel();
        capture.request("failed.png".into(), Some(reply), Instant::now());
        capture.finish(Err("disk full".into()));
        assert_eq!(response.try_recv().unwrap(), "error: disk full");
        assert!(capture.path().is_none());
    }

    #[test]
    fn timeout_depends_on_elapsed_time_not_loop_iterations() {
        let mut capture = Capture::default();
        let now = Instant::now();
        let (reply, response) = channel();
        capture.request("late.png".into(), Some(reply), now);
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

        capture.request("fresh.png".into(), None, now + TIMEOUT);
        capture.expire(now + TIMEOUT);
        assert!(
            capture.path().is_some(),
            "the new request gets its own deadline"
        );
        capture.expire(now + TIMEOUT * 2);
        assert!(capture.path().is_none(), "keyboard captures also expire");
    }
}
