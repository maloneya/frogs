"""Evidence-policy regressions; synthetic timings make these hardware independent."""
import argparse
import copy
import contextlib
import io
import json
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
        "window_ms": 1020, "current_tick": 180, "current_enemies": 100, "current_corpses": 0,
        "mean_step_ms": 10, "presentation_ms": {"mean": 1}, "frame_ms": {"p99": 17},
        "horde": {"instances": 100, "idle_instances": 100, "run_instances": 0, "corpse_instances": 0},
        "frames": {str(i): {"ticks": 1, "end_tick": 121 + i, "enemies": 100,
                            "presented": True, "focused": True, "occluded": False, "frame_ms": 17,
                            "game_step_ms": 10, "presentation_ms": 1}
                   for i in range(60)},
    }
    for asset in ("horde", "character"):
        observation.setdefault(asset, {}).update(active=True, vertices=456, indices=684,
                                                 part_count=1, material_count=1, image_count=1, texture_bytes=21844,
                                                 images={"0": {"width": 64, "height": 64}}, nodes=9, joints=7)
    trial = {"trial": {"warmup_ticks": 120, "sample_ticks": 60, "input": "idle",
                       "schedule_version": 1, "invalid": "", "initial_enemies": 100,
                       "start_enemies": 100, "population_sum": 6000},
             "final_hash": "0123456789abcdef", "finite": True, "perf": observation}
    return {"schema_version": perf.SCHEMA, "errors": [],
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
    def test_failed_build_preserves_rendered_cargo_diagnostics_without_raw_json(self):
        rendered = "error[E0308]: mismatched types\n  --> crates/core/src/instance.rs:172:4\n"
        messages = [
            {"reason": "compiler-message", "message": {"rendered": rendered,
                "spans": [{"irrelevant_payload": "x" * 10_000}]}},
            {"reason": "build-finished", "success": False},
        ]
        build = subprocess.CompletedProcess([], 101,
            stdout="\n".join(json.dumps(message) for message in messages),
            stderr="error: could not compile `arpg-core`\n")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "result.json"
            args = argparse.Namespace(mode="headless", output=output)
            result = fixture()
            with patch.object(perf.subprocess, "run", return_value=build), \
                 contextlib.redirect_stdout(io.StringIO()):
                perf.run(args, result)
            log = Path(str(output) + ".build.log").read_text()
            self.assertEqual(log, build.stderr + rendered)
            self.assertEqual(result["failure"]["kind"], "infrastructure")
            self.assertEqual(result["failure"]["stage"], "build")

    def test_workloads_cannot_depend_on_playable_scenes_even_through_symlinks(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(perf, "ROOT", Path(directory)):
            root = Path(directory)
            (root / "benchmarks").mkdir()
            (root / "scenes").mkdir()
            scene = root / "scenes/play.ron"
            scene.write_text("playable content")
            (root / "benchmarks/link.ron").symlink_to(scene)
            for name in ["scenes/play.ron", "benchmarks/../scenes/play.ron", "benchmarks/link.ron"]:
                with self.assertRaisesRegex(ValueError, "must be owned by benchmarks"):
                    perf.workload_scenes({"workloads": [{"scene": name}]})
            own = root / "benchmarks/test.ron"
            own.write_text("test-owned content")
            self.assertEqual(perf.workload_scenes({"workloads": [{"scene": "benchmarks/test.ron"}]}), [own.resolve()])

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

    def test_corpses_cannot_substitute_for_living_animation(self):
        result = fixture()
        for trial in result["workloads"]["sparse"]:
            trial["perf"]["current_corpses"] = 4
            trial["perf"]["horde"].update(instances=104, corpse_instances=4)
        self.assertEqual(perf.assess(result)["status"], "pass")
        for change in (dict(corpse_instances=3), dict(instances=103),
                       dict(idle_instances=0), dict(run_instances=1)):
            bad = copy.deepcopy(result)
            bad["workloads"]["sparse"][0]["perf"]["horde"].update(change)
            self.assertEqual(perf.assess(bad)["status"], "inconclusive")

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

    def test_retired_and_unrelated_evidence_is_filtered_before_reading_raw(self):
        with tempfile.TemporaryDirectory() as directory:
            store = Path(directory)
            current = fixture()
            manifest = {key: current[key] for key in
                        ("schema_version", "source", "conditions", "suite", "workload_digest")}
            manifest.update(artifact="missing.json.gz", sha256="a" * 64)
            retired = {**manifest, "schema_version": 1}
            perf.save(store / "retired.json", retired)
            unversioned = {k: v for k, v in retired.items() if k != "schema_version"}
            perf.save(store / "unversioned.json", unversioned)
            for key, value in (("build", "cargo build --release -p scenario --bin perf-headless"),
                               ("cpu", "different machine")):
                unrelated = copy.deepcopy(manifest)
                unrelated["conditions"][key] = value
                perf.save(store / (key + ".json"), unrelated)
            exclusions = []
            self.assertEqual(perf.discover(current, store, exclusions), (None, None))
            self.assertEqual(len(exclusions), 2)

    def test_bad_eligible_baseline_aborts_before_build_or_trials(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            current = fixture()
            current["source"]["revision"] = perf.command(["git", "rev-parse", "HEAD"])
            baseline = directory / "baseline.json"
            perf.save(baseline, current)
            store = directory / "store"
            artifact = perf.archive(baseline, store)
            artifact.write_bytes(b"corrupt archive")
            output = directory / "result.json"
            with patch.object(sys, "argv", ["perf", "run", "--output", str(output)]), \
                 patch.object(perf, "prepare", side_effect=lambda args, result: result.update(copy.deepcopy(current))), \
                 patch.object(perf, "EVIDENCE", store), patch.object(perf, "LOCAL_EVIDENCE", directory / "empty"), \
                 patch.object(perf, "run") as measure, \
                 contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(perf.main(), 2)
            measure.assert_not_called()
            saved = perf.load(output)
            self.assertEqual(saved["failure"]["kind"], "infrastructure")
            self.assertEqual(saved["failure"]["stage"], "baseline_preflight")

    def test_late_failures_keep_raw_trials_and_absolute_assessment(self):
        for stage in ("assessment", "archive"):
            with self.subTest(stage=stage), tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "result.json"
                current = fixture()
                original_assess = perf.assess
                calls = []
                def assess(result, baseline=None):
                    if stage == "assessment" and not calls:
                        calls.append("failed")
                        raise ValueError("comparison infrastructure failed")
                    return original_assess(result, baseline)
                def prepare(args, result):
                    result.update(copy.deepcopy(current))
                    result["workloads"] = {}
                def measure(args, result):
                    result["workloads"] = copy.deepcopy(current["workloads"])
                with patch.object(sys, "argv", ["perf", "run", "--output", str(output)]), \
                     patch.object(perf, "prepare", side_effect=prepare), \
                     patch.object(perf, "discover", return_value=(None, None)), \
                     patch.object(perf, "run", side_effect=measure), \
                     patch.object(perf, "assess", side_effect=assess), \
                     patch.object(perf, "archive_result", side_effect=OSError("store unavailable")), \
                     contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(perf.main(), 2)
                saved = perf.load(output)
                self.assertEqual(saved["workloads"], current["workloads"])
                self.assertEqual(saved["assessment"]["absolute_status"], "pass")
                self.assertEqual(saved["assessment"]["status"], "inconclusive")
                self.assertEqual(saved["failure"]["stage"], stage)
                # The saved raw samples can still be compared without measuring again.
                self.assertEqual(original_assess(saved, current)["status"], "pass")

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

    def test_noisy_metric_keeps_other_metrics_and_workloads(self):
        result = fixture()
        other = copy.deepcopy(result["suite"]["workloads"][0])
        other["name"] = "other"
        result["suite"]["workloads"].append(other)
        result["workloads"]["other"] = copy.deepcopy(result["workloads"]["sparse"])
        set_step_ms(result["workloads"]["sparse"][1], 20)
        verdict = perf.assess(result)
        rows = {(r["workload"], r["metric"]): r for r in verdict["metrics"]}
        self.assertEqual(rows["sparse", "mean_step_ms"]["status"], "inconclusive")
        self.assertEqual(rows["sparse", "frame_ms.p99"]["absolute"], "pass")
        self.assertEqual(rows["other", "mean_step_ms"]["absolute"], "pass")
        self.assertEqual(verdict["rerun_workloads"], ["sparse"])
        result["workloads"]["sparse"][0]["finite"] = False
        verdict = perf.assess(result)
        self.assertEqual(verdict["absolute_status"], "inconclusive")
        self.assertTrue(any(r["workload"] == "other" for r in verdict["metrics"]))

    def test_changed_gameplay_preserves_absolute_evidence(self):
        result = fixture()
        for trial in result["workloads"]["sparse"]:
            trial["final_hash"] = "f" * 16
        verdict = perf.assess(result, fixture())
        self.assertEqual(verdict["absolute_status"], "pass")
        self.assertEqual(verdict["relative_status"], "inconclusive")
        self.assertTrue(all(r["relative"] == "incompatible" for r in verdict["metrics"]))

    def test_focused_workload_reuses_full_baseline(self):
        base = fixture()
        workload = copy.deepcopy(base["suite"]["workloads"][0])
        workload["name"] = "other"
        base["suite"]["workloads"].append(workload)
        base["workloads"]["other"] = copy.deepcopy(base["workloads"]["sparse"])
        self.assertEqual(perf.assess(fixture(), base)["status"], "pass")

    def test_dirty_head_is_not_evidence_of_ancestor_source(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            result = fixture()
            result["source"].update(revision=perf.command(["git", "rev-parse", "HEAD"]), dirty=True)
            path = directory / "input.json"
            perf.save(path, result)
            perf.archive(path, directory / "store")
            changed = copy.deepcopy(result)
            changed["source"]["digest"] = "e" * 64
            self.assertEqual(perf.discover(changed, directory / "store"), (None, None))
            self.assertIsNotNone(perf.discover(result, directory / "store")[0])

    def test_headless_requires_complete_raw_ticks_and_matching_build(self):
        result = fixture()
        result["conditions"]["build"] = "cargo build --release -p scenario --bin perf-headless"
        trial = {"schedule_version": 1, "warmup_ticks": 120, "sample_ticks": 60,
                 "input": "idle", "debug_build": False, "finite": True, "current_tick": 180,
                 "initial_enemies": 100, "start_enemies": 100, "end_enemies": 100,
                 "population_sum": 6000, "final_hash": "a" * 16,
                 "step_ns": {str(i): 1000000 for i in range(60)}}
        result["workloads"]["sparse"] = [copy.deepcopy(trial) for _ in range(3)]
        self.assertEqual(perf.assess(result, result)["status"], "pass")
        del result["workloads"]["sparse"][0]["step_ns"]["0"]
        self.assertEqual(perf.assess(result)["status"], "inconclusive")

    def test_archive_is_immutable_integrity_checked_and_reuses_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            result = fixture()
            result["source"]["revision"] = perf.command(["git", "rev-parse", "HEAD"])
            path = directory / "input.json"
            perf.save(path, result)
            store = directory / "store"
            artifact = perf.archive(path, store)
            with self.assertRaises(FileExistsError):
                perf.archive(path, store)
            discovered, manifest = perf.discover(result, store)
            self.assertEqual(discovered, result)
            self.assertTrue(manifest)
            different = copy.deepcopy(result)
            different["conditions"]["cpu"] = "another machine"
            self.assertEqual(perf.discover(different, store), (None, None))
            artifact.write_bytes(b"corrupt")
            with self.assertRaises(OSError):
                perf.discover(result, store)

    def test_discovery_prefers_nearest_ancestor_then_newest_measurement(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, text=True).strip()
            git("init", "-q")
            revisions = []
            for index in range(3):
                (root / "source").write_text(str(index))
                git("add", "source")
                git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", str(index))
                revisions.append(git("rev-parse", "HEAD"))
            store = root / "evidence"
            with patch.object(perf, "ROOT", root):
                for index, (revision, date) in enumerate([
                    (revisions[0], "2026-09-30"),
                    (revisions[1], "2026-09-20"),
                    (revisions[1], "2026-09-21"),
                ]):
                    result = fixture()
                    result["source"]["revision"] = revision
                    result["created_utc"] = date
                    path = root / f"result-{index}.json"
                    perf.save(path, result)
                    perf.archive(path, store)
                current = fixture()
                current["source"]["revision"] = revisions[2]
                selected, _ = perf.discover(current, store)
                self.assertEqual(selected["source"]["revision"], revisions[1])
                self.assertEqual(selected["created_utc"], "2026-09-21")

    def test_archive_preserves_measured_source_identity_and_discovery(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            def git(*args):
                subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)
            git("init", "-q")
            (root / ".gitignore").write_text("/target\n")
            (root / "code.rs").write_text("original")
            git("add", ".")
            git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture")
            (root / "code.rs").write_text("dirty source measured")
            evidence = root / "benchmarks/evidence"
            local = root / "target/perf/evidence"
            with patch.object(perf, "ROOT", root), patch.object(perf, "EVIDENCE", evidence), \
                 patch.object(perf, "LOCAL_EVIDENCE", local):
                result = fixture()
                result["source"] = before = perf.source_identity()
                path = root / "target/result.json"
                perf.save(path, result)
                perf.archive(path)
                self.assertEqual(perf.source_identity(), before)
                self.assertEqual(perf.discover(result)[0], result)
                perf.archive(path, local)
                self.assertEqual(perf.source_identity(), before)
                self.assertEqual(perf.discover(result)[0], result)
                (root / "code.rs").write_text("different source")
                self.assertNotEqual(perf.source_identity(), before)

    def test_run_automatically_saves_and_reuses_local_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            source = fixture()
            source["source"]["revision"] = perf.command(["git", "rev-parse", "HEAD"])
            with patch.object(perf, "EVIDENCE", directory / "published"), \
                 patch.object(perf, "LOCAL_EVIDENCE", directory / "local"), \
                 patch.object(perf, "prepare", side_effect=lambda args, result: result.update(copy.deepcopy(source))), \
                 patch.object(perf, "run"), \
                 contextlib.redirect_stdout(io.StringIO()):
                outputs = []
                for index in range(2):
                    output = directory / f"run-{index}.json"
                    with patch.object(sys, "argv", ["perf", "run", "--output", str(output)]):
                        self.assertEqual(perf.main(), 0)
                    outputs.append(perf.load(output))
                self.assertEqual(outputs[0]["assessment"]["relative_status"], "missing")
                self.assertEqual(outputs[1]["assessment"]["relative_status"], "pass")
                self.assertTrue(outputs[1]["baseline"])
                self.assertEqual(len(list((directory / "local").glob("*.json.gz"))), 2)
                self.assertFalse((directory / "published").exists())

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
