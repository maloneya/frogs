"""Failure summaries expose useful evidence without changing verification decisions."""
import contextlib
import io
import json
from pathlib import Path
import shlex
import tempfile
import unittest

import verify_summary as summary


class FailureSummaries(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.log = self.root / 'check.log'

    def row(self, text, name='tests', **extra):
        self.log.write_text(text)
        return dict(name=name, exit_code=1, command=[], evidence=[{'path': str(self.log)}], **extra)

    def test_compiler_reports_first_error_and_source_location_not_cargo_footer(self):
        row = self.row('Checking sim\n\x1b[31merror[E0308]: mismatched types\x1b[0m\n'
                       '  --> crates/sim/src/pass/walk.rs:12:4\n  | expected f32\n'
                       'error: could not compile sim\n', 'clippy')
        result = summary.failure(row, self.root)
        self.assertEqual(result['category'], 'compiler')
        self.assertEqual(result['log_line'], 2)
        self.assertEqual(result['affected_paths'], ['crates/sim/src/pass/walk.rs'])
        self.assertIn('mismatched types', result['diagnostic'])
        self.assertNotIn('\x1b', result['diagnostic'])
        self.assertEqual(result['retry_command'], 'python3 scripts/verify.py --snapshot --only clippy')

    def test_swift_error_survives_python_wrapper_traceback(self):
        result = summary.failure(self.row('/tmp/app.swift:16:53: error: missing member\n'
            ' 16 | bad code\nTraceback (most recent call last):\nwrapper failed\n', 'companion'), self.root)
        self.assertEqual(result['category'], 'compiler')
        self.assertEqual(result['affected_paths'], ['/tmp/app.swift'])
        self.assertTrue(result['diagnostic'].startswith('/tmp/app.swift:16:53'))

    def test_failed_scenario_in_middle_of_successes_has_predictions_and_safe_focused_retry(self):
        row = self.row('PASS  before\nFAIL  bad case\n        position\n'
            '          expected: 2\n          actual: 3\nPASS  after\nall done\n', 'scenarios-fast')
        row['command'] = ['cargo', 'run', '--', 'scenarios/before.ron', 'scenarios/bad case.ron', 'scenarios/after.ron']
        result = summary.failure(row, self.root)
        self.assertEqual(result['category'], 'scenario')
        self.assertIn('expected: 2', result['diagnostic'])
        self.assertIn('actual: 3', result['diagnostic'])
        self.assertNotIn('PASS', result['diagnostic'])
        self.assertEqual(shlex.split(result['retry_command'])[-1], 'scenarios/bad case.ron')
        self.assertEqual(result['affected_paths'], ['scenarios/bad case.ron'])

    def test_rust_assertion_is_not_misclassified_by_cargo_test_failed_footer(self):
        row = self.row("test tests::position ... FAILED\nfailures:\n"
            "thread 'tests::position' (12) panicked at crates/sim/src/tests.rs:50:4:\n"
            'assertion left == right failed\n  left: 1\n right: 2\n'
            'test result: FAILED\nerror: test failed, to rerun pass -p sim --lib\n')
        result = summary.failure(row, self.root)
        self.assertEqual(result['category'], 'test')
        self.assertIn('left: 1', result['diagnostic'])
        self.assertEqual(result['affected_paths'], ['crates/sim/src/tests.rs'])

    def test_many_failed_scenarios_keep_total_count_and_retry_entire_suite(self):
        row = self.row(''.join(f'FAIL  case-{i}\n  expected: 1\n  actual: 2\n' for i in range(12)), 'scenarios-fast')
        row['command'] = [f'scenarios/case-{i}.ron' for i in range(12)]
        result = summary.failure(row, self.root)
        self.assertEqual(result['affected_path_count'], 12)
        self.assertEqual(len(result['affected_paths']), 10)
        self.assertEqual(result['retry_command'], 'python3 scripts/verify.py --snapshot --only scenarios-fast')

    def test_drift_overrides_misleading_test_output_and_bounds_paths(self):
        row = self.row('error[E0308]: from changing input\n', reason='inputs changed during check',
                       source_changes=[f'source-{i}.rs' for i in range(30)])
        result = summary.failure(row, self.root)
        self.assertEqual(result['category'], 'source-change')
        self.assertEqual(result['affected_path_count'], 30)
        self.assertEqual(len(result['affected_paths']), 10)
        self.assertIsNone(result['log_line'])
        self.assertEqual(summary.changed_paths({'a': '1', 'b': '2'}, {'b': '3', 'c': '4'}), ['a', 'b', 'c'])

    def performance(self, payload, **extra):
        row = self.row('a misleading generic wrapper error\n', 'performance-live', **extra)
        path = self.root / 'performance.json'
        path.write_text(json.dumps(payload))
        row['evidence'].append({'path': str(path)})
        return summary.failure(row, self.root)

    def test_deferred_infrastructure_preserves_original_reason_and_explicit_retry(self):
        result = self.performance({'failure': {'kind': 'infrastructure', 'stage': 'baseline_preflight', 'reason': 'corrupt archive'}}, deferred=True)
        self.assertEqual(result['category'], 'infrastructure')
        self.assertEqual(result['diagnostic'], 'baseline_preflight: corrupt archive')
        self.assertTrue(result['repeated'])
        self.assertIn('foreground', result['next_step'])
        self.assertIn('--only performance-live', result['retry_command'])

    def test_noise_and_regression_come_from_structured_assessment(self):
        metric = dict(workload='combat', metric='mean_step_ms', current_ms=[1, 2, 3], budget_ms=2,
                      stability='inconclusive', status='inconclusive')
        assessment = dict(status='inconclusive', metrics=[metric], reasons=[])
        result = self.performance({'assessment': assessment})
        self.assertEqual(result['category'], 'timing-noise')
        self.assertIn('trials_ms=[1, 2, 3]', result['diagnostic'])
        assessment.update(status='regression')
        metric.update(stability='stable', status='regression')
        result = self.performance({'assessment': assessment})
        self.assertEqual(result['category'], 'performance-regression')
        self.assertIn('Preserve the budget', result['next_step'])
        self.assertIn('docs/performance-follow-ups.md', result['next_step'])
        self.assertIn('continue the assigned task', result['next_step'])

    def test_mixed_coverage_prioritizes_confirmed_failure_over_noise_and_reasons(self):
        noise = dict(workload='sparse', metric='mean_step_ms', status='inconclusive', stability='inconclusive')
        breach = dict(workload='combat', metric='frame_ms.p99', status='inconclusive',
                      absolute='regression', relative='incompatible', current_ms=[29, 29, 29], budget_ms=25)
        result = self.performance({'assessment': dict(status='inconclusive', metrics=[noise, breach],
            reasons=['baseline missing', 'other workload unfocused'])})
        self.assertEqual(result['category'], 'performance-regression')
        self.assertTrue(result['diagnostic'].startswith('Performance observation: confirmed performance failure'))
        self.assertLess(result['diagnostic'].index('combat/frame_ms.p99'), result['diagnostic'].index('sparse/mean_step_ms'))
        self.assertIn("user's explicit scope", result['next_step'])
        self.assertNotIn('Resolve the measurement uncertainty', result['next_step'])

    def test_late_archive_failure_does_not_hide_measured_regression(self):
        breach = dict(workload='combat', metric='frame_ms.p99', status='regression',
                      absolute='regression', current_ms=[29, 29, 29], budget_ms=25)
        result = self.performance({'failure': dict(kind='infrastructure', stage='archive', reason='store unavailable'),
            'assessment': dict(status='inconclusive', metrics=[breach], reasons=[])})
        self.assertEqual(result['category'], 'performance-regression')
        self.assertIn('store unavailable', result['diagnostic'])

    def test_unknown_empty_missing_and_malformed_evidence_remain_readable(self):
        for text in ('', 'unknown failure\n' + 'x' * 5000):
            result = summary.failure(self.row(text), self.root)
            self.assertEqual(result['category'], 'unclassified')
            self.assertLessEqual(len(result['diagnostic']), summary.LIMIT)
        self.log.unlink()
        result = summary.failure(dict(name='tests', exit_code=2, evidence=[{'path': str(self.log)}]), self.root)
        self.assertIn('Cannot read log', result['diagnostic'])
        for payload in (None, [], {'assessment': None}, {'assessment': {'metrics': None, 'reasons': None}}):
            self.performance(payload)

    def test_display_contains_action_retry_and_evidence_without_another_log_read(self):
        result = summary.failure(self.row('FAIL  bad\n  expected: 1\n  actual: 2\n', 'scenarios-fast'), self.root)
        with contextlib.redirect_stdout(io.StringIO()) as output:
            summary.display(result)
        for expected in ['scenarios-fast [scenario]', 'expected: 1', 'actual: 2', 'Retry from', str(self.log) + ':1']:
            self.assertIn(expected, output.getvalue())


if __name__ == '__main__':
    unittest.main()
