import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('metal_analysis', ROOT / 'benchmarks/mobile/analyze_metal.py')
analysis = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analysis)


def table(columns, rows):
    schema = ''.join(f'<col><mnemonic>{column}</mnemonic></col>' for column in columns)
    return f'<export><schema>{schema}</schema>' + ''.join(f'<row>{row}</row>' for row in rows) + '</export>'


class MetalTimelineTests(unittest.TestCase):
    def fixture(self, directory):
        (directory / 'capture.json').write_text(json.dumps(dict(
            status='diagnostic-complete', metalTraceExitCode=0, pid=42, terminatedExactPid=42, captureId='test')))
        (directory / 'metal-application-encoders-list.xml').write_text(table(
            ['process', 'encoder-id', 'encoder-label'], [
                '<process id="p"><pid>42</pid></process><id>A</id><label fmt="ao"/>',
                '<process ref="p"/><id>B</id><label fmt="blur"/>',
                '<process><pid>99</pid></process><id>C</id><label fmt="unrelated"/>',
            ]))
        def interval(encoder, start, duration, depth=0, state='Active'):
            return f'<id>{encoder}</id><start>{start}</start><duration>{duration}</duration><depth>{depth}</depth><state>{state}</state><channel>Compute</channel>'
        (directory / 'metal-gpu-intervals.xml').write_text(table(
            ['encoder-id', 'start', 'duration', 'event-depth', 'state', 'channel-name'], [
                interval('A', 0, 2_000_000), interval('A', 3_000_000, 1_000_000),
                interval('B', 1_000_000, 2_000_000),
                interval('A', 0, 100_000_000, depth=1),
                interval('A', 0, 100_000_000, state='Waiting'),
                interval('C', 0, 100_000_000), interval('unknown', 0, 100_000_000),
            ]))

    def test_exact_process_nested_intervals_and_overlapping_channels(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.fixture(root)
            result = analysis.analyze(root)
            self.assertEqual(result['matchedEncoders'], 2)
            self.assertEqual(result['activeIntervalUnionMs'], 4)
            self.assertEqual(result['observedSpanMs'], 4)
            self.assertEqual(result['stages'][0]['medianActiveMs'], 3)
            self.assertEqual(result['stages'][1]['medianActiveMs'], 2)
            self.assertEqual(result['stages'][0]['encoders'], 1)

    def test_execution_window_excludes_crossing_encoders_instead_of_clipping(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.fixture(root)
            result = analysis.analyze(root, start_ms=1, end_ms=3)
            self.assertEqual(result['matchedEncoders'], 1)
            self.assertEqual(result['excludedEncoderChannels'], 1)
            self.assertEqual(result['stages'][0]['label'], 'blur')
            self.assertEqual(result['stages'][0]['medianActiveMs'], 2)
            self.assertEqual(result['activeIntervalUnionMs'], 2)
            for start, end in [(float('nan'), 3), (0, float('inf')), (-1, 3), (3, 3), (4, 3)]:
                with self.assertRaises(ValueError):
                    analysis.analyze(root, start, end)
            with self.assertRaisesRegex(ValueError, 'no top-level'):
                analysis.analyze(root, 10, 20)

    def test_rejects_failed_captures_missing_references_and_empty_matches(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.fixture(root)
            report = json.loads((root / 'capture.json').read_text())
            report['metalTraceExitCode'] = 1
            (root / 'capture.json').write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, 'successful Metal'):
                analysis.analyze(root)
            self.fixture(root)
            p = root / 'metal-application-encoders-list.xml'
            p.write_text(p.read_text().replace('ref="p"', 'ref="missing"'))
            with self.assertRaisesRegex(ValueError, 'unresolved'):
                analysis.analyze(root)
            self.fixture(root)
            p.write_text(p.read_text().replace('<pid>42</pid>', '<pid>55</pid>'))
            with self.assertRaisesRegex(ValueError, 'no top-level'):
                analysis.analyze(root)


if __name__ == '__main__':
    unittest.main()
