"""Keep scale fixtures real: stable animation targets and honest preflight evidence."""
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


gen = module('stress_generator', 'benchmarks/stress/generate.py')
mobile = module('mobile_prepare', 'benchmarks/mobile/prepare.py')
capture = module('mobile_capture', 'benchmarks/mobile/capture.py')
cpu = module('mobile_cpu', 'benchmarks/mobile/analyze_cpu.py')


class MobileWorkloadTests(unittest.TestCase):
    def test_all_tick_probe_accounts_for_first_and_last_catchup_ticks(self):
        frames = [{'ticks': 0, 'elapsedUs': 0, 'nativeUs': 5},
                  {'ticks': 2, 'elapsedUs': 1_000_001, 'nativeUs': 50}]
        ticks = [{'frameIndex': 1, 'tick': i, 'readbackUs': 2,
                  'systems': [{'system': 'step', 'micros': cost}]}
                 for i, cost in [(1, 30), (2, 10)]]
        result = cpu.analyze_ticks(frames, ticks)[0]
        self.assertEqual(result['firstOfMultiTickStepMedianUs'], 30)
        self.assertEqual(result['lastOfMultiTickStepMedianUs'], 10)
        self.assertEqual(result['nativeOutsideStepsPerTickMedianUs'], 5)
        self.assertEqual(result['ticks'], 2)
        with self.assertRaises(ValueError):
            cpu.analyze_ticks(frames, ticks[:1])
        with self.assertRaises(ValueError):
            cpu.analyze_ticks(frames, ticks[::-1])
        with self.assertRaises(ValueError):
            cpu.analyze_ticks(frames, [dict(ticks[0], frameIndex=3)])

    def test_sdk_stages_are_complete_nonnegative_and_separate_from_systems(self):
        frames = [{'ticks': 2, 'elapsedUs': 0, 'nativeUs': 70}]
        ticks = [{'frameIndex': 0, 'tick': i, 'readbackUs': 1,
                  'stagesUs': {'bridgeStep': bridge, 'snapshotIndex': 3},
                  'systems': [{'system': 'step', 'micros': 20}]}
                 for i, bridge in [(1, 25), (2, 35)]]
        result = cpu.analyze_ticks(frames, ticks)[0]
        self.assertEqual(result['sdkStageMediansUs'], {'bridgeStep': 30, 'snapshotIndex': 3})
        self.assertEqual(result['systemMediansUs']['step'], 20)
        self.assertEqual(result['sdkStageMeansUs']['bridgeStep'], 30)
        self.assertEqual(result['bridgeOutsideStepMeanUs'], 10)
        ticks[0]['stagesUs']['snapshotIndex'] = -1
        with self.assertRaisesRegex(ValueError, 'negative SDK stage'):
            cpu.analyze_ticks(frames, ticks)
        del ticks[0]['stagesUs']['snapshotIndex']
        with self.assertRaisesRegex(ValueError, 'incomplete SDK stage'):
            cpu.analyze_ticks(frames, ticks)

    def test_cpu_analysis_distinguishes_catchup_and_last_tick_samples(self):
        def frame(ticks, native, step, elapsed=10):
            return {'ticks': ticks, 'elapsedUs': elapsed, 'nativeUs': native,
                    'behavioursUs': 12, 'readbackUs': 2,
                    'lastTickSystems': [{'system': 'step', 'micros': step}]}
        report = cpu.analyze({'status': 'diagnostic-complete', 'records': [
            {'event': 'cpuFrames', 'samples': [frame(1, 10, 8), frame(3, 60, 16),
                                              frame(0, 100, 999), frame(0, 50, 999, 1_000_000)]}]})
        first, second = report['windows']
        self.assertEqual(first['ticks'], 4)
        self.assertEqual(first['frames'], 3)
        self.assertEqual(first['frameNativePerTickMedianUs'], 15)
        self.assertEqual(first['lastTickSystemMediansUs'], {'step': 12})
        self.assertEqual(second['lastTickSystemMediansUs'], {})
        self.assertIsNone(second['frameNativePerTickMedianUs'])
        self.assertFalse(report['sustainedQualification'])
        with self.assertRaises(ValueError):
            cpu.analyze({'records': []})

    def test_launch_pid_requires_exact_positive_integer(self):
        for bad in [None, 0, -1, True, 'Runner', '42']:
            with self.assertRaises(ValueError):
                capture.launched_pid({'result': {'process': {'processIdentifier': bad}}})
        with self.assertRaises(KeyError):
            capture.launched_pid({'processes': [{'name': 'Runner', 'pid': 123}]})
        self.assertEqual(capture.launched_pid({'result': {'process': {'processIdentifier': 42}}}), 42)

    def test_device_capture_stops_only_launched_pid_before_copy(self):
        with tempfile.TemporaryDirectory() as temp:
            app = Path(temp) / 'Runner.app'
            app.mkdir()
            (app / 'Runner').write_bytes(b'fixture')
            with (app / 'Info.plist').open('wb') as file:
                capture.plistlib.dump({'CFBundleIdentifier': 'test.dense'}, file)
            commands = []

            def fake_run(cmd, **kwargs):
                commands.append(cmd)
                if 'launch' in cmd:
                    path = Path(cmd[cmd.index('--json-output') + 1])
                    path.write_text(json.dumps({'result': {'process': {'processIdentifier': 42}}}))
                if 'copy' in cmd and 'from' in cmd:
                    stops = [c for c in commands[:-1] if 'terminate' in c]
                    self.assertEqual(len(stops), 1)
                    self.assertEqual(stops[0][-2:], ['--pid', '42'])
                    path = Path(cmd[cmd.index('--destination') + 1])
                    if cmd[cmd.index('--source') + 1].startswith('Documents/screen-space-'):
                        path.write_text('screen_space_us sample=8 depth=10 resolve=5 estimate=20 blur=30 total=40\n')
                    else:
                        transfer = next(c for c in commands if 'copy' in c and 'to' in c)
                        request = json.loads(Path(transfer[transfer.index('--source') + 1]).read_text())
                        start = {'event': 'start', 'captureId': request['captureId']}
                        path.write_text(json.dumps(start) + '\n' +
                                        '{"event":"stats"}\n{"event":"stop","reason":"deadline"}\n')
                return capture.subprocess.CompletedProcess(cmd, 0, '', '')

            with patch.object(capture.subprocess, 'run', side_effect=fake_run), patch.object(capture.time, 'sleep'):
                capture.capture('device', 'test.dense', app, Path(temp) / 'result')
            stops = [cmd for cmd in commands if 'terminate' in cmd]
            self.assertEqual(len(stops), 1)
            self.assertEqual(stops[0][-2:], ['--pid', '42'])
            report = json.loads((Path(temp) / 'result/capture.json').read_text())
            self.assertEqual(report['terminatedExactPid'], 42)
            self.assertFalse(report['sustainedQualification'])
            launch = next(c for c in commands if 'launch' in c)
            self.assertNotIn('--environment-variables', launch,
                             'devicectl rejects an empty environment object')
            commands.clear()
            with patch.object(capture.subprocess, 'run', side_effect=fake_run), patch.object(capture.time, 'sleep'):
                capture.capture('device', 'test.dense', app, Path(temp) / 'traced', trace_gpu=True)
            report = json.loads((Path(temp) / 'traced/capture.json').read_text())
            self.assertEqual(report['screenSpaceSamples'][0]['blurUs'], 30)
            launch = next(c for c in commands if 'launch' in c)
            environment = json.loads(launch[launch.index('--environment-variables') + 1])
            transfer = next(c for c in commands if 'copy' in c and 'to' in c)
            self.assertLess(commands.index(transfer), commands.index(launch))
            request = json.loads(Path(transfer[transfer.index('--source') + 1]).read_text())
            self.assertEqual(request['captureId'], report['captureId'])
            self.assertNotIn('FYNKEL_CAPTURE_ID', environment)
            self.assertEqual(environment['FYNKEL_TRACE_SCREEN_SPACE_FILE'],
                             f"Documents/screen-space-{report['captureId']}.log")

    def test_capture_rejects_stale_logs_and_incomplete_launches(self):
        rows = [{'event': 'start', 'captureId': 'fresh'}, {'event': 'stats'},
                {'event': 'stop', 'reason': 'deadline'}]
        capture.validate_capture_rows(rows, 'fresh')
        for invalid in [[], rows[1:], rows[:-1], [rows[0], rows[-1]]]:
            with self.assertRaises(RuntimeError):
                capture.validate_capture_rows(invalid, 'fresh')
        with self.assertRaisesRegex(RuntimeError, 'this launch'):
            capture.validate_capture_rows(rows, 'different')

    def test_gpu_trace_rejects_missing_malformed_and_repeated_samples(self):
        line = 'screen_space_us sample=8 depth=10 resolve=5 estimate=20 blur=30 total=40'
        self.assertEqual(capture.parse_screen_space(line)[0]['totalUs'], 40)
        for text in ['', 'partial', line + '\n' + line, line.replace('depth=10', 'depth=-1')]:
            with self.assertRaises(ValueError):
                capture.parse_screen_space(text)

    def test_rig_bindings_match_bundled_glb_and_survive_display_rename(self):
        blob = (gen.REFERENCE / 'assets/models/character.glb').read_bytes()
        size, kind = struct.unpack_from('<II', blob, 12)
        self.assertEqual(kind, 0x4E4F534A)
        gltf = json.loads(blob[20:20 + size])
        scene = gen.scene_doc(3, 1, 1, 2, 1)
        counts = {}
        for entity in scene['entities'].values():
            components = list(entity['components'].values())
            bindings = [c['data']['key'] for c in components if c['type'] == 'fynkel.AnimationBinding']
            if bindings:
                key = bindings[0]
                asset, _, index = key.partition('/node/')
                self.assertEqual(asset, gen.CHARACTER)
                self.assertEqual(gltf['nodes'][int(index)]['name'], entity['name'])
                entity['name'] = 'renamed'
                self.assertEqual(bindings[0], key)
                counts[key] = counts.get(key, 0) + 1
            for c in components:
                if c['type'] == 'fynkel.RigidBody':
                    self.assertEqual(c['data']['constraints'], 0)
        expected = {f'{gen.CHARACTER}/node/{i}' for i in gltf['skins'][0]['joints']}
        self.assertEqual(set(counts), expected)
        self.assertEqual(set(counts.values()), {2})

    def test_deterministic_populations_include_real_components(self):
        for preset in mobile.PRESETS.values():
            args = [preset[k] for k in ('static', 'bodies', 'behaviours', 'rigs')]
            scene = gen.scene_doc(*args, 1, orbit=True, n_lights=preset['lights'])
            self.assertEqual(scene, gen.scene_doc(*args, 1, orbit=True, n_lights=preset['lights']))
            types = [c['type'] for e in scene['entities'].values() for c in e['components'].values()]
            self.assertEqual(types.count('game.Wobble'), preset['behaviours'])
            self.assertEqual(types.count('fynkel.Skin'), preset['rigs'])
            self.assertEqual(types.count('fynkel.RigidBody'), preset['bodies'])
            self.assertEqual(types.count('fynkel.MeshRenderer'), sum(args) + 1)
            self.assertEqual(types.count('fynkel.Volume'), 1)

    def test_existing_directory_is_never_given_to_destructive_generator(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(mobile.subprocess, 'run') as run:
            with self.assertRaises(ValueError):
                mobile.prepare('dense-500', Path(temp), Path('/unused'), app=True)
            run.assert_not_called()

    def test_failed_stage_is_preserved_and_never_marked_qualified(self):
        with tempfile.TemporaryDirectory() as temp, patch.object(mobile.subprocess, 'run') as run:
            run.return_value = mobile.subprocess.CompletedProcess([], 1, 'output', 'failure')
            out = Path(temp) / 'scene'
            with self.assertRaises(RuntimeError):
                mobile.prepare('dense-500', out, Path('/unused'))
            report = json.loads(out.with_name('scene-preflight.json').read_text())
            self.assertEqual(report['status'], 'failed')
            self.assertEqual(report['deviceQualification'], 'not-run')
            self.assertEqual(report['commands'][0]['stderr'], 'failure')


if __name__ == '__main__':
    unittest.main()
