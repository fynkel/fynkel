// F8 scene activation/unload and multi-world lifetime qualification.
import 'dart:convert';
import 'dart:io';

import 'package:fynkel/authoring.dart';
import 'package:fynkel/gameplay.dart';
import 'package:fynkel/testing.dart';

void require(bool value, String message) {
  if (!value) throw StateError(message);
}

void main() {
  final project = Directory.systemTemp.createTempSync('fynkel-f8-lifecycle-');
  try {
    final copy = Process.runSync('cp', ['-R', '../../crates/fynkel_document/fixtures/project/.', project.path]);
    require(copy.exitCode == 0, '${copy.stderr}');
    final scene = ProjectWriter(project.path).writeScene(
      'scenes/qualification',
      SceneSpec('Qualification', [
        for (var i = 0; i < 1000; i++)
          ObjectSpec.primitive(
            Primitive.cube,
            name: 'cube$i',
            transform: Pose(position: Vec3(i * 3.0, 0, 0)),
          ),
      ]),
    );
    final cache = cookProject(project.path);
    final records = <Map<String, Object?>>[];
    EntityRef? retired;
    for (var session = 0; session < 10; session++) {
      final world = World.create();
      try {
        final before = world.stats();
        if (retired != null) {
          var refused = false;
          try {
            world.attach(retired);
          } on StateError {
            refused = true;
          }
          require(refused, 'a previous session handle was accepted');
        }
        for (var cycle = 0; cycle < 5; cycle++) {
          final watch = Stopwatch()..start();
          world.loadScene(scene.ref.asset, cacheDir: cache);
          final loadUs = watch.elapsedMicroseconds;
          world.frame(world.fixedDt);
          require(world.stats().liveEntities == 1000, 'incorrect loaded population');
          retired = world.find('cube0')!.entity;
          final loaded = world.stats();
          watch.reset();
          world.unloadScene(scene.ref.asset);
          world.frame(world.fixedDt);
          final unloadUs = watch.elapsedMicroseconds;
          final after = world.stats();
          require(after.liveEntities == 0, 'entities survived scene unload');
          require(after.residencyBytes == 0, 'scene assets survived final scene unload');
          records.add({
            'session': session,
            'cycle': cycle,
            'load_us': loadUs,
            'unload_us': unloadUs,
            'before_native_bytes': before.nativeBytes,
            'loaded_native_bytes': loaded.nativeBytes,
            'after_unload_native_bytes': after.nativeBytes,
            'process_resident_bytes': after.residentBytes,
            'loaded_asset_bytes': loaded.residencyBytes,
            'after_unload_asset_bytes': after.residencyBytes,
          });
        }
      } finally {
        world.dispose();
      }
    }
    print(
      jsonEncode({
        'lifecycle': records,
        'scope': '50 load/unload cycles across 10 world lifetimes; cold first activation then warm cache; synchronous CPU activation, not GPU upload timing; process RSS includes allocator retention',
      }),
    );
  } finally {
    project.deleteSync(recursive: true);
  }
}
