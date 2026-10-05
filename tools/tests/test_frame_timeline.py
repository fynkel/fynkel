import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('frame_analysis', ROOT / 'benchmarks/mobile/analyze_frames.py')
analysis = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analysis)


class FrameTimelineTests(unittest.TestCase):
    def test_correlates_submission_completion_acquisition_and_dropped_cpu_work(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            host = [dict(startUs=100, simulationEndUs=120, renderStartUs=125, endUs=180, steps=1, tick=1, slot=0),
                    dict(startUs=200, simulationEndUs=220, renderStartUs=225, endUs=280, steps=1, tick=2, slot=-1)]
            report = dict(status='diagnostic-complete', captureId='fresh', records=[
                dict(event='start', captureId='fresh'), dict(event='hostFrames', samples=host),
                dict(event='flutterFrames', samples=[])])
            (root/'capture.json').write_text(json.dumps(report))
            rows = [(130,'reserve',1,0), (140,'submit_begin',1,0), (145,'submitted',1,0),
                    (150,'ready',1,0), (155,'notified',1,0), (160,'acquire_call',0,-1),
                    (162,'acquire_begin',1,-1), (170,'acquire_new',1,0), (250,'drop',0,-1)]
            header = 'time_us,event,generation,seq,slot,free,rendering,ready,presented,retiring,a,b,c\n'
            (root/'frames.csv').write_text(header + ''.join(
                f'{t},{event},0,{seq},{slot},0,1,1,1,1,0,0,0\n' for t,event,seq,slot in rows))
            result = analysis.analyze(root)
            self.assertEqual(result['matchedAttempts'], 2)
            self.assertEqual(result['submitToCompletionUs']['p50'], 5)
            self.assertEqual(result['completionToAcquisitionUs']['p50'], 20)
            self.assertEqual(result['acquisitionCallUs']['p50'], 10)
            self.assertEqual(result['renderRequestToAcquisitionUs']['p50'], 45)
            self.assertEqual(result['droppedRenderCpuMs'], .055)
            host[1]['renderStartUs'] = 260
            (root/'capture.json').write_text(json.dumps(report))
            with self.assertRaisesRegex(ValueError, 'no matching'):
                analysis.analyze(root)

    def test_cpu_join_preserves_zero_and_catchup_frames_and_exact_tick_identity(self):
        host = [dict(frame=0, startUs=0, simulationEndUs=20, renderStartUs=25,
                     endUs=40, steps=0, tick=10),
                dict(frame=1, startUs=1_000_000, simulationEndUs=1_000_100,
                     renderStartUs=1_000_110, endUs=1_000_140, steps=2, tick=12)]
        cpu = [dict(ticks=0, nativeUs=2, behavioursUs=15, readbackUs=1),
               dict(ticks=2, nativeUs=70, behavioursUs=25, readbackUs=2)]
        ticks = [dict(frameIndex=1, tick=n, readbackUs=3,
                      systems=[dict(system='step', micros=cost)]) for n,cost in [(10,20),(11,40)]]
        records = [dict(event='cpuFrames',samples=cpu),dict(event='cpuTicks',samples=ticks)]
        result = analysis.correlate_cpu(host,records)
        self.assertEqual(result['total']['ticks'],2)
        self.assertEqual(result['total']['totalsMs']['nativeStepUs'],.06)
        self.assertEqual(result['bySteps']['0']['durations']['nativeStepUs']['p50'],0)
        self.assertEqual(result['bySteps']['2']['durations']['probeUs']['p50'],8)
        self.assertEqual([w['second'] for w in result['windows']],[0,1])
        self.assertIsNone(analysis.correlate_cpu(host,[]))
        host[1]['tick']=13
        with self.assertRaisesRegex(ValueError,'tick sequence'):
            analysis.correlate_cpu(host,records)
        host[1]['tick']=12
        ticks[0]['tick']=9
        with self.assertRaisesRegex(ValueError,'completed host tick range'):
            analysis.correlate_cpu(host,records)
        ticks[0]['tick']=10
        cpu[1]['ticks']=1
        with self.assertRaisesRegex(ValueError,'step count'):
            analysis.correlate_cpu(host,records)
        with self.assertRaisesRegex(ValueError,'batches do not match'):
            analysis.correlate_cpu(host,records[:1])
        cpu[1]['ticks']=2
        ticks[0]['systems'].append(dict(system='step',micros=1))
        with self.assertRaisesRegex(ValueError,'duplicate'):
            analysis.correlate_cpu(host,records)

    def test_prepare_groups_are_complete_and_bound_to_the_host_call(self):
        host = [dict(renderStartUs=100, endUs=200), dict(renderStartUs=300, endUs=400)]
        rows = [dict(time_us=110+i, event=event, a=a, b=b, c=c)
                for i,(event,a,b,c) in enumerate([
                    ('stages', 5, 45, 7), ('prepare_0', 1, 2, 3),
                    ('prepare_1', 4, 5, 6), ('prepare_2', 7, 8, 9)])]
        result = analysis.correlate_prepare(host, rows)
        self.assertEqual(result['frames'], 1)
        self.assertEqual(result['outerRemainderUs']['p50'], 0)
        self.assertEqual(result['phasesUs']['instances']['p50'], 5)
        self.assertAlmostEqual(sum(result['totalsMs'].values()), .045)
        rows[0]['b'] = 50
        self.assertEqual(analysis.correlate_prepare(host, rows)['outerRemainderUs']['p50'], 5)
        rows[0]['b'] = 45
        self.assertIsNone(analysis.correlate_prepare(host, rows[:1]))
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            analysis.correlate_prepare(host, rows[:-1])
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            analysis.correlate_prepare(host, rows + [rows[-1]])
        rows[-1]['time_us'] = 310
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            analysis.correlate_prepare(host, rows)
        rows[-1]['time_us'] = 250
        with self.assertRaisesRegex(ValueError, 'no matching'):
            analysis.correlate_prepare(host, rows)
        rows[-1]['time_us'] = 113
        for invalid in [-1, 100]:
            rows[-1]['c'] = invalid
            with self.assertRaisesRegex(ValueError, 'invalid'):
                analysis.correlate_prepare(host, rows)

    def test_visibility_phases_are_nested_and_required_in_every_new_frame(self):
        host = [dict(renderStartUs=100, endUs=200), dict(renderStartUs=300, endUs=400)]
        rows = [dict(time_us=t+i, event=event, a=a, b=b, c=c)
                for t in [110, 310] for i,(event,a,b,c) in enumerate([
                    ('stages', 5, 45, 7), ('prepare_0', 1, 2, 3),
                    ('prepare_1', 4, 5, 6), ('prepare_2', 7, 8, 9),
                    ('visibility', 1, 1, 1)])]
        result = analysis.correlate_prepare(host, rows)
        self.assertEqual(result['visibilityPhasesUs']['bounds']['count'], 2)
        self.assertEqual(result['visibilityTotalsMs']['queries'], .002)
        for value in [-1, 2]:
            rows[-1]['a'] = value
            with self.assertRaisesRegex(ValueError, 'invalid visibility'):
                analysis.correlate_prepare(host, rows)
        rows[-1]['a'] = 1
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            analysis.correlate_prepare(host, rows[:-1])
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            analysis.correlate_prepare(host, rows + [rows[-1]])
        old_rows = [r for r in rows if r['event'] != 'visibility']
        self.assertIsNone(analysis.correlate_prepare(host, old_rows)['visibilityPhasesUs'])

    def test_visibility_update_classification_matches_its_own_duration(self):
        host = [dict(renderStartUs=100, endUs=200)]
        rows = [dict(time_us=110+i, event=event, a=a, b=b, c=c)
                for i,(event,a,b,c) in enumerate([
                    ('stages', 5, 45, 7), ('prepare_0', 1, 2, 3),
                    ('prepare_1', 4, 5, 6), ('prepare_2', 7, 8, 9),
                    ('visibility', 0, 3, 0), ('visibility_update', 1, 0, 0)])]
        kinds = analysis.correlate_prepare(host, rows)['visibilityUpdateKinds']
        self.assertEqual(kinds['membershipRebuild']['durationsUs']['p50'], 3)
        self.assertEqual(kinds['membershipRebuild']['totalMs'], .003)
        rows[-1]['a'] = 4
        self.assertIn('costRebuild', analysis.correlate_prepare(host, rows)['visibilityUpdateKinds'])
        for kind,name in [(5, 'membershipRepair'), (6, 'depthRebuild')]:
            rows[-1]['a'] = kind
            self.assertIn(name, analysis.correlate_prepare(host, rows)['visibilityUpdateKinds'])
        for kind in [-1, 7]:
            rows[-1]['a'] = kind
            with self.assertRaisesRegex(ValueError, 'unknown visibility'):
                analysis.correlate_prepare(host, rows)
        rows[-1]['a'] = 1
        with self.assertRaisesRegex(ValueError, 'requires visibility'):
            analysis.correlate_prepare(host, [r for r in rows if r['event'] != 'visibility'])
        self.assertIsNone(analysis.correlate_prepare(host, rows[:-1])['visibilityUpdateKinds'])

    def test_rejects_missing_malformed_and_truncated_traces(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)/'trace.csv'
            header = 'time_us,event,generation,seq,slot,free,rendering,ready,presented,retiring,a,b,c\n'
            for content in ['', header, header+'1,bogus,0,0,-1,0,0,0,0,0,0,0,0\n',
                            header+'1,ready,0,0,-1,0,0,0,0,0,0,0\n',
                            header+'1,ready,0,0,-1,0,0,0,0,0,0,0,0\n'*16384]:
                path.write_text(content)
                with self.assertRaises(ValueError):
                    analysis.read_trace(path)

if __name__ == '__main__':
    unittest.main()
