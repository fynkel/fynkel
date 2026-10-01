import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

import 'controller_test.dart' as fixture;

void main() {
  late Directory temp;
  late String cache;
  late NativeLibrary lib;
  const bob = AssetRef(fixture.rigAsset, sub: 'anim/bob');
  setUpAll(() {
    temp = Directory.systemTemp.createTempSync('fynkel-playback-state-');
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
  (World, GameObject) fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    expect(w.loadScene(fixture.rigScene, cacheDir: cache).ok, isTrue);
    final rig = w.find('Rig')!;
    rig.set(rig.get<Animator>()!.copyWith(controller: fixture.controller));
    w.frame(1 / 60);
    return (w, rig);
  }

  void expectPlaying(GameObject rig, bool playing) {
    expect(rig.animation.isPlayingOneShot(), playing);
    expect(rig.animation.isPlayingOneShot(slot: 'Base Layer'), playing);
  }

  test('ordinary one-shot requests report completed playback for both base slot aliases', () {
    for (final slot in ['', 'Base Layer']) {
      final (w, rig) = fresh();
      rig.animation.playOneShot(bob, slot: slot);
      expectPlaying(rig, false);
      w.edit(() => expectPlaying(rig, false));
      expect(w.frame(0).steps, 0);
      expectPlaying(rig, false);
      w.frame(1 / 60);
      expectPlaying(rig, true);
      rig.animation.stopOneShot(slot: slot, blendOut: 0.1);
      expectPlaying(rig, true);
      w.edit(() => expectPlaying(rig, true));
      w.frame(0);
      expectPlaying(rig, true);
      w.frame(1 / 60);
      expectPlaying(rig, false);
      w.frame(4 / 60);
      expectPlaying(rig, false);
    }
  });

  test('aborted play and stop leave playback unchanged and successful requests execute once', () {
    final (w, rig) = fresh();
    expect(
      () => w.edit(() {
        rig.animation.playOneShot(bob);
        throw StateError('abort play');
      }),
      throwsStateError,
    );
    w.frame(3 / 60);
    expectPlaying(rig, false);
    rig.animation.playOneShot(bob, blendIn: 0, blendOut: 0);
    w.frame(2 / 60);
    final before = rig.animation.stateOf()!.oneShotTime;
    expectPlaying(rig, true);
    expect(
      () => w.edit(() {
        rig.animation.stopOneShot(blendOut: 0);
        throw StateError('abort stop');
      }),
      throwsStateError,
    );
    w.frame(0);
    expectPlaying(rig, true);
    w.frame(3 / 60);
    expectPlaying(rig, true);
    expect(
      rig.animation.stateOf()!.oneShotTime,
      closeTo(before + (3 / 60) / 2, 1e-5),
      reason: 'no replay restarts the clip',
    );
    rig.animation.stopOneShot(blendOut: 0);
    w.frame(2 / 60);
    expectPlaying(rig, false);
  });

  test('ordinary and explicit requests use the same validation without optimistic playback', () {
    final (w, rig) = fresh();
    for (final request in <void Function()>[
      () => rig.animation.playOneShot(bob, slot: 'missing'),
      () => rig.animation.playOneShot(bob, speed: 0),
      () => rig.animation.stopOneShot(blendOut: -1),
    ]) {
      expect(request, throwsStateError);
      expectPlaying(rig, false);
      expect(
        () => w.edit(() {
          rig.name = 'discarded';
          try {
            request();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(rig.name, 'Rig');
    }
    w.frame(3 / 60);
    expectPlaying(rig, false);
  });
}
