"""Selection, protocol, process budgets and a real end-to-end proposer run."""
import importlib.util
import json
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("research", ROOT / "research.py")
research = importlib.util.module_from_spec(spec)
spec.loader.exec_module(research)


class ResearchTests(unittest.TestCase):
    def test_selection_refuses_average_gains_with_regressed_failures_or_tail(self):
        base = dict(mean_score=500, worst_score=100, failed_maps=0, incomplete_plays=0)
        self.assertTrue(research.better(dict(base, mean_score=501), base, True))
        self.assertFalse(research.better(base, base, True))
        self.assertTrue(research.better(base, base, False))
        for change in [dict(worst_score=99), dict(failed_maps=1), dict(incomplete_plays=1)]:
            self.assertFalse(research.better(dict(base, mean_score=900, **change), base, True))

    def test_proposals_cannot_change_suite_rules_or_use_non_integer_values(self):
        fields = [dict(name="rock_percent", min=30, max=60)]
        for recipe in [dict(rock_percent=31, min_route=0), dict(rock_percent=True),
                       dict(rock_percent=31.0), dict(rock_percent=999), {}]:
            with self.assertRaises(ValueError):
                research.validate_proposal(dict(hypothesis="test", recipe=recipe), fields)
        for text in ['{"a":1,"a":2}', '{"a":NaN}']:
            with self.assertRaises(ValueError):
                research.strict_json(text)

    def test_source_or_binary_mutation_aborts(self):
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "binary"
            binary.write_text("frozen")
            with mock.patch.object(research, "protected_files", return_value={"a": "changed"}):
                with self.assertRaisesRegex(RuntimeError, "protected"):
                    research.verify({"a": "original"}, binary, research.digest(binary))
            with mock.patch.object(research, "protected_files", return_value={}):
                with self.assertRaisesRegex(RuntimeError, "protected"):
                    research.verify({}, binary, "wrong hash")

    def test_process_timeout_is_enforced(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                research.logged([sys.executable, "-c", "import time; time.sleep(20)"],
                                Path(directory), "slow", 0.05)

    def test_proposer_run_records_evidence_and_reserves_holdout_until_end(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "run"
            proposer = shlex.join([sys.executable, str(ROOT / "tests" / "proposer_fixture.py")])
            result = subprocess.run([sys.executable, str(ROOT / "research.py"), "--output", str(output),
                                     "--experiments", "1", "--proposer", proposer],
                                    capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            summary = json.loads((output / "result.json").read_text())
            self.assertEqual(summary["experiments"], 1)
            context = json.loads((output / "experiment-001" / "context.json").read_text())
            self.assertNotIn("holdout", context["incumbent_reports"])
            self.assertTrue((output / "final" / "holdout" / "summary.json").is_file())
            manifest = json.loads((output / "manifest.json").read_text())
            self.assertEqual(manifest["binary_sha256"], research.digest(output / "evaluator"))
            self.assertTrue(list((output / "baseline" / "development").glob("*.trace.tsv")))
            # Predict dense, heavily smoothed caves will violate area requirements.
            # They must stay in the denominator and receive zero, not disappear.
            invalid_recipe = output / "dense.recipe"
            invalid_recipe.write_text("rock_percent=60\nsmoothing_passes=8\ncarve_percent=20\nstraight_percent=90\n")
            failed_suite = output / "failure-check"
            evaluation = subprocess.run([str(output / "evaluator"), "evaluate", "--suite", "development",
                                         "--recipe", str(invalid_recipe), "--output", str(failed_suite)],
                                        capture_output=True, text=True, timeout=30)
            self.assertEqual(evaluation.returncode, 0, evaluation.stderr)
            report = json.loads((failed_suite / "summary.json").read_text())
            self.assertEqual(report["cases"], 16)
            self.assertGreater(report["failed_maps"], 0)
            self.assertEqual(report["failed_maps"], sum(not c["accepted"] for c in report["results"]))
            self.assertTrue(all(c["score"] == 0 for c in report["results"] if not c["accepted"]))
            self.assertAlmostEqual(report["mean_score"], sum(c["score"] for c in report["results"]) / 16, places=5)
            # Refusing a reused directory preserves its existing record.
            before = (output / "result.json").read_bytes()
            result = subprocess.run([sys.executable, str(ROOT / "research.py"), "--output", str(output)],
                                    capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(before, (output / "result.json").read_bytes())

    def test_codex_adapter_uses_structured_read_only_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            fake = directory / "codex"
            fake.write_text("#!" + sys.executable + "\n" + '''import json,sys
from pathlib import Path
args=sys.argv[1:]
assert args[args.index('--sandbox')+1]=='read-only'
assert '--ephemeral' in args
schema=json.loads(Path(args[args.index('--output-schema')+1]).read_text())
assert schema['additionalProperties'] is False
assert 'Context:' in sys.stdin.read()
Path(args[args.index('--output-last-message')+1]).write_text('{"hypothesis":"test","recipe":{"rock_percent":48}}')
''')
            fake.chmod(0o755)
            context = dict(schema=[dict(name="rock_percent", min=30, max=60)])
            result = subprocess.run([sys.executable, str(ROOT / "codex_proposer.py"), "--codex", str(fake)],
                                    input=json.dumps(context), text=True, cwd=directory,
                                    capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)["recipe"], {"rock_percent": 48})


if __name__ == "__main__":
    unittest.main()
