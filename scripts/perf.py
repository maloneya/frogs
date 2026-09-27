#!/usr/bin/env python3
"""Release performance trials. Standard library only; see docs/performance.md.

Exit 0: pass, 1: regression, 2: inconclusive (including invalid evidence).
Baselines are immutable results, selected explicitly or from compatible archived evidence.
"""
import argparse
import contextlib
import datetime
import hashlib
import gzip
import json
import math
import os
from pathlib import Path
import platform
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import time
import zlib

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT / "benchmarks/suite.json"
# Schema 2 requires corpse telemetry; old observations cannot be reconstructed.
SCHEMA = 2
EVIDENCE = ROOT / "benchmarks/evidence"
LOCAL_EVIDENCE = ROOT / "target/perf/evidence"
METRICS = ("mean_step_ms", "presentation_ms.mean", "frame_ms.p99")
EXIT = {"pass": 0, "regression": 1, "inconclusive": 2}


def command(args, **kwargs):
    return subprocess.check_output(args, cwd=ROOT, text=True, **kwargs).strip()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    return digest(Path(path).read_bytes())


def load(path):
    def invalid(value):
        raise ValueError("non-finite JSON number: " + value)
    value = json.loads(Path(path).read_text(), parse_constant=invalid)
    if not isinstance(value, dict):
        raise ValueError("expected a JSON object: " + str(path))
    return value


def output_paths(output):
    return [Path(output), Path(str(output) + ".app.log"), Path(str(output) + ".build.log")]


def save(path, value):
    """Create exactly once; a run cannot accidentally overwrite its baseline."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x") as output:
        json.dump(value, output, indent=2, allow_nan=False)
        output.write("\n")


def number(value, name, positive=False):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError("missing or invalid measurement: " + name)
    if value < 0 or (positive and value == 0):
        raise ValueError("out-of-range measurement: " + name)
    return value


def fingerprint(value, length):
    if not isinstance(value, str) or len(value) != length or any(c not in "0123456789abcdef" for c in value):
        raise ValueError("missing or invalid fingerprint")


def metric(perf, name):
    value = perf
    for key in name.split("."):
        value = value[key]
    return number(value, name, positive=True)


def merge_status(states):
    # Missing/invalid evidence cannot be outvoted by another workload's pass.
    if "inconclusive" in states:
        return "inconclusive"
    return "regression" if "regression" in states else "pass"


def environment(perf):
    """Only actual presentation settings, not revision or changing population."""
    keys = ("schema_version", "debug_build", "vsync", "width", "height", "scale_factor",
            "refresh_millihertz", "gpu", "collision_debug", "view_height")
    settings = {key: perf[key] for key in keys}
    for asset in ("horde", "character"):
        settings[asset] = {key: perf[asset][key] for key in
                           ("active", "vertices", "indices", "part_count", "material_count", "image_count", "texture_bytes", "images", "nodes", "joints")}
    return settings


def outcome(trial):
    if "step_ns" in trial:
        return [trial["final_hash"], trial["initial_enemies"], trial["start_enemies"], trial["end_enemies"], trial["population_sum"]]
    info, perf = trial["trial"], trial["perf"]
    return [trial["final_hash"], info["initial_enemies"], info["start_enemies"],
            perf["current_enemies"], info["population_sum"]]


def validate_timings(perf, frames):
    """Cross-check gated summaries against raw rows, allowing Report's rounding."""
    intervals = sorted(number(f["frame_ms"], "frame_ms", positive=True) for f in frames)
    stepping = math.fsum(number(f["game_step_ms"], "game_step_ms") for f in frames)
    presentation = math.fsum(number(f["presentation_ms"], "presentation_ms") for f in frames)
    ticks = number(perf["ticks"], "ticks", positive=True)
    checks = {
        "window_ms": (math.fsum(intervals), len(frames), 1),
        "mean_step_ms": (stepping / ticks, len(frames), ticks),
        "presentation_ms.mean": (presentation / len(frames), len(frames), len(frames)),
        "frame_ms.p99": (intervals[math.ceil(len(frames) * 0.99) - 1], 1, 1),
    }
    for name, (expected, terms, divisor) in checks.items():
        # Each row and summary is rounded to four decimal places in milliseconds.
        # The small relative allowance also covers the reporter's f32 conversion.
        tolerance = 0.0001 * (terms + 1) / divisor
        if not math.isclose(metric(perf, name), expected, rel_tol=2e-6, abs_tol=tolerance):
            raise ValueError("summary contradicts raw timings: " + name)


def validate_trial(trial, workload):
    if "step_ns" in trial:
        if trial["debug_build"] is not False or trial["finite"] is not True or trial["schedule_version"] != 1:
            raise ValueError("invalid headless execution")
        for key in ("warmup_ticks", "sample_ticks", "input"):
            if trial[key] != workload[key]:
                raise ValueError("headless workload mismatch: " + key)
        if trial["current_tick"] != workload["warmup_ticks"] + workload["sample_ticks"]:
            raise ValueError("incomplete headless window")
        samples = trial["step_ns"]
        if not isinstance(samples, dict) or set(samples) != {str(i) for i in range(workload["sample_ticks"])}:
            raise ValueError("missing headless samples")
        for value in samples.values():
            number(value, "step_ns", positive=True)
        start = number(trial["start_enemies"], "population", positive=True)
        end = number(trial["end_enemies"], "population", positive=True)
        total = number(trial["population_sum"], "population sum", positive=True)
        if start != trial["initial_enemies"]:
            raise ValueError("population changed during warm-up")
        if workload["input"] == "idle":
            if end != start or total != start * workload["sample_ticks"]:
                raise ValueError("fixed-population workload lost enemies")
        elif not 0 < end < start or not end * workload["sample_ticks"] <= total <= start * workload["sample_ticks"]:
            raise ValueError("combat did not exercise defeats or inconsistent population")
        fingerprint(trial["final_hash"], 16)
        return {"mode": "headless"}
    info, perf = trial["trial"], trial["perf"]
    if trial["finite"] is not True:
        raise ValueError("non-finite simulation outcome")
    if info["invalid"]:
        raise ValueError(info["invalid"])
    if info["schedule_version"] != 1 or perf["schema_version"] != 1:
        raise ValueError("unsupported trial/telemetry version")
    for key in ("warmup_ticks", "sample_ticks", "input"):
        if info[key] != workload[key]:
            raise ValueError("trial does not match workload: " + key)
    if perf["debug_build"] is not False or perf["collision_debug"] is not False:
        raise ValueError("requires release build and collision debug off")
    for key in ("surface_skipped", "unfocused_frames", "occluded_frames", "overwritten"):
        if perf[key] != 0:
            raise ValueError("invalid measurement window: " + key)
    for key in ("width", "height", "scale_factor", "refresh_millihertz", "window_ms", "view_height"):
        number(perf[key], key, positive=True)
    if not isinstance(perf["frames"], dict):
        raise ValueError("frames must be an object keyed by consecutive frame indices")
    # JSON objects have no semantic ordering; an export sorted by key must work.
    frames = [perf["frames"][str(i)] for i in range(len(perf["frames"]))]
    if len(frames) < 30 or perf["samples"] != len(frames) or perf["presented"] != len(frames):
        raise ValueError("insufficient or inconsistent completed frames")
    if perf["ticks"] != info["sample_ticks"]:
        raise ValueError("incomplete tick window (reset or overwritten samples)")
    tick = info["warmup_ticks"]
    for frame in frames:
        tick += number(frame["ticks"], "frame ticks")
        if frame["end_tick"] != tick:
            raise ValueError("noncontiguous tick window")
        if frame["presented"] is not True or frame["focused"] is not True or frame["occluded"] is not False:
            raise ValueError("frame was skipped, unfocused or occluded")
        number(frame["frame_ms"], "frame_ms", positive=True)
        number(frame["enemies"], "frame population", positive=True)
    if tick != info["warmup_ticks"] + info["sample_ticks"] or perf["current_tick"] != tick:
        raise ValueError("wrong trial endpoint")
    validate_timings(perf, frames)
    start = number(info["start_enemies"], "start population", positive=True)
    end = number(perf["current_enemies"], "end population", positive=True)
    if start != info["initial_enemies"]:
        raise ValueError("population changed during warm-up")
    if workload["input"] == "idle":
        if end != start or info["population_sum"] != start * info["sample_ticks"]:
            raise ValueError("fixed-population workload lost enemies")
        if any(frame["enemies"] != start for frame in frames):
            raise ValueError("fixed-population frames changed workload")
    elif not 0 < end < start or not end * info["sample_ticks"] <= info["population_sum"] <= start * info["sample_ticks"]:
        raise ValueError("combat did not exercise defeats or has inconsistent population")
    horde = perf["horde"]
    corpses = number(perf["current_corpses"], "corpse population")
    if horde["corpse_instances"] != corpses or horde["instances"] != end + corpses:
        raise ValueError("rendered living and corpse populations disagree with Game")
    if horde["idle_instances"] + horde["run_instances"] != end or horde[workload["animation"] + "_instances"] <= 0:
        raise ValueError("expected enemy animation was not presented")
    fingerprint(trial["final_hash"], 16)
    for name in METRICS:
        metric(perf, name)
    return environment(perf)


def validate_result(result):
    if result["schema_version"] != SCHEMA or result["errors"]:
        raise ValueError("run failed or has unsupported schema: " + str(result.get("errors")))
    fingerprint(result["source"]["revision"], 40)
    fingerprint(result["source"]["digest"], 64)
    fingerprint(result["binary_sha256"], 64)
    fingerprint(result["workload_digest"], 64)
    if not isinstance(result["source"]["dirty"], bool):
        raise ValueError("missing source state")
    conditions = result["conditions"]
    for key in ("machine", "cpu", "memory_bytes", "os", "rustc"):
        if not isinstance(conditions[key], str) or not conditions[key]:
            raise ValueError("missing machine/compiler conditions: " + key)
    if conditions["build"] not in ("cargo build --release -p arpg", "cargo build --release -p scenario --bin perf-headless") or not isinstance(conditions["build_environment"], dict):
        raise ValueError("missing release build conditions")
    policy = result["suite"]
    if policy["version"] != 1 or not 3 <= policy["repetitions"] <= 10:
        raise ValueError("requires 3..10 repetitions and supported suite")
    for key in ("relative_tolerance", "max_trial_spread"):
        if not 0 < number(policy[key], key) < 1:
            raise ValueError("invalid policy: " + key)
    expected = [w["name"] for w in policy["workloads"]]
    if not expected or len(set(expected)) != len(expected) or set(result["workloads"]) != set(expected):
        raise ValueError("missing or duplicate workloads")
    settings = None
    for workload in policy["workloads"]:
        if set(workload["budgets_ms"]) != {"mean_step_ms", "frame_ms.p99"}:
            raise ValueError("workload must declare step and frame budgets")
        trials = result["workloads"][workload["name"]]
        if len(trials) != policy["repetitions"]:
            raise ValueError("missing repetitions: " + workload["name"])
        for trial in trials:
            if ("step_ns" in trial) != ("perf-headless" in conditions["build"]):
                raise ValueError("trial does not match recorded build mode")
            current = validate_trial(trial, workload)
            if settings is not None and settings != current:
                raise ValueError("presentation settings changed between trials")
            settings = current
            if outcome(trial) != outcome(trials[0]):
                raise ValueError("repeated inputs produced different outcomes: " + workload["name"])
    return settings


def trial_metric(trial, name):
    if "step_ns" in trial:
        return statistics.mean(trial["step_ns"].values()) / 1e6
    return metric(trial["perf"], name)


def selected(result, workload):
    """Validate each workload independently; failures cannot erase other evidence."""
    part = dict(result)
    part["suite"] = {**result["suite"], "workloads": [workload]}
    part["workloads"] = {workload["name"]: result["workloads"][workload["name"]]}
    return part


def assess(current, baseline=None):
    """Revalidate raw evidence; absolute budgets survive missing relative evidence."""
    result = {"status": "inconclusive", "scope": "relative_and_absolute" if baseline else "absolute_only",
              "reasons": [], "metrics": [], "relative_status": "missing" if baseline is None else "pass",
              "rerun_workloads": []}
    states, relative_states = [], []
    try:
        workloads = current["suite"]["workloads"]
        names = [w["name"] for w in workloads]
        if len(names) != len(set(names)) or set(names) != set(current["workloads"]):
            raise ValueError("missing or duplicate workloads")
        if not workloads:
            raise ValueError("missing workloads")
        invalid_workloads = False
        for workload in workloads:
            label = workload["name"]
            try:
                settings = validate_result(selected(current, workload))
            except (KeyError, TypeError, ValueError, IndexError) as error:
                result["reasons"].append(label + ": " + str(error))
                states.append("inconclusive")
                invalid_workloads = True
                result["rerun_workloads"].append(label)
                continue
            trials = current["workloads"][label]
            previous, mismatch = None, None
            if baseline is not None:
                try:
                    old_work = next(w for w in baseline["suite"]["workloads"] if w["name"] == label)
                    if settings != validate_result(selected(baseline, old_work)):
                        raise ValueError("presentation settings differ")
                    for key in ("conditions", "workload_digest"):
                        if current[key] != baseline[key]:
                            raise ValueError("baseline comparison mismatch: " + key)
                    if selected(current, workload)["suite"] != selected(baseline, old_work)["suite"]:
                        raise ValueError("baseline policy/workload differs")
                    previous = baseline["workloads"][label]
                    if outcome(trials[0]) != outcome(previous[0]):
                        raise ValueError("baseline gameplay/population differs")
                except (KeyError, TypeError, ValueError, IndexError, StopIteration) as error:
                    mismatch = str(error) or "baseline workload missing"
                    previous = None
                    relative_states.append("inconclusive")
                    result["reasons"].append(label + ": " + mismatch)
            names = ("mean_step_ms",) if "step_ns" in trials[0] else METRICS
            for name in names:
                values = [trial_metric(t, name) for t in trials]
                row = {"workload": label, "metric": name, "current_ms": values,
                       "median_ms": statistics.median(values), "relative": "incompatible" if mismatch else "missing"}
                checks = []
                stable = (max(values) - min(values)) / statistics.median(values) <= current["suite"]["max_trial_spread"]
                row["stability"] = "stable" if stable else "inconclusive"
                if not stable:
                    checks.append("inconclusive")
                if name in workload["budgets_ms"]:
                    limit = number(workload["budgets_ms"][name], "budget", positive=True)
                    state = "inconclusive" if not stable else "pass" if max(values) <= limit else "regression" if min(values) > limit else "inconclusive"
                    row.update(absolute=state, budget_ms=limit)
                    checks.append(state)
                if previous is not None:
                    old = [trial_metric(t, name) for t in previous]
                    old_stable = (max(old) - min(old)) / statistics.median(old) <= current["suite"]["max_trial_spread"]
                    factor = 1 + current["suite"]["relative_tolerance"]
                    state = "inconclusive" if not stable or not old_stable else "pass" if max(values) <= min(old) * factor else "regression" if min(values) > max(old) * factor else "inconclusive"
                    row.update(relative=state, baseline_ms=old,
                               change_percent=100 * (statistics.median(values) / statistics.median(old) - 1))
                    checks.append(state)
                    relative_states.append(state)
                elif mismatch:
                    checks.append("inconclusive")
                row["status"] = merge_status(checks) if checks else "observed"
                states.extend(checks)
                result["metrics"].append(row)
                if row["status"] == "inconclusive" and not mismatch and label not in result["rerun_workloads"]:
                    result["rerun_workloads"].append(label)
        result["status"] = merge_status(states)
        if relative_states:
            result["relative_status"] = merge_status(relative_states)
        result["absolute_status"] = merge_status([row["absolute"] for row in result["metrics"] if "absolute" in row] +
                                                  (["inconclusive"] if invalid_workloads or len(result["metrics"]) == 0 else []))
    except (KeyError, TypeError, ValueError, IndexError) as error:
        result["reasons"].append(str(error))
    return result


def source_identity(exclude=()):
    excluded = {Path(path).resolve() for path in exclude}
    excluded.add(EVIDENCE.resolve())
    paths = ["."]
    for path in sorted(excluded):
        try:
            paths.append(":(exclude)" + str(path.relative_to(ROOT)) + ("/**" if path.is_dir() else ""))
        except ValueError:
            pass  # Artifacts outside the checkout cannot affect its identity.
    files = command(["git", "ls-files", "-co", "--exclude-standard", "-z"]).split("\0")
    contents = {p: file_digest(ROOT / p) for p in sorted(set(files))
                if p and (ROOT / p).is_file() and not any((ROOT / p).resolve() == e or e in (ROOT / p).resolve().parents for e in excluded)}
    return {"revision": command(["git", "rev-parse", "HEAD"]),
            "dirty": bool(command(["git", "status", "--porcelain", "--untracked-files=all", "--", *paths])),
            "digest": digest(json.dumps(contents, sort_keys=True).encode())}


def host_conditions():
    return {"machine": command(["sysctl", "-n", "hw.model"]),
            "cpu": command(["sysctl", "-n", "machdep.cpu.brand_string"]),
            "memory_bytes": command(["sysctl", "-n", "hw.memsize"]),
            "os": platform.platform(), "rustc": command(["rustc", "-vV"]),
            "build": "cargo build --release -p arpg",
            "build_environment": {k: v for k, v in os.environ.items()
                                  if k in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER")
                                  or k.startswith("CARGO_PROFILE_RELEASE_")}}


def other_games():
    found = []
    for line in command(["ps", "-axo", "pid=,stat=,comm="]).splitlines():
        fields = line.strip().split(None, 2)
        if len(fields) == 3 and Path(fields[2]).name == "arpg" and "T" not in fields[1]:
            found.append(int(fields[0]))
    return found


def request(path, text, timeout=20):
    with socket.socket(socket.AF_UNIX) as stream:
        stream.settimeout(timeout)
        stream.connect(str(path))
        stream.sendall((text + "\n").encode())
        stream.shutdown(socket.SHUT_WR)
        parts = []
        size = 0
        while True:
            part = stream.recv(65536)
            if not part:
                break
            size += len(part)
            if size > 4 * 1024 * 1024:
                raise ValueError("harness reply exceeds bounded telemetry size")
            parts.append(part)
    answer = b"".join(parts).decode().strip()
    if not answer or answer.startswith("error"):
        raise ValueError("harness: " + answer)
    return answer


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


@contextlib.contextmanager
def app(binary, output):
    # Private short socket path: never touch another process's harness or files.
    with tempfile.TemporaryDirectory(prefix="arpg-perf-", dir="/tmp") as directory:
        path = Path(directory) / "control.sock"
        with Path(str(output) + ".app.log").open("x") as log:
            process = subprocess.Popen([str(binary)], cwd=ROOT, stdout=log, stderr=log,
                                       env={**os.environ, "ARPG_HARNESS": str(path), "RUST_LOG": "info"})
            awake = None
            try:
                awake = subprocess.Popen(["caffeinate", "-d", "-i", "-w", str(process.pid)])
                deadline = time.monotonic() + 60
                while True:
                    if process.poll() is not None:
                        raise ValueError("game exited during startup; inspect app log")
                    try:
                        request(path, "perf")
                        break
                    except (FileNotFoundError, ConnectionRefusedError):
                        if time.monotonic() > deadline:
                            raise ValueError("game harness did not become ready")
                        time.sleep(0.1)
                yield path
            finally:
                stop(process)
                if awake is not None:
                    stop(awake)


def workload_scenes(suite):
    """Performance inputs belong to benchmarks, never to editable game content."""
    directory = (ROOT / "benchmarks").resolve()
    paths = [(ROOT / work["scene"]).resolve() for work in suite["workloads"]]
    for path in paths:
        if not path.is_relative_to(directory):
            raise ValueError("benchmark scene must be owned by benchmarks/: " + str(path))
    return paths


def prepare(args, result):
    """Collect baseline eligibility metadata before building or measuring."""
    suite = load(SUITE)
    workload_files = workload_scenes(suite)
    headless = args.mode == "headless"
    if args.workload:
        suite["workloads"] = [w for w in suite["workloads"] if w["name"] in args.workload]
        if {w["name"] for w in suite["workloads"]} != set(args.workload):
            raise ValueError("unknown workload")
    result["suite"] = suite
    if platform.system() != "Darwin":
        raise ValueError("live benchmarks require the target macOS machine")
    games = other_games()
    if games:
        raise ValueError("other game processes are running (leave them untouched): " + str(games))
    result["source"] = source_identity(output_paths(args.output))
    result["conditions"] = host_conditions()
    build_args = ["cargo", "build", "--release", "-p", "scenario", "--bin", "perf-headless"] if headless else ["cargo", "build", "--release", "-p", "arpg"]
    result["conditions"]["build"] = " ".join(build_args)
    workload_files += [ROOT / "benchmarks/driver.rs"]
    if not headless:
        workload_files += sorted((ROOT / "assets").rglob("*.glb"))
    result["workload_digest"] = digest(json.dumps({str(p.relative_to(ROOT)): file_digest(p) for p in workload_files}, sort_keys=True).encode())


def run(args, result):
    """Collect into the caller-owned result so later failures cannot erase it."""
    suite = result["suite"]
    headless = args.mode == "headless"
    outputs = output_paths(args.output)
    before = result["source"]
    build_args = result["conditions"]["build"].split()
    stage = "build"
    try:
        print("Building release app…", flush=True)
        build = subprocess.run([*build_args, "--message-format=json"],
                               cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        artifacts = []
        with Path(str(args.output) + ".build.log").open("x") as build_log:
            build_log.write(build.stderr)
            for line in build.stdout.splitlines():
                message = json.loads(line)
                if message.get("reason") == "compiler-message":
                    rendered = message["message"].get("rendered")
                    if rendered:
                        build_log.write(rendered)
                elif message.get("reason") == "compiler-artifact":
                    artifacts.append(message)
        if build.returncode:
            raise ValueError("release build failed; inspect build log")
        binary = next(Path(a["executable"]) for a in artifacts
                      if a.get("target", {}).get("name") == ("perf-headless" if headless else "arpg") and a.get("executable"))
        result["binary_sha256"] = file_digest(binary)
        stage = "measurement"
        if headless:
            for workload in suite["workloads"]:
                trials = result["workloads"][workload["name"]] = []
                for repetition in range(suite["repetitions"]):
                    print(f"{workload['name']}: headless trial {repetition + 1}/{suite['repetitions']}", flush=True)
                    trial = json.loads(command([str(binary), workload["scene"], str(workload["warmup_ticks"]),
                                               str(workload["sample_ticks"]), workload["input"]]))
                    trials.append(trial)
            if source_identity(outputs) != before or file_digest(binary) != result["binary_sha256"]:
                raise ValueError("source or executable changed during measurement")
            return result
        with app(binary, args.output) as path:
            if request(path, "vsync " + args.vsync) != "vsync " + str(args.vsync == "on").lower():
                raise ValueError("requested present mode unavailable")
            request(path, "debug collision off")
            for workload in suite["workloads"]:
                name = workload["name"]
                result["workloads"][name] = []
                answer = request(path, "scene start " + str(ROOT / workload["scene"]))
                if not answer.startswith("ready "):
                    raise ValueError("scene did not become ready: " + answer)
                for repetition in range(suite["repetitions"]):
                    print(f"{name}: trial {repetition + 1}/{suite['repetitions']} (warm-up + measurement)", flush=True)
                    text = f"perf trial {workload['warmup_ticks']} {workload['sample_ticks']} {workload['input']}"
                    trial = json.loads(request(path, text, timeout=180))
                    result["workloads"][name].append(trial)
        if other_games():
            raise ValueError("another game started during measurement")
        if source_identity(outputs) != before or file_digest(binary) != result["binary_sha256"]:
            raise ValueError("source or executable changed during measurement")
    except (OSError, ValueError, KeyError, TypeError, IndexError, StopIteration, subprocess.SubprocessError) as error:
        result["errors"].append(str(error))
        result["failure"] = {"kind": "infrastructure" if stage == "build" else "measurement",
                             "stage": stage, "reason": str(error)}
    return result


def archive(path, directory=None):
    """Explicit immutable publication, including inconclusive but valid measurements."""
    return archive_result(load(path), directory)


def archive_result(result, directory=None):
    validate_result(result)
    raw = json.dumps(result, sort_keys=True, allow_nan=False).encode()
    identity = digest(raw)
    directory = Path(directory) if directory is not None else EVIDENCE
    directory.mkdir(parents=True, exist_ok=True)
    artifact = directory / (identity + ".json.gz")
    with artifact.open("xb") as stream:
        stream.write(gzip.compress(raw, mtime=0))
    manifest = {"schema_version": SCHEMA, "artifact": artifact.name, "sha256": identity, "source": result["source"],
                "conditions": result["conditions"], "workload_digest": result["workload_digest"],
                "created_utc": result.get("created_utc", ""), "suite": result["suite"]}
    save(directory / (identity + ".json"), manifest)
    return artifact


def eligible(current, offered, automatic=True):
    """Metadata-only filter; never parse trials from an unrelated benchmark."""
    if automatic and offered["source"]["dirty"] and offered["source"]["digest"] != current["source"]["digest"]:
        return False
    if any(current[key] != offered[key] for key in ("conditions", "workload_digest")):
        return False
    wanted = {w["name"]: w for w in current["suite"]["workloads"]}
    available = {w["name"]: w for w in offered["suite"]["workloads"]}
    return (all(available.get(k) == v for k, v in wanted.items()) and
            all(current["suite"][k] == offered["suite"][k]
                for k in ("version", "repetitions", "relative_tolerance", "max_trial_spread")))


def evidence_identity():
    """Changes to local baseline inputs must release a deferred infrastructure failure."""
    return digest(json.dumps({str(p): file_digest(p)
                              for store in (EVIDENCE, LOCAL_EVIDENCE)
                              for p in sorted(store.glob("*.json*"))}, sort_keys=True).encode())


def discover(current, directory=None, exclusions=None):
    """Preflight nearest eligible ancestor; validate raw evidence only after filtering."""
    best = None
    stores = (Path(directory),) if directory is not None else (EVIDENCE, LOCAL_EVIDENCE)
    manifests = sorted(path for store in stores for path in store.glob("*.json"))
    for manifest_path in manifests:
        manifest = load(manifest_path)
        if manifest.get("schema_version") != SCHEMA:
            if exclusions is not None:
                exclusions.append({"path": str(manifest_path), "reason": "unsupported evidence schema"})
            continue
        if not eligible(current, manifest):
            continue
        revision = manifest["source"]["revision"]
        ancestor = subprocess.run(["git", "merge-base", "--is-ancestor", revision, current["source"]["revision"]], cwd=ROOT, capture_output=True)
        if ancestor.returncode:
            continue
        fingerprint(manifest["sha256"], 64)
        if manifest["artifact"] != manifest["sha256"] + ".json.gz":
            raise ValueError("invalid evidence artifact path")
        raw = gzip.decompress((manifest_path.parent / manifest["artifact"]).read_bytes())
        if digest(raw) != manifest["sha256"]:
            raise ValueError("corrupt archived evidence: " + str(manifest_path))
        result = json.loads(raw)
        for key in ("schema_version", "source", "conditions", "workload_digest", "suite"):
            if manifest[key] != result[key]:
                raise ValueError("manifest contradicts raw evidence: " + key)
        validate_result(result)
        distance = int(command(["git", "rev-list", "--count", revision + ".." + current["source"]["revision"]]))
        # Keep only the winner's raw frame dictionaries, not the entire history.
        rank = (-distance, manifest.get("created_utc", ""), str(manifest_path))
        if best is None or rank > best[0]:
            best = (rank, result)
    if best is None:
        return None, None
    return best[1], best[0][2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    live = sub.add_parser("run", help="build release and collect the versioned suite")
    live.add_argument("--mode", choices=("live", "headless"), default="live")
    live.add_argument("--rerun", type=Path, help="select uncertain workloads from a previous result; preserve that result")
    live.add_argument("--output", type=Path, required=True)
    live.add_argument("--baseline", type=Path)
    live.add_argument("--workload", action="append", help="select workload; may repeat (default: full suite)")
    live.add_argument("--vsync", choices=("on", "off"), default="on")
    compare = sub.add_parser("compare", help="revalidate and compare saved results without launching the game")
    compare.add_argument("baseline", type=Path)
    compare.add_argument("candidate", type=Path)
    compare.add_argument("--output", type=Path, required=True)
    publish = sub.add_parser("archive", help="check in immutable raw evidence and its provenance manifest")
    publish.add_argument("result", type=Path)
    args = parser.parse_args()
    if args.command == "archive":
        try:
            print(archive(args.result))
            return 0
        except (OSError, ValueError, KeyError, TypeError) as error:
            print("inconclusive: " + str(error), file=sys.stderr)
            return 2
    if args.command == "run" and args.rerun:
        if args.workload:
            parser.error("choose --rerun or --workload")
        previous = load(args.rerun)
        args.workload = assess(previous)["rerun_workloads"]
        if not args.workload:
            parser.error("no uncertain workloads to rerun")
    # Fail before expensive work if the immutable destination is already used.
    if args.output.exists():
        parser.error("output exists; choose a new result path")
    cargo_bin = str(Path.home() / ".cargo/bin")
    if not shutil.which("cargo"):
        os.environ["PATH"] = cargo_bin + os.pathsep + os.environ.get("PATH", "")
    result = {"schema_version": SCHEMA, "created_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "errors": [], "workloads": {}}
    stage = "baseline_preflight"
    try:
        if args.command == "run":
            for path in output_paths(args.output):
                if os.path.lexists(path):
                    raise ValueError("output exists; choose a new result path: " + str(path))
        args.output.parent.mkdir(parents=True, exist_ok=True)
        baseline = load(args.baseline) if args.baseline else None
        if baseline is not None:
            validate_result(baseline)
        if args.command == "run":
            prepare(args, result)
            baseline_path = str(args.baseline.resolve()) if args.baseline else None
            result["baseline_exclusions"] = []
            if baseline is None:
                baseline, baseline_path = discover(result, exclusions=result["baseline_exclusions"])
            elif not eligible(result, baseline, automatic=False):
                # Explicit dirty baselines are allowed, but known workload/mode mismatches are not.
                raise ValueError("explicit baseline conditions or workload differ")
            if result["baseline_exclusions"]:
                print(f"Retired {len(result['baseline_exclusions'])} incompatible evidence manifests (schema {SCHEMA} required).", flush=True)
            result["baseline"] = baseline_path
            stage = "measurement"
            run(args, result)
            stage = "assessment"
            verdict = assess(result, baseline)
            result["assessment"] = verdict
            result["baseline"] = baseline_path
        else:
            result = verdict = assess(load(args.candidate), baseline)
            result["baseline"] = str(args.baseline.resolve())
            result["candidate"] = str(args.candidate.resolve())
        if baseline is not None:
            result["baseline_digest"] = digest(json.dumps(baseline, sort_keys=True).encode())
            result["baseline_source"] = baseline.get("source") if isinstance(baseline, dict) else None
        stage = "archive"
        if args.command == "run":
            try:
                validate_result(result)
            except (KeyError, TypeError, ValueError, IndexError):
                pass  # Partial/invalid runs remain at the requested output, never eligible baselines.
            else:
                archive_result(result, LOCAL_EVIDENCE)
        save(args.output, result)
    except (OSError, EOFError, zlib.error, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        print("inconclusive: " + str(error), file=sys.stderr)
        try:
            # Keep raw trials and any absolute assessment already obtained.
            result["failure"] = {"kind": "infrastructure", "stage": stage, "reason": str(error)}
            if "assessment" not in result:
                result["assessment"] = assess(result)
            verdict = result["assessment"]
            verdict["status"] = "inconclusive"
            verdict.setdefault("reasons", []).append(str(error))
            save(args.output, result)
        except OSError:
            pass  # Never overwrite an existing result, including after a race.
        return 2
    print(verdict["status"] + ": " + verdict["scope"], flush=True)
    for reason in verdict["reasons"]:
        print("  " + reason)
    for row in verdict["metrics"]:
        print(f"  {row['workload']}/{row['metric']}: {row['median_ms']:.3f} ms — {row['status']}")
    print("Result: " + str(args.output))
    return EXIT[verdict["status"]]


if __name__ == "__main__":
    sys.exit(main())
