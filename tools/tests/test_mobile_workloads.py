"""Keep scale fixtures real: stable animation targets and honest preflight evidence."""
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch, Mock

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
    def test_effect_profiles_preserve_medium_and_only_change_two_switches(self):
        with tempfile.TemporaryDirectory() as temp:
            project = Path(temp)
            medium = {'msaa': 4, 'shadowResolution': 2048, 'renderScale': 1.0}
            path = project / 'fynkel.project.json'
            path.write_text(json.dumps({'quality': {'default': 'medium', 'profiles': {'medium': medium}}}))
            mobile.add_effect_isolation_profiles(project)
            quality = json.loads(path.read_text())['quality']
            self.assertEqual(quality['default'], 'medium')
            self.assertEqual(quality['profiles']['medium'], medium)
            for name, ao, contact in [('medium-no-ao', False, True), ('medium-no-contact', True, False),
                                       ('medium-no-screen-space', False, False)]:
                self.assertEqual(quality['profiles'][name], {**medium, 'ambientOcclusion': ao, 'contactShadows': contact})

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
            quality_verified = True

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
                    elif cmd[cmd.index('--source') + 1].startswith('Documents/physics-'):
                        path.write_text('world,tick,ran,sync_ns,commands_ns,characters_ns,solver_ns,writeback_ns,events_ns,total_ns,collect_ns,hierarchy_ns,classify_ns,apply_ns,propagation_ns,core_adapter_ns,publication_ns,core_total_ns\n1,1,1,3,0,0,2,0,0,6,1,0,1,1,2,6,1,9\n')
                    elif cmd[cmd.index('--source') + 1].startswith('Documents/frames-'):
                        path.write_text('frame-trace-fixture\n')
                    else:
                        transfer = next(c for c in commands if 'copy' in c and 'to' in c)
                        request = json.loads(Path(transfer[transfer.index('--source') + 1]).read_text())
                        start = {'event': 'start', 'captureId': request['captureId'],
                                 'awaitProfiler': request['awaitProfiler'],
                                 'timeline': request['timeline'], 'frozenScene': request['freezeScene'],
                                 'acquireWaitUs': request['acquireWaitUs'], 'bufferCount': request['bufferCount'],
                                 'resolutionScale': request['resolutionScale'], 'cpuTickReadback': request['cpuTicks'], 'quality': request['quality'],
                                 'pipelined': request['pipelined'], 'frameQueue': request['frameQueue']}
                        path.write_text(json.dumps(start) + '\n' +
                                        (json.dumps({'event': 'profiler-ready', 'captureId': request['captureId']}) + '\n' if request['awaitProfiler'] else '') +
                                        json.dumps({'event': 'stats', 'quality': request['quality'], 'qualityVerified': quality_verified}) + '\n' +
                                        '{"event":"stop","reason":"deadline"}\n')
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
                capture.capture('device', 'test.dense', app, Path(temp) / 'traced', trace_gpu=True, trace_physics=True)
            report = json.loads((Path(temp) / 'traced/capture.json').read_text())
            self.assertEqual(report['screenSpaceSamples'][0]['blurUs'], 30)
            self.assertEqual(report['physicsSamples'][0]['sync_ns'], 3)
            launch = next(c for c in commands if 'launch' in c)
            environment = json.loads(launch[launch.index('--environment-variables') + 1])
            transfer = next(c for c in commands if 'copy' in c and 'to' in c)
            self.assertLess(commands.index(transfer), commands.index(launch))
            request = json.loads(Path(transfer[transfer.index('--source') + 1]).read_text())
            self.assertEqual(request['captureId'], report['captureId'])
            self.assertEqual(environment['FYNKEL_TRACE_PHYSICS_FILE'],
                             f"Documents/physics-{report['captureId']}.csv")
            self.assertNotIn('FYNKEL_CAPTURE_ID', environment)
            self.assertEqual(environment['FYNKEL_TRACE_SCREEN_SPACE_FILE'],
                             f"Documents/screen-space-{report['captureId']}.log")

            commands.clear()
            with patch.object(capture.subprocess, 'run', side_effect=fake_run), patch.object(capture.time, 'sleep'):
                capture.capture('device', 'test.dense', app, Path(temp) / 'frames',
                                trace_frames=True, freeze_scene=True, acquire_wait_us=0,
                                buffer_count=8, resolution_scale=.5, quality='medium-no-ao')
            report = json.loads((Path(temp) / 'frames/capture.json').read_text())
            launch = next(c for c in commands if 'launch' in c)
            environment = json.loads(launch[launch.index('--environment-variables') + 1])
            self.assertEqual(environment, {'FYNKEL_TRACE_FRAME_FILE':
                             f"Documents/frames-{report['captureId']}.csv"})
            self.assertEqual((Path(temp) / 'frames/frames.csv').read_text(), 'frame-trace-fixture\n')
            self.assertTrue(report['records'][0]['timeline'])
            self.assertTrue(report['records'][0]['frozenScene'])
            self.assertEqual(report['records'][0]['bufferCount'], 8)
            self.assertEqual(report['records'][0]['resolutionScale'], .5)
            self.assertEqual(report['records'][0]['quality'], 'medium-no-ao')
            commands.clear()
            for exit_code in [0, 1]:
                commands.clear()
                recorder = Mock(returncode=exit_code)
                recorder.wait.return_value = exit_code
                recorder.poll.return_value = exit_code
                with patch.object(capture.subprocess, 'run', side_effect=fake_run), patch.object(capture.time, 'sleep'), patch.object(capture.subprocess, 'Popen', return_value=recorder) as launch_recorder, patch.object(capture, 'wait_profiler_ready') as ready:
                    path = Path(temp) / f'metal-{exit_code}'
                    if exit_code:
                        with self.assertRaisesRegex(RuntimeError, 'Metal trace failed'):
                            capture.capture('device', 'test.dense', app, path, metal_trace_device='physical-udid')
                    else:
                        capture.capture('device', 'test.dense', app, path, metal_trace_device='physical-udid')
                    command = launch_recorder.call_args.args[0]
                    self.assertEqual(command[command.index('--attach') + 1], '42')
                    self.assertEqual(command[command.index('--device') + 1], 'physical-udid')
                    self.assertEqual(command[command.index('--time-limit') + 1], '8s')
                    self.assertNotIn('--all-processes', command)
                    ready.assert_called_once_with(recorder, path / 'metal-trace.log')
                    release = next(c for c in commands if any(str(v).startswith('Documents/capture-go-') for v in c))
                    request = json.loads((path / 'capture-request.json').read_text())
                    self.assertEqual(release[-1], f"Documents/capture-go-{request['captureId']}.json")
                    self.assertLess(commands.index(release), next(i for i,c in enumerate(commands) if 'terminate' in c))
                    report = json.loads((path / 'capture.json').read_text())
                    self.assertEqual(report['terminatedExactPid'], 42)
                    self.assertEqual(report['status'], 'failed' if exit_code else 'diagnostic-complete')
            commands.clear()
            recorder = Mock()
            recorder.poll.return_value = 21
            recorder.returncode = 21
            with patch.object(capture.subprocess, 'run', side_effect=fake_run), patch.object(capture.subprocess, 'Popen', return_value=recorder), patch.object(capture, 'wait_profiler_ready', side_effect=TimeoutError('attachment timeout')):
                path = Path(temp) / 'metal-start-failed'
                with self.assertRaisesRegex(TimeoutError, 'attachment timeout'):
                    capture.capture('device', 'test.dense', app, path, metal_trace_device='physical-udid')
                self.assertFalse(any(any(str(v).startswith('Documents/capture-go-') for v in c) for c in commands))
                report = json.loads((path / 'capture.json').read_text())
                self.assertEqual(report['status'], 'failed')
                self.assertEqual(report['terminatedExactPid'], 42)
                self.assertEqual(report['metalTraceExitCode'], 21)
            commands.clear()
            quality_verified = False
            with patch.object(capture.subprocess, 'run', side_effect=fake_run), patch.object(capture.time, 'sleep'):
                with self.assertRaisesRegex(RuntimeError, 'quality profile'):
                    capture.capture('device', 'test.dense', app, Path(temp) / 'unverified')
            report = json.loads((Path(temp) / 'unverified/capture.json').read_text())
            self.assertEqual(report['status'], 'failed')
            self.assertEqual(report['terminatedExactPid'], 42)


    def test_profiler_start_requires_live_recorder_and_is_bounded(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / 'recorder.log'
            process = Mock()
            process.poll.return_value = None
            log.write_text('Ctrl-C to stop the recording')
            capture.wait_profiler_ready(process, log)
            process.poll.return_value = 21
            with self.assertRaisesRegex(RuntimeError, 'exited before workload'):
                capture.wait_profiler_ready(process, log)
            process.poll.return_value = None
            log.write_text('Starting recording...')
            with patch.object(capture.time, 'monotonic', side_effect=[0, 31]):
                with self.assertRaisesRegex(TimeoutError, 'did not become ready'):
                    capture.wait_profiler_ready(process, log)
            def attach(_):
                log.write_text('Ctrl-C to stop the recording')
            with patch.object(capture.time, 'sleep', side_effect=attach) as sleep:
                capture.wait_profiler_ready(process, log)
                sleep.assert_called_once_with(0.1)

    def test_profiler_cleanup_is_bounded_and_targets_only_its_child(self):
        process = Mock()
        process.poll.return_value = 0
        capture.stop_profiler(process)
        process.send_signal.assert_not_called()
        process.poll.return_value = None
        process.wait.side_effect = [capture.subprocess.TimeoutExpired('xctrace', 5), 0]
        capture.stop_profiler(process)
        process.send_signal.assert_called_once_with(capture.signal.SIGINT)
        process.kill.assert_called_once_with()
        self.assertEqual(process.wait.call_count, 2)

    def test_capture_rejects_stale_logs_and_incomplete_launches(self):
        rows = [{'event': 'start', 'captureId': 'fresh'}, {'event': 'stats'},
                {'event': 'stop', 'reason': 'deadline'}]
        capture.validate_capture_rows(rows, 'fresh')
        for invalid in [[], rows[1:], rows[:-1], [rows[0], rows[-1]]]:
            with self.assertRaises(RuntimeError):
                capture.validate_capture_rows(invalid, 'fresh')
        with self.assertRaisesRegex(RuntimeError, 'this launch'):
            capture.validate_capture_rows(rows, 'different')

    def test_physics_trace_validates_complete_ordered_nonstale_samples(self):
        header = 'world,tick,ran,sync_ns,commands_ns,characters_ns,solver_ns,writeback_ns,events_ns,total_ns,collect_ns,hierarchy_ns,classify_ns,apply_ns,propagation_ns,core_adapter_ns,publication_ns,core_total_ns\n'
        active = '1,1,1,3,0,0,2,0,0,6,1,0,1,1,2,6,1,9\n'
        skipped = '1,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0\n'
        other = '2,1,1,3,0,0,2,0,0,6,1,0,1,1,2,6,1,9\n'
        self.assertEqual(len(capture.parse_physics(header + active + skipped + other)), 3)
        for invalid in ['', header, header + active[:-1], header + active + active,
                        header + skipped, header + active.replace(',6', ',4'),
                        header + active.replace('2,6,1,9', '2,6,1,8'),
                        header + active.replace('2,6,1,9', '2,5,1,9'),
                        header + active.replace('6,1,0,1,1', '6,2,0,1,1'),
                        header + active + skipped.replace(',2,0,0', ',2,0,1'),
                        header + active.replace('1,1,1', '0,1,1'),
                        header + active.replace('1,1,1', '1,1,2'),
                        header + active.replace(',3,', ',-3,'),
                        header + active.replace(',6', ',6,7'),
                        header + ''.join(f'1,{tick},1,3,0,0,2,0,0,6,1,0,1,1,2,6,1,9\n' for tick in range(1, 8193))]:
            with self.subTest(invalid=invalid[:120]), self.assertRaises(ValueError):
                capture.parse_physics(invalid)

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
