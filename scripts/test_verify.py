"""Verification cache cannot hide changed inputs, failed checks or missing evidence."""
import argparse
import contextlib
import fcntl
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import verify
import verify_snapshot
import verify_summary


class Workspace(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        (self.root / ".gitignore").write_text("/target\n")
        (self.root / "source.rs").write_text("first")
        self.artifact = self.root / "target/run-one"
        self.artifact.mkdir(parents=True)
        self.cache = self.root / "target/cache"
        self.machine = {"toolchain": "test"}
        subprocess.run(["git", "add", "."], cwd=self.root, check=True)
        subprocess.run(["git", "-c", "user.name=Test", "-c", "user.email=test@example.com",
                        "commit", "-qm", "initial"], cwd=self.root, check=True)

    def run_check(self, script, fresh=False):
        return verify.run_check("tests", [sys.executable, "-c", script],
                                verify.source_identity(self.root), self.machine,
                                self.artifact, self.cache, fresh, self.root)

    def next_run(self):
        self.artifact = Path(tempfile.mkdtemp(dir=self.root / "target"))

    def snapshot(self):
        state = self.root / "target/snapshot"
        state.mkdir(exist_ok=True)
        workspace = state / "workspace"
        return workspace, verify_snapshot.capture(self.root, workspace, self.artifact)

    def test_snapshot_preserves_dirty_new_deleted_executable_and_linked_inputs(self):
        (self.root / "removed.rs").write_text("remove me")
        subprocess.run(["git", "add", "removed.rs"], cwd=self.root, check=True)
        (self.root / "removed.rs").unlink()  # Staged addition, unstaged deletion.
        (self.root / "source.rs").write_text("dirty content")
        script = self.root / "new.sh"
        script.write_text("#!/bin/sh\nexit 0\n")
        script.chmod(0o755)
        (self.root / "linked").symlink_to("source.rs")
        workspace, captured = self.snapshot()
        self.assertEqual(verify.source_identity(workspace), verify.source_identity(self.root))
        self.assertEqual((workspace / "source.rs").read_text(), "dirty content")
        self.assertFalse((workspace / "removed.rs").exists())
        self.assertEqual(os.readlink(workspace / "linked"), "source.rs")
        self.assertEqual((workspace / "new.sh").stat().st_mode & 0o777, 0o755)
        self.assertEqual(verify_snapshot.git(workspace, "rev-parse", "HEAD"), verify_snapshot.git(self.root, "rev-parse", "HEAD"))
        self.assertEqual(verify_snapshot.git(workspace, "ls-files", "--stage", "-z"),
                         verify_snapshot.git(self.root, "ls-files", "--stage", "-z"))
        with tarfile.open(captured["archive"]) as archive:
            self.assertEqual(archive.extractfile("source.rs").read(), b"dirty content")
            self.assertNotIn("removed.rs", archive.getnames())

    def test_snapshot_preserves_staged_deletion_and_ignores_build_outputs(self):
        subprocess.run(["git", "rm", "-q", "source.rs"], cwd=self.root, check=True)
        (self.root / "target/live-build").write_text("not a source input")
        workspace, _ = self.snapshot()
        self.assertFalse((workspace / "source.rs").exists())
        self.assertFalse((workspace / "target/live-build").exists())
        self.assertEqual(verify.source_identity(workspace), verify.source_identity(self.root))

    def test_snapshot_git_operations_cannot_be_redirected_into_live_metadata(self):
        index = self.root / ".git/index"
        head = self.root / ".git/HEAD"
        original = (index.read_bytes(), head.read_bytes())
        with patch.dict(os.environ, {"GIT_DIR": str(self.root / ".git"),
                                     "GIT_WORK_TREE": str(self.root), "GIT_INDEX_FILE": str(index)}):
            workspace, _ = self.snapshot()
        self.assertEqual((index.read_bytes(), head.read_bytes()), original)
        self.assertEqual(verify.source_identity(workspace), verify.source_identity(self.root))

    def test_failed_capture_with_inherited_index_never_mutates_live_index(self):
        index = self.root / ".git/index"
        before = index.read_bytes()
        git = verify_snapshot.git
        def fail_update(root, *args, **kwargs):
            if args[0] == "update-index":
                raise OSError("capture failed after read-tree")
            return git(root, *args, **kwargs)
        with patch.dict(os.environ, {"GIT_INDEX_FILE": str(index)}), \
             patch.object(verify_snapshot, "git", side_effect=fail_update), \
             self.assertRaisesRegex(OSError, "capture failed"):
            self.snapshot()
        self.assertEqual(index.read_bytes(), before)

    def test_running_check_inherits_snapshot_reservation(self):
        with (self.root / "target/lock").open("w") as lock:
            row = verify.run_check("tests", [sys.executable, "-c", f"import os; os.fstat({lock.fileno()})"],
                                   verify.source_identity(self.root), self.machine, self.artifact, self.cache,
                                   root=self.root, pass_fds=(lock.fileno(),))
        self.assertEqual(row["exit_code"], 0)

    def test_snapshot_refuses_external_links_and_changes_during_copy(self):
        (self.root / "external").symlink_to(self.temp.name)  # Absolute links can escape capture.
        with self.assertRaises(ValueError):
            self.snapshot()
        (self.root / "external").unlink()
        copy = verify_snapshot.shutil.copy2
        def mutate(source, destination):
            result = copy(source, destination)
            if source.name == "source.rs":
                (self.root / "added-during-capture").write_text("new input")
            return result
        with patch.object(verify_snapshot.shutil, "copy2", side_effect=mutate), \
             self.assertRaisesRegex(ValueError, "changed during snapshot capture"):
            self.snapshot()
        self.assertFalse((self.root / "target/snapshot/workspace").exists())

    def test_snapshot_checks_survive_live_edits_but_reject_their_own_mutations(self):
        workspace, captured = self.snapshot()
        command = [sys.executable, "-c", "from pathlib import Path; "
                   "assert Path('source.rs').read_text() == 'first'; "
                   f"Path({str(self.root / 'source.rs')!r}).write_text('live edit')"]
        row = verify.run_check("tests", command, captured["source"], self.machine,
                               self.artifact, self.cache, root=workspace)
        self.assertEqual(row["exit_code"], 0)
        comparison = verify_snapshot.live_comparison(captured)
        self.assertFalse(comparison["matches"])
        self.assertEqual(comparison["changed_paths"], ["source.rs"])
        self.assertEqual((workspace / "source.rs").read_text(), "first")
        self.next_run()
        row = verify.run_check("tests", [sys.executable, "-c", "from pathlib import Path; Path('source.rs').write_text('bad')"],
                               captured["source"], self.machine, self.artifact, self.cache, root=workspace)
        self.assertEqual(row["exit_code"], 2)
        self.assertIn("inputs changed", row["reason"])

    def test_snapshot_reuses_private_build_directory_and_replaces_only_its_workspace(self):
        workspace, _ = self.snapshot()
        (workspace / "target/cache-marker").write_text("retain")
        (self.root / "source.rs").write_text("second")
        self.next_run()
        next_workspace, captured = self.snapshot()
        self.assertEqual(next_workspace, workspace)
        self.assertEqual((workspace / "target/cache-marker").read_text(), "retain")
        self.assertEqual((workspace / "source.rs").read_text(), "second")
        self.assertTrue(verify_snapshot.live_comparison(captured)["matches"])

    def test_snapshot_lock_refuses_concurrent_runs_without_touching_reserved_state(self):
        state = self.root / "target/verify/snapshot"
        state.mkdir(parents=True)
        marker = state / "workspace"
        marker.mkdir()
        (marker / "reserved").write_text("do not touch")
        args = argparse.Namespace(scope="full", only=None, fresh=False, live=False, performance=False)
        with (state / "lock").open("a") as lock, contextlib.redirect_stdout(io.StringIO()):
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(verify_snapshot.run(self.root, args), 2)
        self.assertEqual((marker / "reserved").read_text(), "do not touch")
        result = json.loads(next((self.root / "target/verify").glob("run-*/verification.json")).read_text())
        self.assertEqual(result["coverage"], [])
        self.assertIn("another snapshot verification", result["reason"])

    def test_snapshot_runner_reports_captured_child_result_despite_live_edits(self):
        scripts = self.root / "scripts"
        scripts.mkdir()
        (scripts / "verify.py").write_text("\n".join([
            "import json, os, sys",
            "from pathlib import Path",
            "artifact = Path(sys.argv[sys.argv.index('--snapshot-result') + 1])",
            "assert '--fresh' in sys.argv and '--only' in sys.argv",
            "assert Path(os.environ['CARGO_TARGET_DIR']) == Path.cwd().parent / 'build'",
            "assert Path('source.rs').read_text() == 'first'",
            f"Path({str(self.root / 'source.rs')!r}).write_text('work continued')",
            "result = dict(status='partial_pass', coverage=['tests'], omitted=['clippy'], complete=False)",
            "(artifact / 'verification.json').write_text(json.dumps(result))",
        ]))
        args = argparse.Namespace(scope="full", only="tests", fresh=True, live=False, performance=False)
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify_snapshot.run(self.root, args), 0)
        result = json.loads(next((self.root / "target/verify").glob("run-*/verification.json")).read_text())
        self.assertEqual(result["status"], "partial_pass")
        self.assertEqual(result["coverage"], ["tests"])
        self.assertEqual(result["live_checkout"]["changed_paths"], ["source.rs"])
        self.assertEqual(verify.source_identity(Path(result["snapshot"]["workspace"])), result["snapshot"]["source"])

    def test_success_reuses_exact_inputs_but_missing_or_changed_evidence_reruns(self):
        first = self.run_check("print('tested')")
        self.assertEqual(first["exit_code"], 0)
        self.next_run()
        second = self.run_check("print('tested')")
        self.assertTrue(second["cached"])
        log = Path(first["evidence"][0]["path"])
        log.write_text("corrupted")
        self.next_run()
        third = self.run_check("print('tested')")
        self.assertFalse(third["cached"])
        Path(third["evidence"][0]["path"]).unlink()
        self.next_run()
        self.assertFalse(self.run_check("print('tested')")["cached"])

    def test_source_untracked_content_toolchain_command_and_deletion_invalidate(self):
        first = self.run_check("print('tested')")
        previous = first["key"]
        for change in [lambda: (self.root / "source.rs").write_text("changed"),
                       lambda: (self.root / "scene.ron").write_text("new untracked scene"),
                       lambda: (self.root / "source.rs").unlink(),
                       lambda: self.machine.update(toolchain="new compiler")]:
            change()
            self.next_run()
            row = self.run_check("print('tested')")
            self.assertFalse(row["cached"])
            self.assertNotEqual(row["key"], previous)
            previous = row["key"]
        self.next_run()
        self.assertFalse(self.run_check("print('different command')")["cached"])

    def test_failed_check_and_changed_source_during_check_never_cache(self):
        row = self.run_check("raise SystemExit(1)")
        self.assertEqual(row["exit_code"], 1)
        self.assertFalse(self.cache.exists())
        self.next_run()
        row = self.run_check("from pathlib import Path; Path('source.rs').write_text('mutated')")
        self.assertEqual(row["exit_code"], 2)
        self.assertIn("changed", row["reason"])
        self.assertFalse(self.cache.exists())

    def test_explicit_fresh_ignores_successful_cache(self):
        self.run_check("print('tested')")
        self.next_run()
        self.assertFalse(self.run_check("print('tested')", fresh=True)["cached"])

    def test_infrastructure_failure_defers_only_identical_automatic_retries(self):
        name = "performance-headless"
        def attempt(retry=False, kind="infrastructure", code=2):
            output = self.artifact / (name + ".json")
            script = ("import json; from pathlib import Path; "
                      f"Path({str(output)!r}).write_text(json.dumps({{'failure': "
                      f"{{'kind': {kind!r}, 'stage': 'baseline_preflight', 'reason': 'bad archive'}}}})); "
                      f"raise SystemExit({code})")
            return verify.run_check(name, [sys.executable, "-c", script],
                                    verify.source_identity(self.root), self.machine,
                                    self.artifact, self.cache, root=self.root, retry=retry)
        with patch.object(verify.perf, "evidence_identity", return_value="same"):
            first = attempt()
            self.next_run()
            second = attempt()
            self.assertTrue(second["deferred"])
            self.assertEqual(second["exit_code"], 2)
            self.assertEqual(second["evidence"], first["evidence"])
            self.assertFalse(second["cached"])
            self.next_run()
            self.assertNotIn("deferred", attempt(retry=True))
            self.next_run()
            with patch.object(verify.perf, "evidence_identity", return_value="repaired"):
                self.assertNotIn("deferred", attempt())
            (self.root / "source.rs").write_text("fixed tooling")
            self.next_run()
            changed = attempt()
            self.assertNotIn("deferred", changed)
            Path(changed["evidence"][0]["path"]).write_text("damaged log")
            self.next_run()
            self.assertNotIn("deferred", attempt())
            self.next_run()
            self.machine["toolchain"] = "updated"
            self.assertNotIn("deferred", attempt())
            self.next_run()
            # A successful explicit retry removes the failure, without caching timings.
            self.assertEqual(attempt(retry=True, code=0)["exit_code"], 0)

    def test_noisy_measurement_is_never_deferred(self):
        name = "performance-live"
        for _ in range(2):
            output = self.artifact / (name + ".json")
            script = ("from pathlib import Path; "
                      f"Path({str(output)!r}).write_text('{{\"assessment\": {{\"status\": \"inconclusive\"}}}}'); "
                      "raise SystemExit(2)")
            row = verify.run_check(name, [sys.executable, "-c", script],
                                   verify.source_identity(self.root), self.machine,
                                   self.artifact, self.cache, root=self.root)
            self.assertNotIn("deferred", row)
            self.assertEqual(row["exit_code"], 2)
            self.next_run()

    def test_performance_build_diagnostic_is_preserved_and_sidecar_damage_retries(self):
        def attempt():
            output = self.artifact / "performance-headless.json"
            build = Path(str(output) + ".build.log")
            failure = {"failure": {"kind": "infrastructure", "stage": "build", "reason": "release build failed; inspect build log"}}
            script = ("from pathlib import Path; "
                      f"Path({str(output)!r}).write_text({json.dumps(failure)!r}); "
                      f"Path({str(build)!r}).write_text('error[E0308]: mismatched types\\n  --> crates/core/src/instance.rs:172:61\\n'); "
                      "raise SystemExit(2)")
            return verify.run_check("performance-headless", [sys.executable, "-c", script],
                                    verify.source_identity(self.root), self.machine, self.artifact, self.cache, root=self.root)
        row = attempt()
        summary = verify_summary.failure(row, self.root)
        self.assertEqual(summary["category"], "infrastructure")
        self.assertIn("mismatched types", summary["diagnostic"])
        self.assertTrue(summary["evidence"][0].endswith(".build.log"))
        self.assertEqual(summary["affected_paths"], ["crates/core/src/instance.rs"])
        self.next_run()
        self.assertTrue(attempt()["deferred"])
        Path(summary["evidence"][0]).write_text("damaged")
        self.next_run()
        self.assertNotIn("deferred", attempt())

    def test_full_reports_deferred_failure_but_runs_all_correctness(self):
        def fake(name, *args, **kwargs):
            row = {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                   "evidence": [{"path": "test.log"}]}
            if name in verify.PERFORMANCE:
                row.update(exit_code=2, deferred=True,
                           infrastructure_failure={"reason": "bad archive"})
            return row
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_manifest", return_value={"source.rs": "same"}), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=fake) as run, \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["full"]), 2)
            result = json.loads(next((self.root / "target/verify").glob("run-*/verification.json")).read_text())
            self.assertFalse(result["complete"])
            self.assertEqual(result["coverage"], list(verify.CORRECTNESS))
            self.assertEqual(result["omitted"], list(verify.PERFORMANCE))
            run.reset_mock()
            self.assertEqual(verify.main(["correctness"]), 0)
            self.assertEqual([call.args[0] for call in run.call_args_list], list(verify.CORRECTNESS))

    def test_symlink_retargeting_and_external_content_invalidate_identity(self):
        outside = self.root / "target/outside"
        outside.write_text("same contents")
        other = self.root / "target/other"
        other.write_text("same contents")
        link = self.root / "input"
        link.symlink_to(outside)
        before = verify.source_identity(self.root)
        link.unlink()
        link.symlink_to(other)
        self.assertNotEqual(verify.source_identity(self.root), before)
        before = verify.source_identity(self.root)
        other.write_text("new contents")
        self.assertNotEqual(verify.source_identity(self.root), before)

    def test_directory_symlink_tracks_nested_contents_names_and_retargeting(self):
        target = self.root / "target/skill"
        (target / "nested").mkdir(parents=True)
        source = target / "nested/SKILL.md"
        source.write_text("instructions")
        link = self.root / "skill"
        link.symlink_to(target, target_is_directory=True)
        before = verify.source_identity(self.root)
        source.write_text("new instructions")
        self.assertNotEqual(verify.source_identity(self.root), before)
        before = verify.source_identity(self.root)
        source.rename(target / "nested/RENAMED.md")
        self.assertNotEqual(verify.source_identity(self.root), before)
        other = self.root / "target/other"
        (other / "nested").mkdir(parents=True)
        (other / "nested/RENAMED.md").write_text("new instructions")
        before = verify.source_identity(self.root)
        link.unlink()
        link.symlink_to(other, target_is_directory=True)
        self.assertNotEqual(verify.source_identity(self.root), before)

    def test_link_cycles_fail_closed_and_shared_targets_are_not_cycles(self):
        target = self.root / "target/skill"
        target.mkdir()
        (self.root / "first").symlink_to(target, target_is_directory=True)
        (self.root / "second").symlink_to(target, target_is_directory=True)
        verify.source_identity(self.root)
        (target / "cycle").symlink_to(target, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "cycle"):
            verify.source_identity(self.root)
        (target / "cycle").unlink()
        (target / "a").symlink_to("b")
        (target / "b").symlink_to("a")
        with self.assertRaisesRegex(ValueError, "cycle"):
            verify.source_identity(self.root)

    def test_missing_structured_evidence_cannot_certify_a_scenario(self):
        row = verify.run_check("scenarios-fast", [sys.executable, "-c", "pass"],
                               verify.source_identity(self.root), self.machine,
                               self.artifact, self.cache, root=self.root)
        self.assertEqual(row["exit_code"], 2)
        self.assertFalse(self.cache.exists())

    def test_completion_reports_failed_scenario_without_reading_log(self):
        command = [sys.executable, "-c", "print('FAIL  position\\n        player x\\n          expected: 1\\n          actual: 2'); raise SystemExit(1)",
                   "scenarios/position.ron"]
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "commands", return_value=command), \
             contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(verify.main(["--only", "scenarios-fast"]), 1)
        result = json.loads(next((self.root / "target/verify").glob("run-*/verification.json")).read_text())
        failure = result["failures"][0]
        self.assertEqual(failure["category"], "scenario")
        self.assertEqual(failure["affected_paths"], ["scenarios/position.ron"])
        self.assertIn("actual: 2", output.getvalue())
        self.assertIn(failure["retry_command"], output.getvalue())
        self.assertIn("scenarios-fast", result["omitted"])
        self.assertEqual(result["coverage"], [])

    def test_completion_reports_changed_paths_once_and_keeps_coverage_empty(self):
        command = [sys.executable, "-c", "from pathlib import Path; [Path(f'new-{i:02}.rs').write_text('changed') for i in range(20)]"]
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "commands", return_value=command), \
             contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(verify.main(["--only", "tests"]), 2)
        result = json.loads(next((self.root / "target/verify").glob("run-*/verification.json")).read_text())
        expected = [f"new-{i:02}.rs" for i in range(20)]
        self.assertEqual(result["source_changes"], expected)
        self.assertEqual(result["failures"][0]["affected_paths"], expected[:10])
        self.assertEqual(result["failures"][0]["affected_path_count"], 20)
        self.assertEqual(output.getvalue().count("[source-change]"), 1)
        self.assertEqual(result["coverage"], [])

    def test_arbitrary_inherited_environment_changes_context(self):
        with patch.object(verify.platform, "platform", return_value="test"), \
             patch.object(verify.subprocess, "check_output", return_value="tool version"), \
             patch.dict(verify.os.environ, {"ARPG_TEST_MODE": "one"}):
            first = verify.context()
            with patch.dict(verify.os.environ, {"ARPG_TEST_MODE": "two"}):
                self.assertNotEqual(first["environment"], verify.context()["environment"])

    def test_startup_failure_reports_all_checks_uncertified(self):
        with patch.object(verify, "ROOT", self.root), \
             patch.object(verify, "source_manifest", side_effect=ValueError("broken input")), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["full"]), 2)
        result_path = next((self.root / "target/verify").glob("run-*/verification.json"))
        result = json.loads(result_path.read_text())
        self.assertEqual(result["coverage"], [])
        self.assertEqual(result["omitted"], list(verify.CHECKS))
        self.assertFalse(result["complete"])

    def test_changed_source_at_end_cannot_retain_certified_coverage(self):
        def fake(name, *args, **kwargs):
            return {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                    "evidence": [{"path": "test.log"}]}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_manifest", side_effect=[{"source.rs": "before"}, {"source.rs": "after"}]), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=fake), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["full"]), 2)
        result_path = next((self.root / "target/verify").glob("run-*/verification.json"))
        result = json.loads(result_path.read_text())
        self.assertEqual(result["coverage"], [])
        self.assertEqual(result["omitted"], list(verify.CHECKS))

    def test_not_due_completion_and_discretionary_benchmarks(self):
        def fake(name, *args, **kwargs):
            return {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                    "evidence": [{"path": "test.log"}]}
        checkpoint = {"revision": "test", "commits_since_pass": {}, "due": []}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_manifest", return_value={"source.rs": "same"}), \
             patch.object(verify, "performance_checkpoint", return_value=checkpoint), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=fake) as run, \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["full"]), 0)
            self.assertEqual([call.args[0] for call in run.call_args_list], list(verify.CORRECTNESS))
            result = json.loads(next((self.root / "target/verify").glob("run-*/verification.json")).read_text())
            self.assertTrue(result["complete"])
            self.assertEqual(result["omitted"], [])
            run.reset_mock()
            self.assertEqual(verify.main(["full", "--performance"]), 0)
            self.assertEqual([call.args[0] for call in run.call_args_list], list(verify.CHECKS))

    def test_automatic_runs_never_launch_live_and_keep_due_evidence_omitted(self):
        def fake(name, *args, **kwargs):
            return {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                    "evidence": [{"path": "test.log"}]}
        selections = {
            "quick": ["clippy", "tests", "scenarios-fast"],
            "correctness": list(verify.CORRECTNESS),
            "full": list(verify.CORRECTNESS) + ["performance-headless"],
        }
        checkpoint = {"revision": "test", "commits_since_pass": {}, "due": list(verify.PERFORMANCE)}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_manifest", return_value={"source.rs": "same"}), \
             patch.object(verify, "performance_checkpoint", return_value=checkpoint), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=fake) as run:
            for scope, expected in selections.items():
                # Repetition and --fresh must not turn outstanding live work into permission.
                for flags in ([], [], ["--fresh"]):
                    with self.subTest(scope=scope, flags=flags), contextlib.redirect_stdout(io.StringIO()) as output:
                        before = set((self.root / "target/verify").glob("run-*/verification.json"))
                        run.reset_mock()
                        self.assertEqual(verify.main([scope] + flags), 0)
                        self.assertEqual([call.args[0] for call in run.call_args_list], expected)
                        after = set((self.root / "target/verify").glob("run-*/verification.json"))
                        result = json.loads((after - before).pop().read_text())
                        self.assertEqual(result["status"], "partial_pass")
                        self.assertFalse(result["complete"])
                        self.assertEqual(result["required"], list(verify.CHECKS))
                        self.assertIn("performance-live", result["omitted"])
                        self.assertEqual(result["performance_checkpoint"], checkpoint)
                        self.assertIn("Live performance due; not launched", output.getvalue())
                        self.assertIn("--only performance-live", output.getvalue())

    def test_live_only_due_is_reported_without_automatic_checks(self):
        checkpoint = {"revision": "test", "commits_since_pass": {}, "due": ["performance-live"]}
        def fake(name, *args, **kwargs):
            return {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                    "evidence": [{"path": "test.log"}]}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_manifest", return_value={"source.rs": "same"}), \
             patch.object(verify, "performance_checkpoint", return_value=checkpoint), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=fake) as run, \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["full"]), 0)
        self.assertEqual([call.args[0] for call in run.call_args_list], list(verify.CORRECTNESS))
        result = json.loads(next((self.root / "target/verify").glob("run-*/verification.json")).read_text())
        self.assertEqual(result["omitted"], ["performance-live"])
        self.assertEqual(result["status"], "partial_pass")

    def test_explicit_live_requests_run_even_when_not_due_and_retry_failures(self):
        checkpoint = {"revision": "test", "commits_since_pass": {}, "due": []}
        def fake(name, *args, **kwargs):
            return {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                    "evidence": [{"path": "test.log"}]}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_manifest", return_value={"source.rs": "same"}), \
             patch.object(verify, "performance_checkpoint", return_value=checkpoint), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=fake) as run, \
             contextlib.redirect_stdout(io.StringIO()):
            for flags, expected in [
                (["--live"], list(verify.CORRECTNESS) + ["performance-live"]),
                (["--performance"], list(verify.CHECKS)),
                (["--only", "performance-live"], ["performance-live"]),
            ]:
                with self.subTest(flags=flags):
                    run.reset_mock()
                    self.assertEqual(verify.main(flags), 0)
                    self.assertEqual([call.args[0] for call in run.call_args_list], expected)
                    for call in run.call_args_list:
                        if call.args[0] in verify.PERFORMANCE:
                            self.assertTrue(call.kwargs["retry"])

    def test_checkpoint_counts_relevant_commits_and_rejects_damaged_evidence(self):
        base = self.root / "target/verify"
        run = base / "run-checkpoint"
        run.mkdir(parents=True)
        log = run / "pass.log"
        log.write_text("passed")
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.root, text=True).strip()
        record = {"revision": revision, "status": "complete_pass", "checks": [
            {"name": name, "exit_code": 0, "evidence": [
                {"path": str(log), "sha256": verify.digest(log.read_bytes())}]}
            for name in verify.PERFORMANCE]}
        evidence = run / "verification.json"
        evidence.write_text(json.dumps(record))
        def commit(path, value):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(value)
            subprocess.run(["git", "add", "."], cwd=self.root, check=True)
            subprocess.run(["git", "-c", "user.name=Test", "-c", "user.email=test@example.com",
                            "commit", "-qm", "change"], cwd=self.root, check=True)
        commit(self.root / "crates/README.md", "documentation only")
        commit(self.root / "scenes/play.ron", "playable scene experiment")
        self.assertEqual(verify.performance_checkpoint(self.root, base)["commits_since_pass"],
                         dict.fromkeys(verify.PERFORMANCE, 0))
        commit(self.root / "benchmarks/compressed.ron", "benchmark workload change")
        self.assertEqual(verify.performance_checkpoint(self.root, base)["commits_since_pass"],
                         dict.fromkeys(verify.PERFORMANCE, 1))
        for number in range(8):
            commit(self.root / "crates/example.rs", str(number))
        self.assertEqual(verify.performance_checkpoint(self.root, base)["due"], [])
        (self.root / "crates/example.rs").write_text("dirty")
        self.assertEqual(verify.performance_checkpoint(self.root, base)["due"], [])
        commit(self.root / "crates/example.rs", "tenth")
        self.assertEqual(verify.performance_checkpoint(self.root, base)["due"], list(verify.PERFORMANCE))
        record["revision"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=self.root, text=True).strip()
        record["checks"] = record["checks"][:1]
        evidence.write_text(json.dumps(record))
        self.assertEqual(verify.performance_checkpoint(self.root, base)["due"], ["performance-live"])
        record["status"] = "inconclusive"
        evidence.write_text(json.dumps(record))
        # A failing sibling check cannot discard this mode's successful checkpoint.
        self.assertEqual(verify.performance_checkpoint(self.root, base)["due"], ["performance-live"])
        record["reason"] = "inputs changed during verification"
        evidence.write_text(json.dumps(record))
        self.assertEqual(verify.performance_checkpoint(self.root, base)["due"], list(verify.PERFORMANCE))
        del record["reason"]
        record["status"] = "complete_pass"
        evidence.write_text(json.dumps(record))
        log.write_text("damaged")
        self.assertEqual(verify.performance_checkpoint(self.root, base)["due"], list(verify.PERFORMANCE))

    def test_suite_partition_covers_all_files_and_stale_manifest_fails(self):
        directory = self.root / "scenarios"
        directory.mkdir()
        for name in ("short.ron", "long.ron", "new.ron"):
            (directory / name).write_text("()")
        (directory / "nested").mkdir()
        # Duplicate basenames still identify separate scenarios. Nested files
        # enter fast by default, just like scenarios at the top level.
        (directory / "nested/short.ron").write_text("()")
        (directory / "nested/long.ron").write_text("()")
        manifest = directory / "suites.json"
        manifest.write_text(json.dumps({"extended": {"long.ron": "scale", "nested/long.ron": "duration"}}))
        fast = verify.scenario_files("fast", self.root)
        extended = verify.scenario_files("extended", self.root)
        self.assertEqual(fast, ["scenarios/nested/short.ron", "scenarios/new.ron", "scenarios/short.ron"])
        self.assertEqual(extended, ["scenarios/long.ron", "scenarios/nested/long.ron"])
        manifest.write_text(json.dumps({"extended": {"deleted.ron": "stale"}}))
        with self.assertRaises(ValueError):
            verify.scenario_files("fast", self.root)

    def test_partial_run_cannot_report_complete_and_uncertainty_stays_nonzero(self):
        def fake(name, *args, **kwargs):
            return {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                    "evidence": [{"path": "test.log"}]}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_manifest", return_value={"source.rs": "same"}), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=fake), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["quick"]), 0)
            results = list((self.root / "target/verify").glob("run-*/verification.json"))
            result = json.loads(results[0].read_text())
            self.assertFalse(result["complete"])
            self.assertEqual(result["status"], "partial_pass")
            self.assertIn("performance-headless", result["omitted"])
        def uncertain(name, *args, **kwargs):
            return {**fake(name), "exit_code": 2}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_manifest", return_value={"source.rs": "same"}), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=uncertain), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["--only", "clippy"]), 2)


if __name__ == "__main__":
    unittest.main()
