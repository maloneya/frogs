#!/usr/bin/env python3
"""One verification entry point with immutable evidence and exact-input check reuse.

Quick is explicitly partial. Full includes all correctness checks and due headless
benchmarks. Live benchmarks require --live, --performance or --only performance-live;
due live coverage stays uncertified until explicitly measured. Only successful
correctness checks are cached. Performance samples are always freshly measured,
with baseline reuse owned by perf.py. No source edits or commands are inferred.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
import perf
import verify_summary

ROOT = Path(__file__).resolve().parents[1]
CORRECTNESS = ("clippy", "tests", "docs", "companion", "scenarios-fast", "scenarios-extended")
PERFORMANCE = ("performance-headless", "performance-live")
CHECKS = CORRECTNESS + PERFORMANCE
PERFORMANCE_INTERVAL = 10
PERFORMANCE_PATHS = ("crates", "assets", "benchmarks", "scripts", "Cargo.toml", "Cargo.lock",
                     "rust-toolchain.toml", ".cargo", ":(exclude)**/*.md")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def packed(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def checkout_environment():
    # Hooks can inherit Git variables pointing at another index/worktree. Every
    # Git operation here must address its explicit cwd, especially snapshot edits.
    local = ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR",
             "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES",
             "GIT_IMPLICIT_WORK_TREE", "GIT_GRAFT_FILE", "GIT_REPLACE_REF_BASE",
             "GIT_PREFIX", "GIT_SHALLOW_FILE", "GIT_CONFIG", "GIT_CONFIG_PARAMETERS", "GIT_CONFIG_COUNT")
    return {name: value for name, value in os.environ.items() if name not in local}


def input_digest(path, active=frozenset()):
    """Fingerprint linked inputs recursively; reject cycles and special files."""
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return digest(b"deleted")
    identity = (metadata.st_dev, metadata.st_ino)
    if identity in active:
        raise ValueError("cycle in verification inputs: " + str(path))
    active = active | {identity}
    mode = metadata.st_mode
    if stat.S_ISLNK(mode):
        target = os.readlink(path)
        linked = Path(target) if os.path.isabs(target) else path.parent / target
        return digest(packed({"link": target, "target": input_digest(linked, active)}))
    if stat.S_ISREG(mode):
        return digest(packed({"file": digest(path.read_bytes()), "mode": mode & 0o777}))
    if stat.S_ISDIR(mode):
        children = {child.name: input_digest(child, active)
                    for child in sorted(path.iterdir(), key=lambda p: os.fsencode(p.name))}
        return digest(packed({"directory": children, "mode": mode & 0o777}))
    raise ValueError("unsupported verification input: " + str(path))


def source_manifest(root=ROOT):
    """All tracked/unignored inputs, including dirty and untracked authored data.

    Ignore build outputs through Git's existing ignore rules. Track deletions and
    paths as well as bytes. A generated evidence index remains a real input: its
    changes must invalidate verification rather than silently escaping the key.
    """
    names = subprocess.check_output(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                                    cwd=root, env=checkout_environment())
    return {os.fsdecode(name): input_digest(root / os.fsdecode(name))
            for name in sorted(set(names.split(b"\0")) - {b""})}


def manifest_identity(manifest):
    state = hashlib.sha256()
    for name in sorted(manifest, key=os.fsencode):
        name = os.fsencode(name)
        state.update(name + b"\0")
        state.update(manifest[os.fsdecode(name)].encode())
    return state.hexdigest()


def source_identity(root=ROOT):
    return manifest_identity(source_manifest(root))


def context():
    # Tests and build scripts may read arbitrary inherited variables. Hash the
    # complete environment rather than guessing which prefixes are meaningful.
    # Values stay private; only the digest appears in stored evidence.
    env = dict(os.environ)
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    configs = {name: digest((cargo_home / name).read_bytes()) for name in ("config", "config.toml")
               if (cargo_home / name).is_file()}
    hardware = {}
    if platform.system() == "Darwin":
        for field in ("hw.model", "hw.memsize", "machdep.cpu.brand_string"):
            hardware[field] = subprocess.check_output(["sysctl", "-n", field], text=True).strip()
    return {"hardware": hardware, "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
            "cargo": subprocess.check_output(["cargo", "-V"], text=True),
            "swift": subprocess.check_output(["swift", "--version"], text=True, stderr=subprocess.STDOUT),
            "python": sys.version, "machine": platform.platform(),
            "environment": digest(packed(env)), "cargo_config": configs}


def scenario_files(suite, root=ROOT):
    if suite not in ("fast", "extended"):
        raise ValueError("unknown scenario suite: " + suite)
    manifest = json.loads((root / "scenarios/suites.json").read_text())
    if set(manifest) != {"extended"} or not isinstance(manifest["extended"], dict):
        raise ValueError("suite manifest must contain only an extended scenario/reason map")
    directory = root / "scenarios"
    files = {p.relative_to(directory).as_posix(): p for p in directory.rglob("*.ron")}
    extended = manifest["extended"]
    for name, reason in extended.items():
        if name not in files or not isinstance(reason, str) or not reason.strip():
            raise ValueError("unknown extended scenario or missing reason: " + name)
    selected = [str(path.relative_to(root)) for name, path in sorted(files.items())
                if (name in extended) == (suite == "extended")]
    if not selected:
        raise ValueError("empty scenario suite: " + suite)
    return selected


def commands(name, artifact, root=ROOT):
    if name == "clippy":
        return ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]
    if name == "tests":
        return ["cargo", "test", "--workspace"]
    if name == "companion":
        return [sys.executable, "scripts/companion.py", "test"]
    if name == "docs":
        return ["cargo", "doc", "--workspace", "--lib", "--bins", "--no-deps", "--document-private-items"]
    if name.startswith("scenarios-"):
        suite = name.removeprefix("scenarios-")
        return ["cargo", "run", "--quiet", "-p", "scenario", "--", "--timings",
                str(artifact / (name + ".timings.json"))] + scenario_files(suite, root)
    if name.startswith("performance-"):
        return [sys.executable, "scripts/perf.py", "run", "--mode", name.removeprefix("performance-"),
                "--output", str(artifact / (name + ".json"))]
    raise ValueError("unknown check: " + name)


def cache_key(source, machine, command, artifact):
    # Output destination varies per invocation but is not an input to the test.
    canonical = [part.replace(str(artifact), "<evidence>") for part in command]
    return digest(packed({"source": source, "context": machine, "command": canonical}))


def cached_record(cache, key, exit_code=0):
    try:
        record = json.loads((cache / (key + ".json")).read_text())
        if record["key"] != key or record["exit_code"] != exit_code:
            return None
        for item in record["evidence"]:
            if digest(Path(item["path"]).read_bytes()) != item["sha256"]:
                return None
        if not record["evidence"]:
            return None
        return record
    except (OSError, ValueError, KeyError, TypeError):
        return None


def run_check(name, command, source, machine, artifact, cache, fresh=False, root=ROOT, retry=False, source_inputs=None, pass_fds=()):
    performance = name in PERFORMANCE
    inputs = {**machine, "baseline_inputs": perf.evidence_identity()} if performance else machine
    key = cache_key(source, inputs, command, artifact)
    failures = cache.parent / "infrastructure-failures"
    if performance and not retry:
        blocked = cached_record(failures, key, exit_code=2)
        if blocked:
            return {**blocked, "cached": False, "deferred": True, "seconds": 0}
    old = None if fresh or name.startswith("performance-") else cached_record(cache, key)
    if old:
        return {**old, "cached": True}
    before = source_inputs if source_inputs is not None else source_manifest(root)
    log = artifact / (name + ".log")
    started = time.monotonic()
    with log.open("x") as out:
        code = subprocess.run(command, cwd=root, stdout=out, stderr=subprocess.STDOUT, check=False,
                              pass_fds=pass_fds, env=checkout_environment()).returncode
    record = {"name": name, "key": key, "command": command, "exit_code": code,
              "seconds": round(time.monotonic() - started, 3), "cached": False,
              "evidence": [{"path": str(log), "sha256": digest(log.read_bytes())}]}
    extra = artifact / (name + (".timings.json" if name.startswith("scenarios-") else ".json"))
    if code == 0 and name.startswith(("scenarios-", "performance-")) and not extra.is_file():
        record["exit_code"] = 2
        record["reason"] = "check did not produce its required structured evidence"
    if extra.exists() and performance and code == 2:
        try:
            failure = json.loads(extra.read_text()).get("failure", {})
            if failure.get("kind") == "infrastructure":
                record["infrastructure_failure"] = failure
        except (OSError, ValueError, TypeError, AttributeError):
            pass  # Unclassified failures must not suppress future measurements.
    if extra.exists():
        record["evidence"].append({"path": str(extra), "sha256": digest(extra.read_bytes())})
    if performance:
        record["evidence"].extend({"path": str(path), "sha256": digest(path.read_bytes())}
                                  for path in perf.output_paths(extra)[1:] if path.is_file())
    after = source_manifest(root)
    if manifest_identity(after) != source:
        record["exit_code"] = 2
        record["reason"] = "inputs changed during check; result cannot certify current source"
        record["source_changes"] = verify_summary.changed_paths(before, after)
    if record["exit_code"] == 0 and not name.startswith("performance-"):
        cache.mkdir(parents=True, exist_ok=True)
        destination = cache / (key + ".json")
        # Replace a corrupt/missing entry atomically; immutable run evidence stays.
        with tempfile.NamedTemporaryFile("w", dir=cache, delete=False) as out:
            json.dump(record, out, indent=2)
            temporary = out.name
        os.replace(temporary, destination)
    if performance and record["exit_code"] == 2 and "infrastructure_failure" in record and not record.get("reason"):
        failures.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile("w", dir=failures, delete=False) as out:
            json.dump(record, out, indent=2)
            temporary = out.name
        os.replace(temporary, failures / (key + ".json"))
    elif performance:
        (failures / (key + ".json")).unlink(missing_ok=True)
    return record


def performance_checkpoint(root, base):
    """Only intact successful evidence on this ancestry advances the cadence.

    Dirty runs count at their HEAD, conservatively leaving the eventual commit
    to count toward the next checkpoint. Modes have independent checkpoints.
    """
    environment = checkout_environment()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True, env=environment).strip()
    distances = {}
    for path in base.glob("run-*/verification.json"):
        try:
            result = json.loads(path.read_text())
            revision = result["revision"]
            if result.get("reason"):
                continue
            if subprocess.run(["git", "merge-base", "--is-ancestor", revision, head],
                              cwd=root, capture_output=True, env=environment).returncode:
                continue
            distance = int(subprocess.check_output(
                ["git", "rev-list", "--count", revision + ".." + head, "--", *PERFORMANCE_PATHS],
                cwd=root, text=True, env=environment))
            for row in result["checks"]:
                name = row["name"]
                if name not in PERFORMANCE or row["exit_code"] != 0 or not row["evidence"]:
                    continue
                if all(digest(Path(item["path"]).read_bytes()) == item["sha256"]
                       for item in row["evidence"]):
                    distances[name] = min(distance, distances.get(name, distance))
        except (OSError, ValueError, KeyError, TypeError):
            continue
    due = [name for name in PERFORMANCE
           if name not in distances or distances[name] >= PERFORMANCE_INTERVAL]
    return {"revision": head, "commits_since_pass": distances, "due": due}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scope", choices=("quick", "correctness", "full"), nargs="?", default="full")
    parser.add_argument("--performance", action="store_true", help="run both benchmark modes now; needs an uninterrupted foreground session")
    parser.add_argument("--live", action="store_true", help="run live benchmarks now; needs an uninterrupted foreground session")
    parser.add_argument("--only", choices=CHECKS, help="one explicitly partial check (for hooks)")
    parser.add_argument("--fresh", action="store_true", help="do not reuse correctness results")
    parser.add_argument("--snapshot", action="store_true", help="verify an isolated copy of current files while work continues")
    parser.add_argument("--snapshot-result", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--snapshot-lock-fd", type=int, help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.only and (args.live or args.performance):
        parser.error("--only cannot be combined with --live or --performance")
    if args.snapshot:
        if args.snapshot_result:
            parser.error("snapshot execution cannot create another snapshot")
        import verify_snapshot
        return verify_snapshot.run(ROOT, args)
    base = ROOT / "target/verify"
    if args.snapshot_result:
        base = args.snapshot_result.parent
    base.mkdir(parents=True, exist_ok=True)
    artifact = args.snapshot_result or Path(tempfile.mkdtemp(prefix="run-", dir=base))
    required = list(CHECKS)
    result = {"schema": 1, "scope": args.scope, "checks": [], "coverage": [], "omitted": required.copy(), "failures": []}
    retry_root = ROOT
    try:
        before = source_manifest(ROOT)
        source = manifest_identity(before)
        if args.snapshot_result:
            captured = json.loads((artifact / "snapshot.json").read_text())
            if source != captured["source"] or str(ROOT) != captured["workspace"]:
                raise ValueError("snapshot no longer matches its captured inputs")
            result["snapshot"] = captured
            retry_root = Path(captured["origin"])
        machine = context()
        checkpoint = performance_checkpoint(ROOT, base)
        requested = list(PERFORMANCE) if args.performance else (["performance-live"] if args.live else [])
        benchmarks = [name for name in PERFORMANCE if name in checkpoint["due"] or name in requested]
        required = list(CORRECTNESS) + benchmarks
        result.update(source=source, context=machine, revision=checkpoint["revision"],
                      performance_checkpoint=checkpoint, required=required)
        print("Performance checkpoint: " + (", ".join(benchmarks) if benchmarks else "not due"), flush=True)
        selected = [args.only] if args.only else list(required if args.scope == "full" else
                                                      CORRECTNESS if args.scope == "correctness" else
                                                      ("clippy", "tests", "scenarios-fast"))
        # Cadence requires evidence; it never authorizes taking over the desktop.
        if not args.only and "performance-live" not in requested:
            selected = [name for name in selected if name != "performance-live"]
        for name in requested:
            if name not in selected:
                selected.append(name)
        if "performance-live" in required and "performance-live" not in selected:
            print("Live performance due; not launched. Reserve an uninterrupted foreground session, "
                  "then run: python3 scripts/verify.py --only performance-live", flush=True)
        for name in selected:
            print("Checking " + name + "…", flush=True)
            row = run_check(name, commands(name, artifact, root=ROOT), source, machine, artifact, base / "cache", args.fresh,
                            root=ROOT, retry=name in requested or args.only in PERFORMANCE, source_inputs=before,
                            pass_fds=() if args.snapshot_lock_fd is None else (args.snapshot_lock_fd,))
            result["checks"].append(row)
            state = "deferred infrastructure failure" if row.get("deferred") else "cached pass" if row["cached"] else "pass" if row["exit_code"] == 0 else "inconclusive" if row["exit_code"] == 2 else "failed"
            print(f"  {state}: {row['seconds']:.2f}s; {row['evidence'][0]['path']}", flush=True)
            if row["exit_code"] != 0:
                summary = verify_summary.failure(row, retry_root, checked_root=ROOT)
                result["failures"].append(summary)
                verify_summary.display(summary)
            if row.get("reason"):
                break
        result["coverage"] = [row["name"] for row in result["checks"] if row["exit_code"] == 0]
        result["omitted"] = [name for name in required if name not in result["coverage"]]
        codes = [row["exit_code"] for row in result["checks"]]
        code = 1 if any(c not in (0, 2) for c in codes) else 2 if 2 in codes else 0
        after = source_manifest(ROOT)
        if manifest_identity(after) != source:
            code = 2
            result["reason"] = "inputs changed during verification"
            result["source_changes"] = verify_summary.changed_paths(before, after)
            result["coverage"] = []
            result["omitted"] = required.copy()
        result["complete"] = code == 0 and not result["omitted"]
        result["status"] = "complete_pass" if result["complete"] else "partial_pass" if code == 0 else "failed" if code == 1 else "inconclusive"
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        result.update(status="inconclusive", complete=False, reason=str(error))
        code = 2
    path = artifact / "verification.json"
    if result.get("reason") and not ("source_changes" in result and any(
            row.get("source_changes") == result["source_changes"] for row in result["checks"])):
        summary = verify_summary.runner_failure(result["reason"], retry_root,
            "python3 scripts/verify.py " + args.scope + " --snapshot", result.get("source_changes", []))
        result["failures"].append(summary)
        verify_summary.display(summary)
    path.write_text(json.dumps(result, indent=2) + "\n")
    print(result["status"] + ": " + str(path))
    if result["omitted"]:
        print("Not certified: " + ", ".join(result["omitted"]))
    return code


if __name__ == "__main__":
    raise SystemExit(main())
