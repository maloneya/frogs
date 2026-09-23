#!/usr/bin/env python3
"""Adapt the recipe proposal protocol to an installed, authenticated Codex CLI."""
import argparse
import json
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", default="codex", help="Codex CLI executable")
    args = parser.parse_args()
    context = json.load(sys.stdin)
    fields = context["schema"]
    properties = {f["name"]: {"type": "integer", "minimum": f["min"], "maximum": f["max"]} for f in fields}
    schema = {"type": "object", "additionalProperties": False,
              "properties": {"hypothesis": {"type": "string"},
                             "recipe": {"type": "object", "additionalProperties": False,
                                        "properties": properties, "required": list(properties)}},
              "required": ["hypothesis", "recipe"]}
    directory = Path.cwd()
    schema_path = directory / "proposal-schema.json"
    response_path = directory / "codex-response.json"
    schema_path.write_text(json.dumps(schema))
    instructions = Path(__file__).with_name("program.md").read_text()
    prompt = (instructions + "\n\nMake one proposal, then stop. Use read-only inspection of the supplied development "
              "artifacts if useful. Do not run holdout evaluation. The context supplies the fixed "
              "objective and selection rules. Validity and completion are mandatory for credit.\n\nContext:\n" + json.dumps(context))
    # Keep the runner's process group so its timeout also terminates this child.
    command = [args.codex, "exec", "--sandbox", "read-only", "--ephemeral", "--skip-git-repo-check",
               "-c", 'approval_policy="never"', "--cd", str(directory),
               "--output-schema", str(schema_path), "--output-last-message", str(response_path), "-"]
    result = subprocess.run(command, input=prompt, text=True, stdout=sys.stderr, stderr=sys.stderr)
    if result.returncode:
        return result.returncode
    if response_path.stat().st_size > 65536:
        raise ValueError("Codex response exceeded 64 KiB")
    sys.stdout.write(response_path.read_text())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
