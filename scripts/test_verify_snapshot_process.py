"""Snapshot reservation survives loss of the process waiting for its verifier."""
import fcntl
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


class SnapshotProcesses(unittest.TestCase):
    def wait_until(self, condition, message):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if condition():
                return
            time.sleep(0.02)
        self.fail(message)

    def test_reservation_survives_outer_runner_termination_until_child_finishes(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory).resolve()
            root = base / "repo"
            scripts = root / "scripts"
            scripts.mkdir(parents=True)
            (root / ".gitignore").write_text("/target\n")
            ready, release, finished = [base / name for name in ("ready", "release", "finished")]
            (scripts / "verify.py").write_text("\n".join([
                "import time",
                "from pathlib import Path",
                f"Path({str(ready)!r}).write_text('ready')",
                f"while not Path({str(release)!r}).exists(): time.sleep(0.02)",
                f"Path({str(finished)!r}).write_text('finished')",
            ]))
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            subprocess.run(["git", "-c", "user.name=Test", "-c", "user.email=test@example.com",
                            "commit", "-qm", "initial"], cwd=root, check=True)
            runner = "\n".join([
                "import argparse, sys",
                "from pathlib import Path",
                f"sys.path.insert(0, {str(Path(__file__).resolve().parent)!r})",
                "import verify_snapshot",
                "args = argparse.Namespace(scope='full', only=None, fresh=False, live=False, performance=False)",
                f"raise SystemExit(verify_snapshot.run(Path({str(root)!r}), args))",
            ])
            with (base / "outer.log").open("w+") as output:
                outer = subprocess.Popen([sys.executable, "-B", "-c", runner],
                                         stdout=output, stderr=subprocess.STDOUT,
                                         start_new_session=True)
                try:
                    self.wait_until(lambda: ready.exists() or outer.poll() is not None,
                                    "captured verifier did not start")
                    output.flush()
                    output.seek(0)
                    self.assertTrue(ready.exists(), output.read())
                    outer.terminate()
                    self.assertEqual(outer.wait(timeout=10), -signal.SIGTERM)
                    with (root / "target/verify/snapshot/lock").open("a") as lock:
                        with self.assertRaises(BlockingIOError,
                                              msg="surviving verifier lost the snapshot reservation"):
                            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        release.write_text("finish")
                        self.wait_until(finished.exists, "captured verifier did not finish")

                        def released():
                            try:
                                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                                return True
                            except BlockingIOError:
                                return False

                        self.wait_until(released, "finished verifier retained the snapshot reservation")
                finally:
                    # The isolated process group includes an orphaned verifier
                    # if an assertion fails after the outer runner is stopped.
                    try:
                        os.killpg(outer.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    outer.wait(timeout=10)


if __name__ == "__main__":
    unittest.main()
