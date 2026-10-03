from pathlib import Path
import sys
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(TOOLS))
import foundation_release as gate


class ReleaseGateTest(unittest.TestCase):
    def test_percentiles_preserve_tail_and_reject_invalid_samples(self):
        values = list(range(1, 101))
        self.assertEqual(gate.percentiles(values), {'p50': 50, 'p95': 95, 'p99': 99})
        for bad in ([], [float('nan')], [-1], [float('inf')]):
            with self.assertRaises(ValueError):
                gate.percentiles(bad)

    def test_missing_workloads_cannot_pass_a_regression_gate(self):
        self.assertTrue(gate.compare({}, {}))
        self.assertTrue(gate.compare({'one': {'p99': 10}}, {'two': {'p99': 10}}))
        self.assertTrue(gate.compare({'one': {'p99': 400}}, {'one': {'p99': 100}}))
        self.assertFalse(gate.compare({'one': {'p99': 110}}, {'one': {'p99': 100}}))

    def test_records_require_real_sample_sets_and_convert_units(self):
        logs = 'test starting\n{"entities": 2, "samples_ns": ' + str([1000]*40) + '}\n'
        self.assertEqual(gate.metrics(gate.json_records(logs))['registered/2']['p99'], 1)
        with self.assertRaises(ValueError):
            gate.metrics([{'entities': 2, 'samples_us': [0]}])
        with self.assertRaises(ValueError):
            gate.metrics(gate.json_records(logs+logs))

    def test_tool_progress_and_multiline_records_do_not_hide_measurements(self):
        self.assertEqual(gate.json_records('Running build hooks...{\n"x": 1\n}\nnoise {bad}\n{"y": 2}'), [{'x': 1}, {'y': 2}])
        self.assertIsNone(gate.capture(['/nonexistent/fynkel-qualification-tool'])['exit_code'])

    def test_runner_retains_nonzero_exit_and_timeout(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory)/'run.log'
            result = gate.run_command([sys.executable, '-c', 'print("failed"); raise SystemExit(7)'], TOOLS, log, 5, None)
            self.assertEqual(result['exit_code'], 7)
            self.assertIn('failed', log.read_text())
            result = gate.run_command([sys.executable, '-c', 'import time; time.sleep(10)'], TOOLS, log, 1, None)
            self.assertNotEqual(result['exit_code'], 0)
            self.assertEqual(result['state'], 'timed-out')


if __name__ == '__main__':
    unittest.main()
