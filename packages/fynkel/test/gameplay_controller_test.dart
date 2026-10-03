import 'support/animation_bindings.dart';

import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

import 'controller_test.dart' as fixture;

void main() {
  late Directory temp;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    temp = Directory.systemTemp.createTempSync('fynkel-edit-controller-');
    final project = Directory('${temp.path}/project');
    fixture.copyTree(Directory('../../crates/fynkel_document/fixtures/project').absolute, project);
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
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    expect(w.loadScene(fixture.rigScene, cacheDir: cache).ok, isTrue);
    final rig = w.find('Rig')!;
    final states = rig.addBehaviour(fixture.States());
    rig.set(rig.get<Animator>()!.copyWith(controller: fixture.controller));
    w.frame(1 / 60);
    states.log.clear();
    return (w, rig, states);
  }

  const bob = AssetRef(fixture.rigAsset, sub: 'anim/bob');

  test('state transitions publish once at the next tick, including pending owners', () {
    final (w, rig, states) = fresh();
    w.edit(() {
      rig.animation.play('Jump');
      expect(rig.animation.stateOf()!.name, 'Idle');
      expect(states.log, isEmpty);
    });
    expect(rig.animation.stateOf()!.name, 'Idle');
    w.frame(1 / 60);
    expect(rig.animation.stateOf()!.name, 'Jump');
    final events = List<String>.of(states.log);
    w.frame(1 / 60);
    expect(states.log, events);
    final pending = w.edit(() {
      final go = w.spawn()..set(const Animator(controller: fixture.controller));
      addRigTargets(w, go);
      go.animation.play('Jump', offset: 0.25);
      expect(go.animation.layers, isEmpty);
      return go;
    });
    w.frame(1 / 60);
    expect(pending.animation.stateOf()!.name, 'Jump');
    expect(pending.animation.stateOf()!.normalizedTime, greaterThanOrEqualTo(0.25));
  });

  test('one-shot requests never leak through SDK pending caches on abort', () {
    final (w, rig, _) = fresh();
    expect(
      () => w.edit(() {
        rig.animation.playOneShot(bob);
        expect(rig.animation.isPlayingOneShot(), isFalse);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(rig.animation.isPlayingOneShot(), isFalse);
    w.frame(1 / 60);
    expect(rig.animation.isPlayingOneShot(), isFalse);
    w.edit(() => rig.animation.playOneShot(bob));
    expect(rig.animation.isPlayingOneShot(), isFalse);
    w.frame(1 / 60);
    expect(rig.animation.isPlayingOneShot(), isTrue);
    expect(
      () => w.edit(() {
        rig.animation.stopOneShot();
        expect(rig.animation.isPlayingOneShot(), isTrue);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    w.frame(1 / 60);
    expect(rig.animation.isPlayingOneShot(), isTrue);
    w.edit(() => rig.animation.stopOneShot(blendOut: 0));
    w.frame(1 / 60);
    expect(rig.animation.isPlayingOneShot(), isFalse);
  });

  test('invalid controller requests and parameters reject stickily', () {
    final (w, rig, states) = fresh();
    for (final invalid in <void Function()>[
      () => rig.animation.play('missing'),
      () => rig.animation.play('Jump', layer: -2),
      () => rig.animation.play('Jump', layer: 1 << 40),
      () => rig.animation.crossfade('Jump', -1),
      () => rig.animation.playOneShot(bob, slot: 'missing'),
      () => rig.animation.playOneShot(bob, speed: 0),
      () => rig.animation.setIKTarget('missing', Vec3.zero),
      () => rig.animation.setIKTarget('missing', Vec3.zero, weight: 2),
      () => rig.animation.clearIKTarget('missing'),
      () => rig.animation.setFloat('speed', double.nan),
      () => rig.animation.setInteger('speed', 2),
      () => rig.animation.setInteger('weapon', 1 << 40),
      () => rig.animation.setTrigger('missing'),
    ]) {
      expect(
        () => w.edit(() {
          rig.name = 'discard';
          rig.animation.play('Jump');
          try {
            invalid();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(rig.name, 'Rig');
      expect(rig.animation.stateOf()!.name, 'Idle');
      expect(rig.animation.getFloat('speed'), 1);
    }
    w.frame(1 / 60);
    expect(rig.animation.stateOf()!.name, 'Idle');
    expect(states.log, isEmpty);
  });

  test('typed parameter reads use native staged and committed intents', () {
    final (w, rig, _) = fresh();
    final a = rig.animation;
    w.edit(() {
      expect(a.getFloat('speed'), 1);
      a.setFloat('speed', 2.5);
      a.setInteger('weapon', -2147483648);
      a.setBool('grounded', false);
      a.setTrigger('jump');
      expect(a.getFloat('speed'), 2.5);
      expect(a.getInteger('weapon'), -2147483648);
      expect(a.getBool('grounded'), isFalse);
      expect(a.getBool('jump'), isTrue);
      a.resetTrigger('jump');
      expect(a.getBool('jump'), isFalse);
      expect(a.parameters.singleWhere((p) => p.name == 'speed').value, 1);
    });
    w.edit(() {
      expect(a.getFloat('speed'), 2.5);
      expect(a.getInteger('weapon'), -2147483648);
      a.setInteger('weapon', 2147483647);
    });
    w.frame(1 / 60);
    expect(a.getFloat('speed'), 2.5);
    expect(a.getInteger('weapon'), 2147483647);
    expect(a.getBool('grounded'), isFalse);
    expect(a.stateOf()!.name, 'Idle');
  });

  test('parameter abort does not warm SDK caches or alter runtime', () {
    final (w, rig, _) = fresh();
    final a = rig.animation;
    expect(
      () => w.edit(() {
        a.setFloat('speed', 9);
        a.setTrigger('jump');
        expect(a.getFloat('speed'), 9);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(a.getFloat('speed'), 1);
    w.frame(1 / 60);
    expect(a.getFloat('speed'), 1);
    expect(a.stateOf()!.name, 'Idle');
    expect(
      () => w.edit(() {
        a.setFloat('speed', 9);
        try {
          a.getInteger('speed');
        } catch (_) {}
      }),
      throwsStateError,
    );
    w.frame(1 / 60);
    expect(a.getFloat('speed'), 1);
  });

  test('pending owners read defaults and staged parameters before their first tick', () {
    final (w, _, _) = fresh();
    final go = w.edit(() {
      final go = w.spawn()..set(const Animator(controller: fixture.controller));
      addRigTargets(w, go);
      expect(go.animation.getFloat('speed'), 1);
      go.animation.setFloat('speed', 3);
      expect(go.animation.getFloat('speed'), 3);
      return go;
    });
    w.frame(1 / 60);
    expect(go.animation.getFloat('speed'), 3);
  });

  test('changing the selected controller cannot redirect a staged request', () {
    final (w, rig, _) = fresh();
    const other = AssetRef('c0000000-0000-4000-8000-0000000000ff');
    expect(
      () => w.edit(() {
        rig.animation.play('Jump');
        rig.set(rig.get<Animator>()!.copyWith(controller: other));
      }),
      throwsStateError,
    );
    expect(rig.get<Animator>()!.controller, fixture.controller);
    w.frame(1 / 60);
    expect(rig.animation.stateOf()!.name, 'Idle');
  });

  test('controller crossfade timing is consistent at four render rates', () {
    for (final hz in [30, 60, 120, 240]) {
      final (w, rig, states) = fresh();
      w.edit(() => rig.animation.crossfade('Jump', 0.5));
      expect(states.log, isEmpty);
      for (var i = 0; i < hz ~/ 5; i++) {
        w.frame(1 / hz);
      }
      expect(rig.animation.stateOf()!.transitionProgress, closeTo(0.4, 1e-5), reason: '$hz Hz');
      expect(states.log.where((s) => s.startsWith('enter ')).length, 1);
    }
  });
}
