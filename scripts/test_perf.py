"""Evidence-policy regressions; synthetic timings make these hardware independent."""
import copy
import contextlib
import io
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest
from unittest.mock import patch

import perf


def fixture():
    workload = {"name": "sparse", "scene": "sparse.ron", "warmup_ticks": 120,
                "sample_ticks": 60, "input": "idle", "animation": "idle",
                "budgets_ms": {"mean_step_ms": 13, "frame_ms.p99": 25}}
    observation = {
        "schema_version": 1, "debug_build": False, "collision_debug": False,
        "vsync": True, "width": 1280, "height": 720, "scale_factor": 1,
        "view_height": 32,
        "refresh_millihertz": 60000, "gpu": {"name": "test"},
        "surface_skipped": 0, "unfocused_frames": 0, "occluded_frames": 0,
        "overwritten": 0, "samples": 60, "presented": 60, "ticks": 60,
        "window_ms": 1020, "current_tick": 180, "current_enemies": 100,
        "mean_step_ms": 10, "presentation_ms": {"mean": 1}, "frame_ms": {"p99": 17},
        "horde": {"instances": 100, "idle_instances": 100},
        "frames": {str(i): {"ticks": 1, "end_tick": 121 + i, "enemies": 100,
                            "presented": True, "focused": True, "occluded": False, "frame_ms": 17,
                            "game_step_ms": 10, "presentation_ms": 1}
                   for i in range(60)},
    }
    for asset in ("horde", "character"):
        observation.setdefault(asset, {}).update(active=True, vertices=456, indices=684,
                                                 texture_width=64, texture_height=64, nodes=9, joints=7)
    trial = {"trial": {"warmup_ticks": 120, "sample_ticks": 60, "input": "idle",
                       "schedule_version": 1, "invalid": "", "initial_enemies": 100,
                       "start_enemies": 100, "population_sum": 6000},
             "final_hash": "0123456789abcdef", "finite": True, "perf": observation}
    return {"schema_version": 1, "errors": [],
            "source": {"revision": "a" * 40, "digest": "b" * 64, "dirty": False},
            "binary_sha256": "c" * 64, "workload_digest": "d" * 64,
            "conditions": {"machine": "test", "cpu": "test", "memory_bytes": "1024", "os": "test",
                           "rustc": "test", "build": "cargo build --release -p arpg", "build_environment": {}},
            "suite": {"version": 1, "repetitions": 3, "relative_tolerance": 0.1,
                      "max_trial_spread": 0.15, "workloads": [workload]},
            "workloads": {"sparse": [copy.deepcopy(trial) for _ in range(3)]}}


def set_step_ms(trial, milliseconds):
    trial["perf"]["mean_step_ms"] = milliseconds
    for frame in trial["perf"]["frames"].values():
        frame["game_step_ms"] = milliseconds * frame["ticks"]


class EvidenceTests(unittest.TestCase):
    def test_stable_same_work_passes_and_slower_work_regresses(self):
        base = fixture()
        self.assertEqual(perf.assess(base, base)["status"], "pass")
        slow = copy.deepcopy(base)
        for trial in slow["workloads"]["sparse"]:
            set_step_ms(trial, 12)
        result = perf.assess(slow, base)
        self.assertEqual(result["status"], "regression")
        self.assertEqual(result["metrics"][0]["absolute"], "pass")
        self.assertEqual(result["metrics"][0]["relative"], "regression")

    def test_absolute_budget_needs_no_baseline(self):
        result = fixture()
        self.assertEqual(perf.assess(result)["metrics"][1]["status"], "observed")
        for trial in result["workloads"]["sparse"]:
            trial["perf"]["frame_ms"]["p99"] = 30
            trial["perf"]["window_ms"] = 1800
            for frame in trial["perf"]["frames"].values():
                frame["frame_ms"] = 30
        self.assertEqual(perf.assess(result)["status"], "regression")

    def test_a_best_sample_cannot_hide_unstable_repetitions(self):
        result = fixture()
        set_step_ms(result["workloads"]["sparse"][1], 20)
        self.assertEqual(perf.assess(result, fixture())["status"], "inconclusive")

    def test_threshold_overlap_is_not_a_pass(self):
        result = fixture()
        for trial, duration in zip(result["workloads"]["sparse"], [10.9, 11.1, 11.2]):
            set_step_ms(trial, duration)
        self.assertEqual(perf.assess(result, fixture())["status"], "inconclusive")

    def test_invalid_measurement_windows_cannot_pass(self):
        mutations = [
            ("debug_build", True), ("unfocused_frames", 1), ("occluded_frames", 1),
            ("surface_skipped", 1), ("overwritten", 1), ("ticks", 59),
            ("samples", 61), ("current_tick", 181), ("current_enemies", 99),
            ("width", 0), ("mean_step_ms", None), ("mean_step_ms", float("nan")),
        ]
        for key, value in mutations:
            with self.subTest(key=key):
                result = fixture()
                result["workloads"]["sparse"][0]["perf"][key] = value
                self.assertEqual(perf.assess(result)["status"], "inconclusive")

    def test_unfocused_raw_frame_cannot_hide_behind_good_summary(self):
        result = fixture()
        result["workloads"]["sparse"][0]["perf"]["frames"]["10"]["focused"] = False
        self.assertEqual(perf.assess(result)["status"], "inconclusive")

    def test_raw_durations_cannot_contradict_a_passing_summary(self):
        for field in ("frame_ms", "game_step_ms", "presentation_ms"):
            result = fixture()
            for frame in result["workloads"]["sparse"][0]["perf"]["frames"].values():
                frame[field] = 1000
            verdict = perf.assess(result)
            self.assertEqual(verdict["status"], "inconclusive")
            self.assertIn("summary contradicts raw timings", verdict["reasons"][0])

    def test_report_rounding_and_json_key_order_preserve_valid_evidence(self):
        result = fixture()
        for trial in result["workloads"]["sparse"]:
            observation = trial["perf"]
            observation["frames"] = dict(sorted(observation["frames"].items()))
            for frame in observation["frames"].values():
                frame["frame_ms"] = 16.6667
            observation["window_ms"] = 1000
            observation["frame_ms"]["p99"] = 16.6667
            observation["mean_step_ms"] = 10.0001
        self.assertEqual(perf.assess(result)["status"], "pass")

    def test_invalid_frame_containers_are_inconclusive(self):
        for frames in ([], None, "frames", {"10": {}}):
            result = fixture()
            result["workloads"]["sparse"][0]["perf"]["frames"] = frames
            self.assertEqual(perf.assess(result)["status"], "inconclusive")

    def test_invalid_baseline_is_rejected_before_live_work(self):
        with tempfile.TemporaryDirectory() as directory:
            for index, invalid in enumerate((None, [], {}, fixture())):
                if isinstance(invalid, dict) and "errors" in invalid:
                    invalid["errors"] = ["previous run failed"]
                baseline = Path(directory) / f"base-{index}.json"
                output = Path(directory) / f"result-{index}.json"
                perf.save(baseline, invalid)
                with patch.object(sys, "argv", ["perf", "run", "--baseline", str(baseline), "--output", str(output)]), \
                        patch.object(perf, "run", return_value=fixture()) as live, \
                        contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(perf.main(), 2)
                live.assert_not_called()
                self.assertEqual(perf.load(output)["assessment"]["status"], "inconclusive")

    def test_existing_logs_are_rejected_before_live_work(self):
        with tempfile.TemporaryDirectory() as directory:
            for index, suffix in enumerate((".build.log", ".app.log")):
                output = Path(directory) / f"result-{index}.json"
                log = Path(str(output) + suffix)
                log.write_text("preserve this log")
                with patch.object(sys, "argv", ["perf", "run", "--output", str(output)]), \
                        patch.object(perf, "run", return_value=fixture()) as live, \
                        contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(perf.main(), 2)
                live.assert_not_called()
                self.assertEqual(log.read_text(), "preserve this log")

    def test_repeated_and_baseline_population_and_hash_must_match(self):
        result = fixture()
        result["workloads"]["sparse"][1]["final_hash"] = "ffffffffffffffff"
        self.assertEqual(perf.assess(result)["status"], "inconclusive")
        result["workloads"]["sparse"] = [copy.deepcopy(result["workloads"]["sparse"][1]) for _ in range(3)]
        self.assertEqual(perf.assess(result, fixture())["status"], "inconclusive")

    def test_settings_workload_and_policy_changes_require_new_baseline(self):
        for field in ("conditions", "workload_digest", "suite"):
            result = fixture()
            result[field] = {} if field != "workload_digest" else "changed"
            self.assertEqual(perf.assess(result, fixture())["status"], "inconclusive")
        result = fixture()
        for trial in result["workloads"]["sparse"]:
            trial["perf"]["width"] = 1920
        self.assertEqual(perf.assess(result, fixture())["status"], "inconclusive")

    def test_missing_trials_and_workloads_fail_closed(self):
        result = fixture()
        result["workloads"]["sparse"].pop()
        self.assertEqual(perf.assess(result)["status"], "inconclusive")
        result["workloads"].clear()
        self.assertEqual(perf.assess(result)["status"], "inconclusive")

    def test_stored_verdict_is_not_trusted(self):
        result = fixture()
        result["assessment"] = {"status": "pass"}
        result["errors"] = ["startup failed"]
        self.assertEqual(perf.assess(result)["status"], "inconclusive")

    def test_absent_provenance_is_not_matching_provenance(self):
        for field in ("source", "conditions", "workload_digest", "binary_sha256"):
            result = fixture()
            del result[field]
            self.assertEqual(perf.assess(result, result)["status"], "inconclusive")

    def test_owned_logs_do_not_masquerade_as_source_edits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            def git(*args):
                subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)
            git("init", "-q")
            (root / "code.rs").write_text("original")
            git("add", "code.rs")
            git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture")
            log = root / "results" / "run.log"
            with patch.object(perf, "ROOT", root):
                before = perf.source_identity([log])
                self.assertFalse(before["dirty"])
                log.parent.mkdir()
                log.write_text("owned benchmark output")
                self.assertEqual(perf.source_identity([log]), before)
                (root / "code.rs").write_text("changed during measurement")
                self.assertNotEqual(perf.source_identity([log]), before)

    def test_baseline_files_are_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "baseline.json"
            perf.save(path, fixture())
            before = path.read_bytes()
            with self.assertRaises(FileExistsError):
                perf.save(path, {})
            self.assertEqual(path.read_bytes(), before)
            path.write_text('{"metric": NaN}')
            with self.assertRaises(ValueError):
                perf.load(path)

    def test_cli_reports_pass_regression_and_invalid_baseline_as_json(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            base = directory / "base.json"
            candidate = directory / "candidate.json"
            perf.save(base, fixture())
            slow = fixture()
            for trial in slow["workloads"]["sparse"]:
                set_step_ms(trial, 12)
            perf.save(candidate, slow)
            for label, baseline, current, code in [
                ("pass", base, base, 0), ("regression", base, candidate, 1),
                ("inconclusive", directory / "missing.json", candidate, 2),
            ]:
                output = directory / (label + ".json")
                completed = subprocess.run([sys.executable, "-B", str(Path(perf.__file__)),
                                            "compare", str(baseline), str(current), "--output", str(output)],
                                           capture_output=True, text=True)
                self.assertEqual(completed.returncode, code, completed.stderr)
                result = perf.load(output)
                self.assertEqual(result.get("status", result.get("assessment", {}).get("status")), label)


if __name__ == "__main__":
    unittest.main()
