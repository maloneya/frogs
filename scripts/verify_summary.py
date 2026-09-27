"""Bounded diagnostics for verification results; never decides pass/fail or retry policy."""
import json
from pathlib import Path
import re
import shlex

ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
LOCATION = re.compile(r"(?:-->\s+|panicked at |^)([^\s]+\.(?:rs|swift|py)):(\d+)(?::\d+)?")
LIMIT = 1200


def clipped(value):
    value = ANSI.sub("", str(value))
    return value if len(value) <= LIMIT else value[:LIMIT - 3] + "..."


def changed_paths(before, after):
    return sorted(name for name in before.keys() | after.keys() if before.get(name) != after.get(name))


def log_diagnostic(path):
    """Scan once, retaining bounded excerpts rather than returning entire logs."""
    candidates, active, tail = {}, None, []
    scenarios = []
    try:
        with Path(path).open(errors="replace") as stream:
            for number, raw in enumerate(stream, 1):
                line = clipped(raw.rstrip())
                tail = (tail + [line])[-6:]
                category = None
                if re.match(r"error(?:\[E\d+\])?:", line) and not line.startswith(("error: could not compile", "error: test failed", "error: fatalError")):
                    category = "error"
                elif re.search(r"\.swift:\d+:\d+: error:", line):
                    category = "compiler"
                elif line.startswith("FAIL  "):
                    category = "scenario"
                    if len(scenarios) < 10:
                        scenarios.append(line.split(None, 1)[1])
                elif re.search(r"^thread .+ panicked at |^.+: error: -\[|^AssertionError", line):
                    category = "test"
                if category and category not in candidates:
                    active = [number, []]
                    candidates[category] = active
                elif category or line.startswith(("PASS  ", "test result:", "failures:")):
                    active = None
                if active is not None and len(active[1]) < 6:
                    active[1].append(line)
        if "error" in candidates:
            _, lines = candidates["error"]
            if any(LOCATION.search(line) for line in lines) or re.match(r"error\[E\d+\]", lines[0]):
                candidates.setdefault("compiler", candidates.pop("error"))
        for category in ("compiler", "scenario", "test", "error"):
            if category in candidates:
                number, lines = candidates[category]
                diagnostic = clipped("\n".join(lines))
                locations = [m.group(1) for line in lines if (m := LOCATION.search(line))]
                return "unclassified" if category == "error" else category, diagnostic, number, locations, scenarios
        return "unclassified", clipped("\n".join(tail)), max(1, number - len(tail) + 1) if tail else 1, [], scenarios
    except OSError as error:
        return "unclassified", clipped("Cannot read log: " + str(error)), None, [], []


def failure(row, cwd, checked_root=None):
    name = row["name"]
    evidence = [item["path"] for item in row.get("evidence", [])]
    log = evidence[0] if evidence else None
    category, diagnostic, line, paths, scenarios = log_diagnostic(log) if log else ("unclassified", "", None, [], [])
    action = "Inspect the diagnostic and full evidence, fix the cause, then retry."
    retry = "python3 scripts/verify.py --snapshot --only " + shlex.quote(name)
    if category == "compiler":
        action = "Fix the reported compiler/lint error, then retry this check."
    elif category == "scenario":
        paths = [arg for arg in row.get("command", []) if arg.endswith(".ron") and Path(arg).stem in scenarios]
        action = "Compare the expected and actual outcome; fix the cause without blessing or weakening the assertion."
        if paths:
            retry = shlex.join(["cargo", "run", "--quiet", "-p", "scenario", "--", *paths])
    elif category == "test":
        action = "Fix the failing test's cause, then retry this check."
    reason = row.get("reason", "")
    if reason:
        diagnostic = clipped(reason)
        if "inputs changed" in reason:
            category, paths = "source-change", row.get("source_changes", [])
            action = "Capture the updated inputs and retry; this run cannot certify changed source."
        elif "structured evidence" in reason:
            category = "missing-evidence"
            action = "Repair evidence generation, then retry; a zero process exit alone is insufficient."
        else:
            category = "unclassified"
        line = None
    elif name.startswith("performance-"):
        data = {}
        try:
            structured = next(path for path in evidence if path.endswith(".json"))
            data = json.loads(Path(structured).read_text())
            if not isinstance(data, dict):
                data = {}
        except (OSError, ValueError, StopIteration):
            pass
        infrastructure = row.get("infrastructure_failure") or data.get("failure", {})
        assessment = data.get("assessment", {})
        if isinstance(infrastructure, dict) and infrastructure.get("kind") == "infrastructure":
            category = "infrastructure"
            diagnostic = clipped(str(infrastructure.get("stage", "benchmark")) + ": " + str(infrastructure.get("reason", "unknown infrastructure failure")))
            action = "Repair the reported infrastructure problem before explicitly retrying this mode."
        elif isinstance(assessment, dict) and assessment:
            raw_metrics = assessment.get("metrics", [])
            metrics = [m for m in raw_metrics if isinstance(m, dict) and m.get("status") in ("regression", "inconclusive")] if isinstance(raw_metrics, list) else []
            category = "performance-regression" if assessment.get("status") == "regression" else "performance-inconclusive"
            if category != "performance-regression" and any(m.get("stability") == "inconclusive" for m in metrics):
                category = "timing-noise"
            reasons = assessment.get("reasons", [])
            details = reasons[:3] if isinstance(reasons, list) else []
            details += [f"{m.get('workload')}/{m.get('metric')}: {m.get('status')}; trials_ms={m.get('current_ms')}; budget_ms={m.get('budget_ms')}" for m in metrics[:3]]
            diagnostic = clipped("\n".join(map(str, details)) or "Benchmark assessment: " + str(assessment.get("status", "unknown")))
            action = "Investigate the measured regression; preserve the budget." if category == "performance-regression" else "Resolve the measurement uncertainty and rerun this mode with no competing heavy work."
        line = None
        if name == "performance-live":
            action += " Reserve an uninterrupted foreground session for the retry."
    if checked_root is not None:
        paths = [str(Path(path).relative_to(checked_root)) if Path(path).is_absolute() and Path(path).is_relative_to(checked_root)
                 else path for path in paths]
    return {"check": name, "category": category, "diagnostic": diagnostic or f"Process exited {row['exit_code']} without a recognized diagnostic.",
            "affected_paths": paths[:10], "affected_path_count": len(paths), "evidence": evidence,
            "log_line": line, "repeated": bool(row.get("deferred")), "next_step": action,
            "retry_command": retry, "working_directory": str(cwd)}


def runner_failure(reason, cwd, retry, paths=()):
    return {"check": "verification", "category": "source-change" if "inputs changed" in reason else "unclassified",
            "diagnostic": clipped(reason), "affected_paths": list(paths)[:10], "affected_path_count": len(paths),
            "evidence": [], "log_line": None, "repeated": False,
            "next_step": "Resolve the reported condition, then retry from the working directory.",
            "retry_command": retry, "working_directory": str(cwd)}


def display(summary):
    repeated = "; unchanged infrastructure failure, retry deferred" if summary["repeated"] else ""
    print(f"  {summary['check']} [{summary['category']}{repeated}]", flush=True)
    print("    " + summary["diagnostic"].replace("\n", "\n    "), flush=True)
    if summary["affected_paths"]:
        print("    Paths: " + ", ".join(summary["affected_paths"]) + f" ({summary['affected_path_count']} total)", flush=True)
    print("    Next: " + summary["next_step"], flush=True)
    print("    Retry from " + summary["working_directory"] + ": " + summary["retry_command"], flush=True)
    for index, path in enumerate(summary["evidence"][:2]):
        print("    Evidence: " + path + (":" + str(summary["log_line"]) if index == 0 and summary["log_line"] else ""), flush=True)
