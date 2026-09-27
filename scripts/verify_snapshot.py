"""Capture current inputs and run the ordinary verifier in an isolated checkout."""
import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

import verify
import verify_summary


def git(root, *args, data=None):
    return subprocess.run(["git", *args], cwd=root, input=data, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, check=True, env=verify.checkout_environment()).stdout


def check_links(path, root):
    # source_manifest already rejects cycles. Relative in-repository links keep
    # their meaning in the copy; external/absolute links could still read live data.
    if path.is_symlink():
        if os.path.isabs(os.readlink(path)) or not path.resolve().is_relative_to(root):
            raise ValueError("snapshot requires relative links within the checkout: " + str(path))
    if path.is_dir():
        for child in path.iterdir():
            check_links(child, root)


def capture(root, workspace, artifact):
    """Refuse changing or uncopiable inputs; never substitute committed content."""
    root = root.resolve()
    before = verify.source_manifest(root)
    revision = git(root, "rev-parse", "HEAD").decode().strip()
    index = git(root, "ls-files", "--stage", "-z")
    if any(entry.split(b"\t", 1)[0].split()[-1] != b"0" for entry in index.split(b"\0") if entry):
        raise ValueError("resolve unmerged index entries before capturing a snapshot")
    for name in before:
        if name == "target" or name.startswith("target/"):
            raise ValueError("snapshot build directory must not be a tracked input")
        check_links(root / name, root)

    staging = Path(tempfile.mkdtemp(prefix="capture-", dir=workspace.parent))
    try:
        # Copy Git objects rather than borrowing live metadata or an index. Pin
        # HEAD and recreate the captured index, including staged adds/deletions.
        git(root, "clone", "--local", "--no-hardlinks", "--no-checkout", str(root), str(staging))
        git(staging, "update-ref", "--no-deref", "HEAD", revision)
        git(staging, "read-tree", "--empty")
        git(staging, "update-index", "-z", "--index-info", data=index)
        for name in before:
            source, destination = root / name, staging / name
            if not source.exists() and not source.is_symlink():
                continue  # Preserve tracked deletions rather than restoring HEAD.
            destination.parent.mkdir(parents=True, exist_ok=True)
            if source.is_symlink():
                destination.symlink_to(os.readlink(source))
            else:
                shutil.copy2(source, destination)
        if (verify.source_manifest(root) != before or verify.source_manifest(staging) != before
                or git(root, "rev-parse", "HEAD").decode().strip() != revision
                or git(root, "ls-files", "--stage", "-z") != index):
            raise ValueError("inputs changed during snapshot capture; retry after the edits settle")
        if workspace.exists():
            shutil.rmtree(workspace)
        staging.rename(workspace)
        # The stable workspace path and private build directory retain compiler
        # caches across snapshot runs. The caller holds the lock for the whole run.
        build = workspace.parent / "build"
        build.mkdir(exist_ok=True)
        (workspace / "target").symlink_to(build, target_is_directory=True)
        if verify.source_manifest(workspace) != before:
            raise ValueError("snapshot inputs changed when installing the isolated checkout")
        archive = artifact / "source.tar"
        with tarfile.open(archive, "x") as bundle:
            for name in before:
                path = workspace / name
                if path.exists() or path.is_symlink():
                    bundle.add(path, arcname=name, recursive=False)
        captured = {"origin": str(root), "workspace": str(workspace), "revision": revision,
                    "source": verify.manifest_identity(before), "inputs": before,
                    "archive": str(archive), "archive_sha256": verify.digest(archive.read_bytes())}
        (artifact / "snapshot.json").write_text(json.dumps(captured, indent=2) + "\n")
        return captured
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def live_comparison(captured):
    try:
        current = verify.source_manifest(Path(captured["origin"]))
        expected = captured["inputs"]
        changed = verify_summary.changed_paths(expected, current)
        return {"matches": not changed, "source": verify.manifest_identity(current), "changed_paths": changed}
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        return {"matches": False, "error": str(error)}


def run(root, args):
    base = root / "target/verify"
    base.mkdir(parents=True, exist_ok=True)
    artifact = Path(tempfile.mkdtemp(prefix="run-", dir=base)).resolve()
    state = base / "snapshot"
    state.mkdir(exist_ok=True)
    result = {"schema": 1, "scope": args.scope, "checks": [], "coverage": [],
              "omitted": list(verify.CHECKS), "complete": False, "status": "inconclusive"}
    try:
        with (state / "lock").open("a") as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as error:
                raise ValueError("another snapshot verification is running; its checkout and build cache are reserved") from error
            workspace = (state / "workspace").resolve()
            captured = capture(root, workspace, artifact)
            print("Captured snapshot " + captured["source"] + "; continuing edits will not change these inputs.", flush=True)
            flags = [args.scope, "--snapshot-result", str(artifact), "--snapshot-lock-fd", str(lock.fileno())]
            for name in ("fresh", "performance", "live"):
                if getattr(args, name):
                    flags.append("--" + name)
            if args.only:
                flags += ["--only", args.only]
            environment = dict(verify.checkout_environment(), CARGO_TARGET_DIR=str(state.resolve() / "build"), PWD=str(workspace))
            code = subprocess.run([sys.executable, "-B", "scripts/verify.py", *flags],
                                  cwd=workspace, env=environment, check=False, pass_fds=(lock.fileno(),)).returncode
            result = json.loads((artifact / "verification.json").read_text())
            result["snapshot"] = captured
            result["live_checkout"] = live_comparison(captured)
            # Preserve the snapshot's result even if the user moved on. A check
            # that mutated the captured workspace still fails in the normal runner.
            print("Live checkout " + ("still matches the verified snapshot." if result["live_checkout"]["matches"]
                  else "differs or could not be compared; this result certifies only the snapshot."), flush=True)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        result.update(status="inconclusive", complete=False, reason=str(error), coverage=[], omitted=list(verify.CHECKS))
        summary = verify_summary.runner_failure(str(error), root,
            "python3 scripts/verify.py " + args.scope + " --snapshot" + (" --only " + args.only if args.only else ""))
        result.setdefault("failures", []).append(summary)
        verify_summary.display(summary)
        code = 2
    (artifact / "verification.json").write_text(json.dumps(result, indent=2) + "\n")
    print(result["status"] + ": " + str(artifact / "verification.json"), flush=True)
    return code
