import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

import 'controller_test.dart' as fixture;

void main() {
  late Directory temp;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    temp = Directory.systemTemp.createTempSync('fynkel-controller-clock-');
    final project = Directory('${temp.path}/project');
    fixture.copyTree(Directory('../../crates/fynkel_document/fixtures/project'), project);
    File('${project.path}/assets/rigs/rig.controller.json').writeAsStringSync(fixture.controllerJson);
    cache = '${temp.path}/cache';
    cookProject(project.path, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    lib.dispose();
    temp.deleteSync(recursive: true);
  });

  (World, GameObject, fixture.States) fresh() {
    final world = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(world.dispose);
    world.onLoadDiagnostics = (_) {};
    world.loadScene(fixture.rigScene, cacheDir: cache);
    final rig = world.find('Rig')!;
    final states = rig.addBehaviour(fixture.States());
    rig.set(rig.get<Animator>()!.copyWith(controller: fixture.controller));
    world.frame(1 / 60);
    states.log.clear();
    return (world, rig, states);
  }

  test('native parameter reads preserve zero-tick writes and report trigger consumption at four rates', () {
    for (final hz in [30, 60, 120, 240]) {
      final (world, rig, states) = fresh();
      final animation = rig.animation;
      animation.setFloat('speed', 2);
      expect(animation.getFloat('speed'), 2);
      world.frame(0);
      animation.setFloat('speed', 3);
      animation.setTrigger('jump');
      expect(
        () => world.edit(() {
          animation.setFloat('speed', 9);
          animation.resetTrigger('jump');
          throw StateError('abort');
        }),
        throwsStateError,
      );
      expect(animation.getFloat('speed'), 3);
      expect(animation.getBool('jump'), isTrue);
      final before = world.tick;
      final observedAtFixed = <bool>[];
      for (var i = 0; i < hz ~/ 5; i++) {
        world.frame(
          1 / hz,
          fixedUpdate: (_) {
            observedAtFixed.add(animation.getBool('jump'));
            expect(animation.getFloat('speed'), 3);
          },
        );
        expect(animation.getBool('jump'), world.tick == before, reason: '$hz Hz frame $i');
      }
      expect(observedAtFixed, isNotEmpty);
      expect(observedAtFixed.first, isTrue, reason: '$hz Hz first tick sees the queued trigger');
      expect(observedAtFixed.skip(1), everyElement(isFalse), reason: '$hz Hz trigger consumed only once');
      expect(animation.getFloat('speed'), 3);
      expect(animation.stateOf()!.name, 'Jump');
      expect(states.log.where((event) => event == 'enter Idle → Jump'), hasLength(1));
    }
  });

  test(
    'successive zero-tick commits fold in order and an aborted reset does not clear an earlier trigger',
    () {
      final (world, rig, _) = fresh();
      final animation = rig.animation;
      animation.setTrigger('jump');
      world.frame(0);
      animation.resetTrigger('jump');
      world.frame(0);
      expect(animation.getBool('jump'), isFalse);
      world.frame(1 / 60);
      expect(animation.stateOf()!.name, 'Idle');
      animation.setTrigger('jump');
      expect(
        () => world.edit(() {
          animation.resetTrigger('jump');
          throw StateError('abort');
        }),
        throwsStateError,
      );
      world.frame(0);
      expect(animation.getBool('jump'), isTrue);
      world.frame(2 / 60);
      expect(animation.getBool('jump'), isFalse);
    },
  );
}
