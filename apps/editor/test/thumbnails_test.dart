// The Project grid's thumbnails through the real `fynkel thumbnail`
// (FYNKEL_CLI, the repository's build): one batch for what was asked,
// PNGs where the contract puts them, a texture refused, nothing asked
// twice.
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/env/tools.dart';
import 'package:fynkel_editor/src/project_browser/thumbnails.dart';

import 'project_test.dart' show copyFixture;

const robot = 'c0000000-0000-4000-8000-000000000001';
const rusted = 'c0000000-0000-4000-8000-000000000020';
const crate = 'c0000000-0000-4000-8000-000000000040';

void main() {
  final cli = Platform.environment['FYNKEL_CLI'];

  test(
    'fynkel thumbnail draws a prefab, a mesh part and a material; a texture keeps its file',
    () async {
      final dir = copyFixture('thumbnails');
      addTearDown(() => dir.deleteSync(recursive: true));
      final calls = <List<String>>[];
      final cache = ThumbnailCache(
        projectDir: dir.path,
        cli: () => cli,
        run: (exe, args, {workingDirectory}) {
          calls.add(args);
          return runProcess(exe, args, workingDirectory: workingDirectory);
        },
      );
      addTearDown(cache.dispose);
      var notified = 0;
      cache.addListener(() => notified++);

      final texture = File('${dir.path}/assets/textures/swatch.png.meta.json').readAsStringSync();
      final swatch = RegExp(r'"id": *"([^"]+)"').firstMatch(texture)!.group(1)!;
      final asked = {
        crate: '${dir.path}/assets/prefabs/crate.prefab.json',
        '$robot#mesh/Cube': '${dir.path}/assets/models/robot.glb',
        rusted: '${dir.path}/assets/materials/rusted.material.json',
        swatch: '${dir.path}/assets/textures/swatch.png',
      };
      void ask() {
        for (final MapEntry(:key, :value) in asked.entries) {
          final [id, ...sub] = key.split('#');
          cache.image(id, sub.firstOrNull, value);
        }
      }

      ask();
      final deadline = DateTime.now().add(const Duration(minutes: 3));
      while (cache.busy && DateTime.now().isBefore(deadline)) {
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      expect(cache.busy, isFalse);
      expect(calls, hasLength(1), reason: 'one batch');
      expect(notified, 1);
      for (final name in ['$crate.png', '${robot}__mesh_Cube.png', '$rusted.png']) {
        final png = File('${cache.dir}/$name');
        expect(png.existsSync(), isTrue, reason: name);
        expect(png.readAsBytesSync().sublist(1, 4), 'PNG'.codeUnits);
      }
      expect(File('${cache.dir}/$swatch.png').existsSync(), isFalse, reason: 'no thumbnail for a texture');
      expect(cache.image(crate, null, asked[crate]!), isA<ThumbnailImage>());

      // Asked again: drawn ones are current, the texture was refused.
      ask();
      await Future<void>.delayed(const Duration(milliseconds: 300));
      expect(cache.busy, isFalse);
      expect(calls, hasLength(1));
    },
    skip: cli == null ? 'FYNKEL_CLI is not set' : null,
    timeout: const Timeout(Duration(minutes: 4)),
  );
}
