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
        "schema_version": 2, "debug_build": False, "collision_debug": False,
        "vsync": True, "width": 1280, "height": 720, "scale_factor": 1,
        "view_height": 32,
        "refresh_millihertz": 60000, "gpu": {"name": "test", "timing_requested": False, "timing_available": False, "desired_maximum_frame_latency": 2},
        "gpu_timing_available": False, "gpu_samples": 0, "gpu_partial": 0, "gpu_failed": 0, "gpu_pending": 0, "gpu_unsampled": 60,
        "surface_skipped": 0, "unfocused_frames": 0, "occluded_frames": 0,
        "overwritten": 0, "samples": 60, "presented": 60, "ticks": 60,
        "window_ms": 1020, "current_tick": 180, "current_enemies": 100, "current_corpses": 0,
        "mean_step_ms": 10, "presentation_ms": {"mean": 1}, "frame_ms": {"p99": 17},
        "horde": {"instances": 100, "idle_instances": 100, "run_instances": 0, "attack_instances": 0, "recoiling_instances": 0, "corpse_instances": 0},
        "frames": {str(i): {"ticks": 1, "end_tick": 121 + i, "enemies": 100,
                            "presented": True, "focused": True, "occluded": False, "frame_ms": 17,
                            "game_step_ms": 10, "presentation_ms": 1, "render_cpu_ms": 2,
                            "acquire_ms": 1, "upload_ms": 0.5, "encode_ms": 0.25,
                            "submit_ms": 0.2, "present_ms": 0.05, "capture_ms": 0,
                            "gpu_poll_ms": 0, "frame_id": i + 1, "gpu_status": "not_sampled"}
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
            "asset_digests": {path: "e" * 64 for path in perf.CHARACTER_ASSETS.values()},
            "conditions": {"machine": "test", "cpu": "test", "memory_bytes": "1024", "os": "test",
                           "rustc": "test", "build": "cargo build --release -p arpg", "build_environment": {}},
            "suite": {"version": 1, "repetitions": 3, "relative_tolerance": 0.1,
                      "max_trial_spread": 0.15, "workloads": [workload]},
            "workloads": {"sparse": [copy.deepcopy(trial) for _ in range(3)]}}


def set_step_ms(trial, milliseconds):
    trial["perf"]["mean_step_ms"] = milliseconds
    for frame in trial["perf"]["frames"].values():
        frame["game_step_ms"] = milliseconds * frame["ticks"]


def set_frame_ms(trial, milliseconds):
    trial["perf"]["frame_ms"]["p99"] = milliseconds
    frames = trial["perf"]["frames"]
    trial["perf"]["window_ms"] = milliseconds * len(frames)
    for frame in frames.values():
        frame["frame_ms"] = milliseconds


@contextlib.contextmanager
def accepted_store(directory, results):
    directory = Path(directory)
    store = directory / "published"
    identities = [perf.archive_result(result, store).name.removesuffix(".json.gz") for result in results]
    policy = directory / "accepted-ceiling.json"
    perf.save(policy, {"headless": [], "live": identities})
    with patch.object(perf, "EVIDENCE", store), patch.object(perf, "ACCEPTED_REFERENCES", policy):
        yield policy


class EvidenceTests(unittest.TestCase):
    def test_surface_latency_is_mandatory_integer_gpu_context(self):
        for latency in (1, 2):
            with self.subTest(latency=latency):
                result = fixture()
                for trial in result["workloads"]["sparse"]:
                    trial["perf"]["gpu"]["desired_maximum_frame_latency"] = latency
                perf.validate_result(result)
        for latency in (None, False, True, 0, -1, 3, 1.0, 1.5, "1", [], {}):
            with self.subTest(invalid_latency=latency):
                result = fixture()
                result["workloads"]["sparse"][0]["perf"]["gpu"]["desired_maximum_frame_latency"] = latency
                with self.assertRaisesRegex(ValueError, "desired_maximum_frame_latency"):
                    perf.validate_result(result)
        missing = fixture()
        del missing["workloads"]["sparse"][0]["perf"]["gpu"]["desired_maximum_frame_latency"]
        with self.assertRaisesRegex(ValueError, "desired_maximum_frame_latency"):
            perf.validate_result(missing)

    def test_different_surface_latencies_preserve_absolute_but_refuse_relative_comparison(self):
        baseline = fixture()
        candidate = copy.deepcopy(baseline)
        for trial in candidate["workloads"]["sparse"]:
            trial["perf"]["gpu"]["desired_maximum_frame_latency"] = 1
        self.assertNotEqual(perf.environment(candidate["workloads"]["sparse"][0]["perf"]),
                            perf.environment(baseline["workloads"]["sparse"][0]["perf"]))
        verdict = perf.assess(candidate, perf.references(baseline))
        self.assertEqual(verdict["absolute_status"], "pass")
        self.assertEqual(verdict["relative_status"], "inconclusive")
        self.assertTrue(all(row["relative"] == "incompatible" for row in verdict["metrics"]))

    def test_prior_schema_without_surface_latency_is_rejected(self):
        retired = fixture()
        retired["schema_version"] = 5
        for trial in retired["workloads"]["sparse"]:
            del trial["perf"]["gpu"]["desired_maximum_frame_latency"]
        with self.assertRaisesRegex(ValueError, "unsupported schema"):
            perf.validate_result(retired)

    def test_gpu_mode_and_raw_coverage_are_part_of_validated_evidence(self):
        ordinary = fixture()
        observed = copy.deepcopy(ordinary)
        for trial in observed["workloads"]["sparse"]:
            p = trial["perf"]
            p["gpu"].update(timing_requested=True, timing_available=True)
            p.update(gpu_timing_available=True, gpu_samples=60, gpu_unsampled=0)
            for frame in p["frames"].values():
                frame.update(gpu_status="ready", gpu_unavailable_passes=0, gpu_span_ms=2, gpu_main_ms=1, gpu_directional_shadow_ms=0.5)
            for name, value, count in (("gpu_span_ms", 2, 60), ("gpu_main_ms", 1, 60),
                                        ("gpu_directional_shadow_ms", .5, 60), ("gpu_fire_shadow_ms", 0, 0)):
                p[name] = dict(mean=value, p50=value, p95=value, p99=value, max=value, samples=count)
        perf.validate_result(observed)
        self.assertNotEqual(perf.environment(ordinary["workloads"]["sparse"][0]["perf"]),
                            perf.environment(observed["workloads"]["sparse"][0]["perf"]))
        partial = copy.deepcopy(observed)
        p = partial["workloads"]["sparse"][0]["perf"]
        p.update(gpu_samples=59, gpu_partial=1)
        frame = p["frames"]["0"]
        frame.update(gpu_status="partial", gpu_unavailable_passes=1)
        del frame["gpu_directional_shadow_ms"], frame["gpu_span_ms"]
        p["gpu_directional_shadow_ms"]["samples"] = 59
        p["gpu_span_ms"]["samples"] = 59
        perf.validate_result(partial)
        frame["gpu_directional_shadow_ms"] = 0
        with self.assertRaisesRegex(ValueError, "unavailable GPU pass"):
            perf.validate_result(partial)
        for mutate in [lambda p: p.update(gpu_samples=59),
                       lambda p: p["frames"]["1"].update(frame_id=1),
                       lambda p: p["frames"]["0"].update(gpu_status="pending"),
                       lambda p: p["frames"]["0"].update(gpu_main_ms=3),
                       lambda p: p["frames"]["0"].update(acquire_ms=100),
                       lambda p: p["gpu_main_ms"].update(mean=2)]:
            bad = copy.deepcopy(observed)
            mutate(bad["workloads"]["sparse"][0]["perf"])
            with self.assertRaises(ValueError):
                perf.validate_result(bad)

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
        self.assertEqual(perf.assess(base, perf.references(base))["status"], "pass")
        slow = copy.deepcopy(base)
        for trial in slow["workloads"]["sparse"]:
            set_step_ms(trial, 12)
        result = perf.assess(slow, perf.references(base))
        self.assertEqual(result["status"], "regression")
        self.assertEqual(result["metrics"][0]["absolute"], "pass")
        self.assertEqual(result["metrics"][0]["relative"], "regression")

    def test_corpses_cannot_substitute_for_living_animation(self):
        result = fixture()
        for trial in result["workloads"]["sparse"]:
            trial["perf"]["current_corpses"] = 4
            trial["perf"]["horde"].update(instances=104, corpse_instances=4)
        self.assertEqual(perf.assess(result, perf.references(result))["status"], "pass")
        for change in (dict(corpse_instances=3), dict(instances=103),
                       dict(idle_instances=0), dict(run_instances=1)):
            bad = copy.deepcopy(result)
            bad["workloads"]["sparse"][0]["perf"]["horde"].update(change)
            self.assertEqual(perf.assess(bad)["status"], "inconclusive")

    def test_absolute_budget_needs_no_baseline(self):
        result = fixture()
        self.assertEqual(perf.assess(result)["absolute_status"], "pass")
        self.assertEqual(perf.assess(result)["status"], "inconclusive")
        for trial in result["workloads"]["sparse"]:
            trial["perf"]["frame_ms"]["p99"] = 30
            trial["perf"]["window_ms"] = 1800
            for frame in trial["perf"]["frames"].values():
                frame["frame_ms"] = 30
        self.assertEqual(perf.assess(result)["absolute_status"], "regression")
        self.assertTrue(perf.assess(result)["confirmed_regressions"])

    def test_a_best_sample_cannot_hide_unstable_repetitions(self):
        result = fixture()
        set_step_ms(result["workloads"]["sparse"][1], 20)
        self.assertEqual(perf.assess(result, perf.references(fixture()))["status"], "inconclusive")

    def test_threshold_overlap_is_not_a_pass(self):
        result = fixture()
        for trial, duration in zip(result["workloads"]["sparse"], [10.9, 11.1, 11.2]):
            set_step_ms(trial, duration)
        self.assertEqual(perf.assess(result, perf.references(fixture()))["status"], "inconclusive")

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
        self.assertEqual(perf.assess(result, perf.references(result))["status"], "pass")

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
                def measure(args, result, baseline=None):
                    result["workloads"] = copy.deepcopy(current["workloads"])
                with patch.object(sys, "argv", ["perf", "run", "--output", str(output)]), \
                     patch.object(perf, "prepare", side_effect=prepare), \
                     patch.object(perf, "accepted_references", return_value=perf.references(current)), \
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
                self.assertEqual(original_assess(saved, perf.references(current))["status"], "pass")

    def test_repeated_and_baseline_population_and_hash_must_match(self):
        result = fixture()
        result["workloads"]["sparse"][1]["final_hash"] = "ffffffffffffffff"
        self.assertEqual(perf.assess(result)["status"], "inconclusive")
        result["workloads"]["sparse"] = [copy.deepcopy(result["workloads"]["sparse"][1]) for _ in range(3)]
        self.assertEqual(perf.assess(result, perf.references(fixture()))["status"], "inconclusive")

    def test_settings_workload_and_policy_changes_require_new_baseline(self):
        for field in ("conditions", "workload_digest", "suite"):
            result = fixture()
            result[field] = {} if field != "workload_digest" else "changed"
            self.assertEqual(perf.assess(result, perf.references(fixture()))["status"], "inconclusive")
        result = fixture()
        for trial in result["workloads"]["sparse"]:
            trial["perf"]["width"] = 1920
        self.assertEqual(perf.assess(result, perf.references(fixture()))["status"], "inconclusive")

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
            verdict = perf.assess(result, perf.references(fixture()))
            self.assertEqual(verdict["status"], "inconclusive")
            self.assertEqual(verdict["relative_status"], "inconclusive")

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
        baselines = perf.references(copy.deepcopy(result))
        set_step_ms(result["workloads"]["sparse"][1], 20)
        verdict = perf.assess(result, baselines)
        rows = {(r["workload"], r["metric"]): r for r in verdict["metrics"]}
        self.assertEqual(rows["sparse", "mean_step_ms"]["status"], "inconclusive")
        self.assertEqual(rows["sparse", "frame_ms.p99"]["absolute"], "pass")
        self.assertEqual(rows["other", "mean_step_ms"]["absolute"], "pass")
        self.assertEqual(verdict["rerun_workloads"], ["sparse"])
        result["workloads"]["sparse"][0]["finite"] = False
        verdict = perf.assess(result)
        self.assertEqual(verdict["absolute_status"], "inconclusive")
        self.assertTrue(any(r["workload"] == "other" for r in verdict["metrics"]))

    def test_confirmed_failure_survives_another_metrics_uncertainty(self):
        result = fixture()
        for trial in result["workloads"]["sparse"]:
            set_frame_ms(trial, 30)
        set_step_ms(result["workloads"]["sparse"][1], 20)
        verdict = perf.assess(result)
        self.assertEqual(verdict["status"], "inconclusive")
        self.assertTrue(verdict["review_required"])
        self.assertEqual([m["metric"] for m in verdict["confirmed_regressions"]], ["frame_ms.p99"])

    def test_feature_asset_comparison_requires_exact_declared_changes_and_same_conditions(self):
        baseline, candidate = fixture(), fixture()
        path = perf.CHARACTER_ASSETS["horde"]
        candidate["asset_digests"][path] = "f" * 64
        for trial in candidate["workloads"]["sparse"]:
            trial["perf"]["horde"]["vertices"] = 900
        self.assertEqual(perf.assess(candidate, perf.references(baseline))["relative_status"], "inconclusive")
        self.assertFalse(perf.compatible(candidate, baseline, candidate["suite"]["workloads"]))
        self.assertEqual(perf.assess(candidate, perf.references(baseline), [path])["status"], "pass")
        with self.assertRaises(ValueError):
            perf.asset_differences(candidate, baseline, [path, perf.CHARACTER_ASSETS["character"]])
        for trial in candidate["workloads"]["sparse"]:
            trial["perf"]["width"] = 1920
        self.assertEqual(perf.assess(candidate, perf.references(baseline), [path])["relative_status"], "inconclusive")

    def test_changed_assets_do_not_erase_absolute_failure(self):
        baseline, candidate = fixture(), fixture()
        candidate["asset_digests"][perf.CHARACTER_ASSETS["horde"]] = "f" * 64
        for trial in candidate["workloads"]["sparse"]:
            set_frame_ms(trial, 30)
        verdict = perf.assess(candidate, perf.references(baseline))
        self.assertEqual(verdict["relative_status"], "inconclusive")
        self.assertEqual(verdict["confirmed_regressions"][0]["absolute"], "regression")

    def test_feature_comparison_cli_records_provenance_and_cannot_enter_baseline_store(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            baseline, candidate = fixture(), fixture()
            asset = perf.CHARACTER_ASSETS["horde"]
            candidate["asset_digests"][asset] = "f" * 64
            for trial in candidate["workloads"]["sparse"]:
                trial["perf"]["horde"]["vertices"] = 900
            base, current, output = [directory / name for name in ["base.json", "current.json", "comparison.json"]]
            perf.save(base, baseline)
            perf.save(current, candidate)
            with patch.object(sys, "argv", ["perf", "compare", str(base), str(current), "--asset-change", asset, "--output", str(output)]), \
                 contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(perf.main(), 0)
            report = perf.load(output)
            self.assertEqual(report["comparison_kind"], "feature_assets")
            self.assertFalse(report["baseline_eligible"])
            self.assertEqual(report["asset_changes"][asset], {"before": "e" * 64, "after": "f" * 64})
            self.assertEqual(report["candidate_source"], candidate["source"])
            with self.assertRaises((KeyError, ValueError)):
                perf.archive(output, directory / "store")

    def test_undeclared_asset_changes_preserve_raw_absolute_observations(self):
        baseline, candidate = fixture(), fixture()
        candidate["asset_digests"][perf.CHARACTER_ASSETS["character"]] = "f" * 64
        verdict = perf.assess(candidate, perf.references(baseline), [perf.CHARACTER_ASSETS["horde"]])
        self.assertEqual(verdict["absolute_status"], "pass")
        self.assertEqual(verdict["relative_status"], "inconclusive")

    def test_live_readiness_observes_three_valid_frames_and_activates_once(self):
        observation = copy.deepcopy(fixture()["workloads"]["sparse"][0]["perf"])
        observation["readiness"] = dict(focused=True, occluded=False, surface=True)
        observation["frames"]["0"]["focused"] = False
        with patch.object(perf, "request", side_effect=["ok", json.dumps(observation)]) as request:
            perf.live_ready("socket", activate=True)
        self.assertEqual(request.call_args_list[0].args, ("socket", "perf prepare"))
        observation["readiness"]["focused"] = False
        with patch.object(perf, "request", return_value=json.dumps(observation)) as request:
            with self.assertRaisesRegex(perf.MeasurementStopped, "not ready"):
                perf.live_ready("socket", timeout=0)
        self.assertEqual(request.call_count, 1)
        observation["readiness"]["focused"] = True
        observation["frames"]["59"]["focused"] = False
        with patch.object(perf, "request", return_value=json.dumps(observation)):
            with self.assertRaises(perf.MeasurementStopped):
                perf.live_ready("socket", timeout=0)

    def test_live_collection_stops_after_invalid_trial_and_retains_it(self):
        result = fixture()
        trial = result["workloads"]["sparse"][0]
        trial["perf"]["unfocused_frames"] = 1
        result["workloads"] = {}
        with patch.object(perf, "live_ready"), \
             patch.object(perf, "request", side_effect=["vsync true", "ok", "ready tick=0", json.dumps(trial)]) as request, \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(perf.MeasurementStopped, "unfocused_frames"):
                perf.collect_live("socket", argparse.Namespace(vsync="on", gpu_timing=False), result, None)
        self.assertEqual(len(result["workloads"]["sparse"]), 1)
        self.assertEqual(request.call_count, 4)

    def test_live_collection_retains_regression_and_measures_remaining_workloads(self):
        result = fixture()
        workload = copy.deepcopy(result["suite"]["workloads"][0])
        workload["name"] = "next-workload"
        result["suite"]["workloads"].append(workload)
        trials = result["workloads"]["sparse"]
        for trial in trials:
            set_frame_ms(trial, 30)
        result["workloads"] = {}
        with patch.object(perf, "live_ready"), \
             patch.object(perf, "request", side_effect=["vsync true", "ok", "ready tick=0", *map(json.dumps, trials),
                                                       "ready tick=0", *map(json.dumps, trials)]), \
             contextlib.redirect_stdout(io.StringIO()):
            perf.collect_live("socket", argparse.Namespace(vsync="on", gpu_timing=False), result, None)
        self.assertEqual(len(result["workloads"]["next-workload"]), len(trials))
        verdict = perf.assess(result)
        self.assertEqual(verdict["absolute_status"], "regression")
        self.assertTrue(verdict["confirmed_regressions"])

    def test_changed_gameplay_preserves_absolute_evidence(self):
        result = fixture()
        for trial in result["workloads"]["sparse"]:
            trial["final_hash"] = "f" * 16
        verdict = perf.assess(result, perf.references(fixture()))
        self.assertEqual(verdict["absolute_status"], "pass")
        self.assertEqual(verdict["relative_status"], "inconclusive")
        self.assertTrue(all(r["relative"] == "incompatible" for r in verdict["metrics"]))

    def test_focused_workload_reuses_full_baseline(self):
        base = fixture()
        workload = copy.deepcopy(base["suite"]["workloads"][0])
        workload["name"] = "other"
        base["suite"]["workloads"].append(workload)
        base["workloads"]["other"] = copy.deepcopy(base["workloads"]["sparse"])
        self.assertEqual(perf.assess(fixture(), perf.references(base))["status"], "pass")

    def test_headless_requires_complete_raw_ticks_and_matching_build(self):
        result = fixture()
        result["conditions"]["build"] = "cargo build --release -p scenario --bin perf-headless"
        trial = {"schedule_version": 1, "warmup_ticks": 120, "sample_ticks": 60,
                 "input": "idle", "debug_build": False, "finite": True, "current_tick": 180,
                 "initial_enemies": 100, "start_enemies": 100, "end_enemies": 100,
                 "population_sum": 6000, "final_hash": "a" * 16,
                 "step_ns": {str(i): 1000000 for i in range(60)}}
        result["workloads"]["sparse"] = [copy.deepcopy(trial) for _ in range(3)]
        self.assertEqual(perf.assess(result, perf.references(result))["status"], "pass")
        del result["workloads"]["sparse"][0]["step_ns"]["0"]
        self.assertEqual(perf.assess(result)["status"], "inconclusive")

    def test_accepted_ceiling_covers_real_suite_and_preserves_dirty_uncertainty(self):
        names = {work["name"] for work in perf.load(perf.SUITE)["workloads"]}
        for mode in ("live", "headless"):
            baselines = perf.accepted_references(mode)
            self.assertEqual(set(baselines), names)
            self.assertTrue(all(ref.result["source"]["dirty"] for ref in baselines.values()))
        # Reading immutable historical evidence is not a benchmark run.
        baselines = perf.accepted_references("live")
        for name in ("compressed", "night_fire"):
            ref = baselines[name].result
            verdict = perf.assess(ref, baselines)
            self.assertEqual(verdict["relative_status"], "inconclusive")
            self.assertTrue(any(row["relative"] == "inconclusive" for row in verdict["metrics"]))

    def test_separate_workload_references_keep_their_own_sources_and_timings(self):
        candidate = fixture()
        other = copy.deepcopy(candidate["suite"]["workloads"][0])
        other["name"] = "other"
        candidate["suite"]["workloads"].append(other)
        candidate["workloads"]["other"] = copy.deepcopy(candidate["workloads"]["sparse"])
        sparse = perf.selected(copy.deepcopy(candidate), candidate["suite"]["workloads"][0])
        sparse["source"].update(dirty=True, digest="1" * 64)
        other_result = perf.selected(copy.deepcopy(candidate), other)
        other_result["source"].update(dirty=True, digest="2" * 64)
        other_result["binary_sha256"] = "3" * 64
        with tempfile.TemporaryDirectory() as directory, accepted_store(directory, [sparse, other_result]):
            baselines = perf.accepted_references("live")
            self.assertEqual(perf.assess(candidate, baselines)["status"], "pass")
            self.assertEqual(baselines["sparse"].provenance()["source"], sparse["source"])
            self.assertEqual(baselines["other"].provenance()["binary_sha256"], "3" * 64)
            self.assertNotEqual(baselines["sparse"].sha256, baselines["other"].sha256)
            del baselines["other"]
            verdict = perf.assess(candidate, baselines)
            self.assertEqual(verdict["status"], "inconclusive")
            self.assertEqual(verdict["absolute_status"], "pass")
            self.assertIn("other: missing relative reference", verdict["reasons"])

    def test_default_run_keeps_pinned_reference_despite_newer_local_measurements(self):
        baseline = fixture()
        for trial in baseline["workloads"]["sparse"]:
            set_step_ms(trial, 1)
        baseline["source"].update(dirty=True, digest="1" * 64)
        candidate = copy.deepcopy(baseline)
        candidate["source"]["digest"] = "2" * 64
        for trial in candidate["workloads"]["sparse"]:
            set_step_ms(trial, 2)
        with tempfile.TemporaryDirectory() as directory, accepted_store(directory, [baseline]):
            directory = Path(directory)
            with patch.object(perf, "LOCAL_EVIDENCE", directory / "local"), \
                 patch.object(perf, "prepare", side_effect=lambda args, result: result.update(copy.deepcopy(candidate))), \
                 patch.object(perf, "run"), contextlib.redirect_stdout(io.StringIO()):
                for index in range(2):
                    output = directory / f"run-{index}.json"
                    with patch.object(sys, "argv", ["perf", "run", "--output", str(output)]):
                        self.assertEqual(perf.main(), 1)
                    saved = perf.load(output)
                    self.assertEqual(saved["assessment"]["absolute_status"], "pass")
                    self.assertEqual(saved["assessment"]["relative_status"], "regression")
                    self.assertEqual(saved["baselines"]["sparse"]["source"], baseline["source"])
                self.assertEqual(len(list((directory / "local").glob("*.json.gz"))), 2)

    def test_missing_corrupt_and_redirected_pins_stop_before_measurement(self):
        for defect in ("missing", "corrupt", "redirected"):
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as directory, \
                 accepted_store(directory, [fixture()]):
                store = perf.EVIDENCE
                artifact = next(store.glob("*.json.gz"))
                if defect == "missing":
                    artifact.unlink()
                elif defect == "corrupt":
                    artifact.write_bytes(b"not gzip")
                else:
                    manifest = artifact.with_suffix("")
                    value = perf.load(manifest)
                    value["sha256"] = "f" * 64
                    value["artifact"] = "f" * 64 + ".json.gz"
                    manifest.write_text(json.dumps(value))
                output = Path(directory) / "candidate.json"
                with patch.object(sys, "argv", ["perf", "run", "--output", str(output)]), \
                     patch.object(perf, "prepare"), patch.object(perf, "run") as measure, \
                     contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(perf.main(), 2)
                measure.assert_not_called()
                self.assertEqual(perf.load(output)["failure"]["stage"], "baseline_preflight")

    def test_rerun_cannot_erase_a_confirmed_stop_by_switching_references(self):
        original, candidate = fixture(), fixture()
        for trial in original["workloads"]["sparse"]:
            set_step_ms(trial, 1)
        for trial in candidate["workloads"]["sparse"]:
            set_step_ms(trial, 2)
        candidate["assessment"] = perf.assess(candidate, perf.references(original))
        self.assertTrue(candidate["assessment"]["confirmed_regressions"])
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            previous, replacement, output = [directory / name for name in ("previous.json", "replacement.json", "result.json")]
            perf.save(previous, candidate)
            perf.save(replacement, candidate)
            with patch.object(sys, "argv", ["perf", "run", "--rerun", str(previous), "--baseline", str(replacement), "--output", str(output)]), \
                 patch.object(perf, "run") as measure, \
                 contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(perf.main(), 2)
            measure.assert_not_called()
            self.assertIn("confirmed failure", perf.load(output)["failure"]["reason"])

    def test_malformed_stored_assessments_fail_admission_without_crashing(self):
        for malformed in (None, [], "pass", 0):
            candidate = fixture()
            candidate["assessment"] = malformed
            self.assertFalse(perf.accepted_pass(candidate, "live"))
            self.assertEqual(perf.confirmed_regressions(malformed), [])

    def test_missing_workload_reference_stops_before_build(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "candidate.json"
            with patch.object(sys, "argv", ["perf", "run", "--output", str(output)]), \
                 patch.object(perf, "accepted_references", return_value={}), \
                 patch.object(perf, "prepare", side_effect=lambda args, result: result.update(fixture())), \
                 patch.object(perf, "run") as measure, \
                 contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(perf.main(), 2)
            measure.assert_not_called()
            self.assertIn("missing relative reference", perf.load(output)["failure"]["reason"])

    def test_noisy_reference_cannot_pass_stable_candidate(self):
        baseline, candidate = fixture(), fixture()
        for trial, duration in zip(baseline["workloads"]["sparse"], (8, 10, 12)):
            set_step_ms(trial, duration)
        verdict = perf.assess(candidate, perf.references(baseline))
        self.assertEqual(verdict["absolute_status"], "pass")
        self.assertEqual(verdict["relative_status"], "inconclusive")
        self.assertEqual(verdict["status"], "inconclusive")

    def test_complete_accepted_pass_revalidates_raw_and_reference_provenance(self):
        candidate = fixture()
        with tempfile.TemporaryDirectory() as directory, accepted_store(directory, [fixture()]):
            suite_path = Path(directory) / "suite.json"
            perf.save(suite_path, candidate["suite"])
            baselines = perf.accepted_references("live")
            candidate["baselines"] = {name: ref.provenance() for name, ref in baselines.items()}
            candidate["assessment"] = perf.assess(candidate, baselines)
            with patch.object(perf, "SUITE", suite_path):
                self.assertTrue(perf.accepted_pass(candidate, "live"))
                candidate["baselines"]["sparse"]["sha256"] = "f" * 64
                self.assertFalse(perf.accepted_pass(candidate, "live"))
                candidate["baselines"] = {name: ref.provenance() for name, ref in baselines.items()}
                for trial in candidate["workloads"]["sparse"]:
                    set_step_ms(trial, 12)
                self.assertFalse(perf.accepted_pass(candidate, "live"))
                candidate["assessment"]["scope"] = "absolute_only"
                self.assertFalse(perf.accepted_pass(candidate, "live"))

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
            restored = perf.read_archive(artifact.with_suffix(""))
            self.assertEqual(restored["sparse"].result, result)
            artifact.write_bytes(b"corrupt")
            with self.assertRaises(OSError):
                perf.read_archive(artifact.with_suffix(""))

    def test_archive_preserves_measured_source_identity(self):
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
                self.assertEqual(perf.read_archive(next(evidence.glob("*.json")))["sparse"].result, result)
                perf.archive(path, local)
                self.assertEqual(perf.source_identity(), before)
                self.assertEqual(perf.read_archive(next(evidence.glob("*.json")))["sparse"].result, result)
                (root / "code.rs").write_text("different source")
                self.assertNotEqual(perf.source_identity(), before)

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
