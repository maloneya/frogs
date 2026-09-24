"""Verification cache cannot hide changed inputs, failed checks or missing evidence."""
import contextlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import verify


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

    def test_arbitrary_inherited_environment_changes_context(self):
        with patch.object(verify.platform, "platform", return_value="test"), \
             patch.object(verify.subprocess, "check_output", return_value="tool version"), \
             patch.dict(verify.os.environ, {"ARPG_TEST_MODE": "one"}):
            first = verify.context()
            with patch.dict(verify.os.environ, {"ARPG_TEST_MODE": "two"}):
                self.assertNotEqual(first["environment"], verify.context()["environment"])

    def test_startup_failure_reports_all_checks_uncertified(self):
        with patch.object(verify, "ROOT", self.root), \
             patch.object(verify, "source_identity", side_effect=ValueError("broken input")), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["full"]), 2)
        result_path = next((self.root / "target/verify").glob("run-*/verification.json"))
        result = json.loads(result_path.read_text())
        self.assertEqual(result["coverage"], [])
        self.assertEqual(result["omitted"], list(verify.CHECKS))
        self.assertFalse(result["complete"])

    def test_changed_source_at_end_cannot_retain_certified_coverage(self):
        def fake(name, *args):
            return {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                    "evidence": [{"path": "test.log"}]}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_identity", side_effect=["before", "after"]), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=fake), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["full"]), 2)
        result_path = next((self.root / "target/verify").glob("run-*/verification.json"))
        result = json.loads(result_path.read_text())
        self.assertEqual(result["coverage"], [])
        self.assertEqual(result["omitted"], list(verify.CHECKS))

    def test_not_due_completion_and_discretionary_benchmarks(self):
        def fake(name, *args):
            return {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                    "evidence": [{"path": "test.log"}]}
        checkpoint = {"revision": "test", "commits_since_pass": {}, "due": []}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_identity", return_value="same"), \
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
        self.assertEqual(verify.performance_checkpoint(self.root, base)["commits_since_pass"],
                         dict.fromkeys(verify.PERFORMANCE, 0))
        for number in range(9):
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
        self.assertEqual(verify.performance_checkpoint(self.root, base)["due"], list(verify.PERFORMANCE))
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
        def fake(name, *args):
            return {"name": name, "cached": False, "exit_code": 0, "seconds": 0,
                    "evidence": [{"path": "test.log"}]}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_identity", return_value="same"), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=fake), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["quick"]), 0)
            results = list((self.root / "target/verify").glob("run-*/verification.json"))
            result = json.loads(results[0].read_text())
            self.assertFalse(result["complete"])
            self.assertEqual(result["status"], "partial_pass")
            self.assertIn("performance-headless", result["omitted"])
        def uncertain(name, *args):
            return {**fake(name), "exit_code": 2}
        with patch.object(verify, "ROOT", self.root), patch.object(verify, "context", return_value={}), \
             patch.object(verify, "source_identity", return_value="same"), \
             patch.object(verify, "commands", return_value=[]), patch.object(verify, "run_check", side_effect=uncertain), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(verify.main(["--only", "clippy"]), 2)


if __name__ == "__main__":
    unittest.main()
