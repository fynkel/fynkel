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


class MobileWorkloadTests(unittest.TestCase):
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
                if 'copy' in cmd:
                    self.assertIn('--pid', commands[-2])
                    self.assertEqual(commands[-2][-1], '42')
                    path = Path(cmd[cmd.index('--destination') + 1])
                    path.write_text('{"event":"stats"}\n{"event":"stop","reason":"deadline"}\n')
                return capture.subprocess.CompletedProcess(cmd, 0, '', '')

            with patch.object(capture.subprocess, 'run', side_effect=fake_run), patch.object(capture.time, 'sleep'):
                capture.capture('device', 'test.dense', app, Path(temp) / 'result')
            stops = [cmd for cmd in commands if 'terminate' in cmd]
            self.assertEqual(len(stops), 1)
            self.assertEqual(stops[0][-2:], ['--pid', '42'])
            report = json.loads((Path(temp) / 'result/capture.json').read_text())
            self.assertEqual(report['terminatedExactPid'], 42)
            self.assertFalse(report['sustainedQualification'])

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
