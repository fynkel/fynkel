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
    expect(all, hasLength(7));
    expect(all.any((e) => e.spawned), isFalse);
    expect(all.firstWhere((e) => e.id == robot).name, 'Robot');
    final crate = all.where((e) => e.parent == crateA).single;
    expect(crate.id, isNot(startsWith('runtime:')), reason: 'the id the cook derived for the instance');
    final tick = live.tick;

    final sunNow = live.entity(sun);
    final light = sunNow.component('fynkel.DirectionalLight')!;
    expect(live.set(sun, 'fynkel.DirectionalLight', {...light, 'intensity': 9.5}), isEmpty);
    expect(live.rename(robot, 'Hero'), isEmpty);
    expect(live.entity(sun).component('fynkel.DirectionalLight')!['intensity'], 9.5);
    expect(live.entity(robot).name, 'Hero');
    live.entities();
    expect(live.tick, tick, reason: 'a live edit runs no tick');

    // The world refuses what it would from a game; a bad call throws.
    expect(live.remove(robot, 'fynkel.Camera'), isNotEmpty);
    expect(
      () => live.entity('runtime:1'),
      throwsA(isA<AuthoringException>().having((e) => e.status, 'status', AuthoringStatus.invalidArgument)),
    );
    expect(() => LiveWorld(0), throwsArgumentError);
    expect(File('${dir.path}/$mainScene').readAsStringSync(), contains('"Robot"'), reason: 'no document is touched');
  });
}
