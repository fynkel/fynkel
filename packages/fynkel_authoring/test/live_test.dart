// Live Play mode's Dart surface (ADR-0021 P7 L1): LiveWorld over a real
// running package:fynkel World — the fixture's main scene loaded from its
// cooked cache, the ids of the document, the components as the document
// stores them, writes that show at once without a tick.

import 'dart:io';

import 'package:fynkel/fynkel.dart' as fynkel;
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:test/test.dart';

import 'session_test.dart' show copyFixture, mainScene, robot, crateA, sun;

void main() {
  test('LiveWorld reads and changes a running world in document terms', () {
    final dir = copyFixture('live');
    addTearDown(() => dir.deleteSync(recursive: true));
    // The cache the game would read: cooked by the editor's first preview.
    final session = AuthoringSession();
    addTearDown(session.dispose);
    session.openProject(dir.path);
    expect(session.openDocument(mainScene).openPreview().cook!.ok, isTrue);

    final world = fynkel.World.create();
    addTearDown(world.dispose);
    world.loadProject(dir.path);
    world.step();
    final live = LiveWorld(world.native.handleAddress);

    final all = live.entities();
    expect(all, hasLength(6));
    expect(all.any((e) => e.spawned), isFalse);
    final robotRow = all.firstWhere((e) => e.id == robot);
    expect(robotRow.name, 'Robot');
    expect(robotRow.types, contains('fynkel.MeshRenderer'));
    expect(live.entity(robot).handle, robotRow.handle);
    expect(world.attach(robotRow.handle).name, 'Robot', reason: "the handle is package:fynkel's entity");
    final crate = all.where((e) => e.id == crateA).single;
    expect(crate.types, contains('fynkel.MeshRenderer'), reason: 'the instance is the prefab\'s root (ADR-0022)');
    final tick = live.tick;
    world.step();
    expect(live.changedEntities(), isNull, reason: 'nothing spawned, renamed or moved in the table');

    final sunNow = live.entity(sun);
    final light = sunNow.component('fynkel.DirectionalLight')!;
    expect(live.set(sun, 'fynkel.DirectionalLight', {...light, 'intensity': 9.5}), isEmpty);
    expect(live.rename(robot, 'Hero'), isEmpty);
    expect(live.entity(sun).component('fynkel.DirectionalLight')!['intensity'], 9.5);
    expect(live.entity(robot).name, 'Hero');
    expect(live.changedEntities()!.firstWhere((e) => e.id == robot).name, 'Hero');
    expect(live.tick, tick + 1, reason: 'a live edit runs no tick');

    // Switching an instance off and on again.
    expect(live.setActive(crateA, false), isEmpty);
    expect(live.entity(crateA).activeSelf, isFalse);
    final off = live.changedEntities()!.firstWhere((e) => e.id == crateA);
    expect(off.activeInHierarchy, isFalse);
    expect(live.setActive(crateA, true), isEmpty);
    expect(live.entity(crateA).activeInHierarchy, isTrue);

    // The world refuses what it would from a game; a bad call throws.
    expect(live.remove(robot, 'fynkel.Camera'), isNotEmpty);
    expect(
      () => live.entity('runtime:1'),
      throwsA(isA<AuthoringException>().having((e) => e.status, 'status', AuthoringStatus.invalidArgument)),
    );
    expect(live.reparent(sun, robot, index: 0), isEmpty);
    expect(live.entity(sun).parent, robot);
    expect(live.destroy(robot), isEmpty);
    expect(live.entities().where((e) => e.id == robot || e.id == sun), isEmpty, reason: 'the subtree went');

    const made = '0b1c2d3e-4f50-4617-8293-a4b5c6d7e8f9';
    final lamp = live.spawn(
      name: 'Lamp',
      authoringId: made,
      components: [
        {
          'type': 'fynkel.Transform',
          'data': {
            'position': [0, 3, 0],
            'rotation': [0, 0, 0, 1],
            'scale': [1, 1, 1],
          },
        },
      ],
    );
    expect(lamp.id, made);
    expect(lamp.refused, isEmpty);
    expect(live.entity(made).name, 'Lamp');
    expect(live.entity(made).component('fynkel.Transform')!['position'], [0, 3, 0]);
    expect(live.spawn(name: 'Loose').id, startsWith('runtime:'));
    expect(() => LiveWorld(0), throwsArgumentError);
    expect(File('${dir.path}/$mainScene').readAsStringSync(), contains('"Robot"'), reason: 'no document is touched');
  });

  test('LiveWorld picks and outlines the running world for the Scene view', () {
    final dir = copyFixture('live-pick');
    addTearDown(() => dir.deleteSync(recursive: true));
    final session = AuthoringSession();
    addTearDown(session.dispose);
    session.openProject(dir.path);
    final preview = session.openDocument(mainScene).openPreview();
    expect(preview.cook!.ok, isTrue);

    final world = fynkel.World.create();
    addTearDown(world.dispose);
    world.loadProject(dir.path);
    world.step();
    final live = LiveWorld(world.native.handleAddress);

    // The robot's centre, as the edit preview would pick it: through its
    // mesh (the world's own cache), from the same ray.
    final m = live.worldMatrices([robot, 'runtime:1'])[robot]!;
    expect(live.worldMatrices(['runtime:1']), isEmpty);
    final centre = [m[12], m[13], m[14]];
    final from = [centre[0], centre[1] + 20, centre[2]];
    final down = [0.0, -1.0, 0.0];
    final hits = live.pickRay(from, down).hits;
    final edits = preview.pickRay(from, down).hits;
    expect([for (final h in hits) h.id], [for (final h in edits) h.id], reason: 'the same picking as the edit preview');
    final hit = hits.firstWhere((h) => h.id == robot);
    expect(hit.mesh, isTrue, reason: 'the mesh came from the cache the world loaded');
    expect(hit.point, edits.firstWhere((h) => h.id == robot).point);

    final outline = live.lines(outline: [robot]);
    expect(outline.count, preview.lines(outline: [robot]).count);
    expect(live.lines(iconsAspect: 1.5).count, greaterThan(0), reason: "the sun's arrows at least");
    expect(live.pickRect(const PreviewCamera(world: _lookingDown), 1, [-1, -1], [1, 1]), contains(robot));
  });
}

/// A camera 50 m up, looking straight down.
const _lookingDown = <double>[1, 0, 0, 0, 0, 0, -1, 0, 0, 1, 0, 0, 0, 50, 0, 1];
