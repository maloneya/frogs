#!/usr/bin/env python3
"""Bounded recipe search with fixed Rust evaluation and an optional JSON proposer."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys

ROOT = Path(__file__).resolve().parent


def strict_json(text):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    def constant(value):
        raise ValueError(f"non-finite JSON constant: {value}")

    return json.loads(text, object_pairs_hook=pairs, parse_constant=constant)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def protected_files():
    files = [ROOT / name for name in ("Cargo.toml", "Cargo.lock", "research.py", "codex_proposer.py", "program.md")]
    for folder in ("src", "tests"):
        files.extend(p for p in (ROOT / folder).rglob("*") if p.is_file() and "__pycache__" not in p.parts)
    return {str(p.relative_to(ROOT)): digest(p) for p in sorted(files)}


def verify(snapshot, binary, binary_hash):
    if protected_files() != snapshot or digest(binary) != binary_hash:
        raise RuntimeError("protected source or evaluator binary changed; experiment aborted")


def logged(command, directory, prefix, timeout, input_text=None, cwd=ROOT):
    """Bound the entire process group, keeping stdout/stderr as inspectable evidence."""
    stdout = directory / f"{prefix}.stdout.txt"
    stderr = directory / f"{prefix}.stderr.txt"
    with stdout.open("w") as out, stderr.open("w") as err:
        process = subprocess.Popen(command, cwd=cwd, stdin=subprocess.PIPE,
                                   stdout=out, stderr=err, text=True, start_new_session=True)
        try:
            process.communicate(input_text, timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.communicate()
            raise RuntimeError(f"{prefix} timed out after {timeout}s") from None
    if process.returncode:
        raise RuntimeError(f"{prefix} exited {process.returncode}; see {stderr}")
    return stdout


def validate_proposal(proposal, fields):
    if not isinstance(proposal, dict) or set(proposal) != {"hypothesis", "recipe"}:
        raise ValueError("proposal must contain exactly hypothesis and recipe")
    if not isinstance(proposal["hypothesis"], str) or not 1 <= len(proposal["hypothesis"]) <= 2000:
        raise ValueError("hypothesis must be 1..2000 characters")
    recipe = proposal["recipe"]
    if not isinstance(recipe, dict) or set(recipe) != {f["name"] for f in fields}:
        raise ValueError("recipe must contain exactly the fields supplied by the evaluator")
    for field in fields:
        value = recipe[field["name"]]
        if type(value) is not int or not field["min"] <= value <= field["max"]:
            raise ValueError(f"invalid value for {field['name']}")
    return recipe


def recipe_text(recipe, fields):
    return "".join(f"{field['name']}={recipe[field['name']]}\n" for field in fields)


def propose_builtin(best, fields, seen):
    for field in fields:
        for direction in (-1, 1):
            value = best[field["name"]] + direction * field["step"]
            candidate = dict(best, **{field["name"]: value})
            key = tuple(candidate[f["name"]] for f in fields)
            if field["min"] <= value <= field["max"] and key not in seen:
                return {"hypothesis": f"Change {field['name']} from {best[field['name']]} to {value}; test navigation cost.",
                        "recipe": candidate}
    return None


def summary_metrics(report):
    return {key: report[key] for key in ("mean_score", "worst_score", "failed_maps", "incomplete_plays")}


def better(candidate, baseline, strict):
    # Never trade a failure or a worse worst-case for an average improvement.
    return (candidate["failed_maps"] <= baseline["failed_maps"]
            and candidate["incomplete_plays"] <= baseline["incomplete_plays"]
            and candidate["worst_score"] >= baseline["worst_score"]
            and candidate["mean_score"] >= baseline["mean_score"] + (1.0 if strict else 0.0))


def examples(candidate, baseline):
    old = {case["case"]: case for case in baseline["results"]}
    changes = [{"case": case["case"], "score_delta": round(case["score"] - old[case["case"]]["score"], 6)}
               for case in candidate["results"]]
    changes.sort(key=lambda c: (c["score_delta"], c["case"]))
    return {"most_worsened": changes[:3], "most_improved": changes[-3:]}


def save_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def evaluate(binary, recipe_file, suite, directory, timeout):
    logged([str(binary), "evaluate", "--suite", suite, "--recipe", str(recipe_file),
            "--output", str(directory / suite)], directory, suite, timeout)
    report = strict_json((directory / suite / "summary.json").read_text())
    for key in ("mean_score", "worst_score"):
        if not math.isfinite(report[key]):
            raise RuntimeError("evaluator returned non-finite score")
    return report


def run(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    snapshot = protected_files()
    build_target = output / "build-target"
    logged(["cargo", "build", "--release", "--locked", "--target-dir", str(build_target)], output, "build", args.timeout)
    if protected_files() != snapshot:
        raise RuntimeError("protected source changed during compilation")
    binary = output / "evaluator"
    shutil.copy2(build_target / "release" / "levelgen", binary)
    binary.chmod(0o555)
    binary_hash = digest(binary)
    provenance = output / "source"
    for name in snapshot:
        destination = provenance / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / name, destination)
    schema_file = logged([str(binary), "recipe", "--schema"], output, "schema", args.timeout)
    fields = strict_json(schema_file.read_text())["fields"]
    best = {f["name"]: f["default"] for f in fields}
    initial_recipe = best.copy()
    manifest = {"source_sha256": snapshot, "binary_sha256": binary_hash, "schema": fields,
                "experiments_budget": args.experiments, "timeout_seconds": args.timeout,
                "proposer": args.proposer or "builtin coordinate search",
                "selection": "development mean +1, validation mean nondecrease, neither suite may regress worst score or failures"}
    save_json(output / "manifest.json", manifest)
    baseline_dir = output / "baseline"
    baseline_dir.mkdir()
    recipe_file = baseline_dir / "recipe.txt"
    recipe_file.write_text(recipe_text(best, fields))
    baseline = {s: evaluate(binary, recipe_file, s, baseline_dir, args.timeout)
                for s in ("development", "validation")}
    incumbent = baseline
    incumbent_dir = baseline_dir
    history = []
    seen = {tuple(best[f["name"]] for f in fields)}
    journal = output / "experiments.jsonl"
    journal.touch()
    print(f"Baseline: development={baseline['development']['mean_score']:.2f}, validation={baseline['validation']['mean_score']:.2f}", flush=True)
    for number in range(1, args.experiments + 1):
        verify(snapshot, binary, binary_hash)
        directory = output / f"experiment-{number:03}"
        directory.mkdir()
        context = {"experiment": number, "remaining": args.experiments - number + 1,
                   "schema": fields, "objective": baseline["development"]["objective"],
                   "selection": manifest["selection"], "incumbent_recipe": best,
                   "incumbent_reports": {s: summary_metrics(r) for s, r in incumbent.items()},
                   "incumbent_artifacts": str(incumbent_dir), "recent_experiments": history[-8:]}
        save_json(directory / "context.json", context)
        record = {"experiment": number, "artifacts": str(directory), "status": "invalid"}
        try:
            if args.proposer:
                response = logged(shlex.split(args.proposer), directory, "proposer", args.timeout,
                                  json.dumps(context), cwd=directory)
                if response.stat().st_size > 65536:
                    raise ValueError("proposal exceeded 64 KiB")
                proposal = strict_json(response.read_text())
            else:
                proposal = propose_builtin(best, fields, seen)
                if proposal is None:
                    record.update(status="exhausted", reason="all neighboring recipes evaluated")
                    save_json(directory / "result.json", record)
                    with journal.open("a") as f:
                        f.write(json.dumps(record) + "\n")
                    history.append(record)
                    break
            verify(snapshot, binary, binary_hash)
            save_json(directory / "proposal.json", proposal)
            candidate = validate_proposal(proposal, fields)
            record.update(hypothesis=proposal["hypothesis"], recipe=candidate)
            key = tuple(candidate[f["name"]] for f in fields)
            if key in seen:
                raise ValueError("recipe was already evaluated")
            seen.add(key)
            candidate_file = directory / "recipe.txt"
            candidate_file.write_text(recipe_text(candidate, fields))
            dev = evaluate(binary, candidate_file, "development", directory, args.timeout)
            record.update(development=summary_metrics(dev), examples=examples(dev, incumbent["development"]))
            if better(dev, incumbent["development"], strict=True):
                val = evaluate(binary, candidate_file, "validation", directory, args.timeout)
                record["validation"] = summary_metrics(val)
                keep = better(val, incumbent["validation"], strict=False)
                record["reason"] = "passed both suites" if keep else "validation regression"
                if keep:
                    best, incumbent, incumbent_dir = candidate, {"development": dev, "validation": val}, directory
            else:
                keep = False
                record["reason"] = "development gate did not improve"
            record["status"] = "keep" if keep else "discard"
        except (ValueError, RuntimeError, OSError) as error:
            # No metrics are credited for crashed, malformed or duplicate proposals.
            record["reason"] = str(error)
            record["status"] = "invalid" if isinstance(error, ValueError) else "crash"
        verify(snapshot, binary, binary_hash)
        save_json(directory / "result.json", record)
        with journal.open("a") as f:
            f.write(json.dumps(record, allow_nan=False) + "\n")
        history.append(record)
        print(f"{number:03}: {record['status']} — {record['reason']}", flush=True)
    verify(snapshot, binary, binary_hash)
    winner_file = output / "best-recipe.txt"
    winner_file.write_text(recipe_text(best, fields))
    final = output / "final"
    final.mkdir()
    # The holdout is first read after selection has finished; it never feeds proposals.
    holdout_initial = evaluate(binary, recipe_file, "holdout", baseline_dir, args.timeout)
    holdout_best = evaluate(binary, winner_file, "holdout", final, args.timeout)
    verify(snapshot, binary, binary_hash)
    result = {"baseline_recipe": initial_recipe, "best_recipe": best,
              "experiments": len(history), "kept": sum(r["status"] == "keep" for r in history),
              "baseline": {s: summary_metrics(r) for s, r in baseline.items()},
              "best": {s: summary_metrics(r) for s, r in incumbent.items()},
              "holdout_baseline": summary_metrics(holdout_initial), "holdout_best": summary_metrics(holdout_best),
              "holdout_no_regression": better(holdout_best, holdout_initial, strict=False),
              "winner_artifacts": str(incumbent_dir)}
    save_json(output / "result.json", result)
    rows = ["# Navigation research run", "", f"Experiments: {len(history)}; kept: {result['kept']}.", "",
            "Scores measure navigation targets, not enjoyment. Holdout is reporting only; it did not select the recipe.", "",
            "| Suite | Baseline mean | Selected mean | Baseline worst | Selected worst |",
            "| --- | ---: | ---: | ---: | ---: |"]
    for suite, a, b in [(s, baseline[s], incumbent[s]) for s in baseline] + [("holdout", holdout_initial, holdout_best)]:
        rows.append(f"| {suite} | {a['mean_score']:.2f} | {b['mean_score']:.2f} | {a['worst_score']:.2f} | {b['worst_score']:.2f} |")
    rows += ["", f"Holdout non-regression gate: {result['holdout_no_regression']}.", "",
             f"Selected recipe: [best-recipe.txt](best-recipe.txt). Selected development traces: [{incumbent_dir.name}]({incumbent_dir.name}/development/summary.json).",
             "", "Per-experiment hypotheses, decisions and example differences: [experiments.jsonl](experiments.jsonl)."]
    (output / "report.md").write_text("\n".join(rows) + "\n")
    print(f"Finished. Kept {result['kept']} changes. Holdout non-regression: {result['holdout_no_regression']}. Report: {output / 'report.md'}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="new run directory; existing directories are refused")
    parser.add_argument("--experiments", type=int, default=8)
    parser.add_argument("--timeout", type=int, default=120, help="seconds per process group, including proposer")
    parser.add_argument("--proposer", help="trusted local command; JSON context on stdin, JSON proposal on stdout; no shell")
    args = parser.parse_args()
    if not 1 <= args.experiments <= 128 or not 1 <= args.timeout <= 600:
        parser.error("experiments must be 1..128; timeout must be 1..600")
    try:
        run(args)
    except (OSError, ValueError, RuntimeError) as error:
        parser.exit(2, f"research: {error}\n")


if __name__ == "__main__":
    main()
