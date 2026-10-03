import 'support/animation_bindings.dart';

import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-edit-animation-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    lib.dispose();
    Directory(cache).deleteSync(recursive: true);
  });
  World fresh({bool load = false}) {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    if (load) {
      w.onLoadDiagnostics = (_) {};
      expect(w.loadScene('c0000000-0000-4000-8000-000000000034', cacheDir: cache).ok, isTrue);
      w.frame(1 / 60);
    }
    return w;
  }

  test('pending animator controls are logical configuration and later writes win', () {
    final w = fresh();
    final go = w.edit(() {
      final go = w.spawn()..set(const Animator());
      addRigTargets(w, go);
      go.animation.pause();
      go.animation.speed = -2;
      go.animation.weight = 0.5;
      expect(go.get<Animator>()!.playing, isFalse);
      expect(go.get<Animator>()!.speed, -2);
      expect(go.get<Animator>()!.weight, 0.5);
      expect(go.animation.state, isNull);
      return go;
    });
    w.edit(() {
      go.animation.resume();
      go.set(const Animator(playing: false, speed: 3, weight: 0.75));
    });
    w.frame(1 / 60);
    expect(go.get<Animator>()!.playing, isFalse);
    expect(go.get<Animator>()!.speed, 3);
    expect(go.get<Animator>()!.weight, 0.75);
  });

  test('pause and resume preserve completed time at four render rates', () {
    for (final hz in [30, 60, 120, 240]) {
      final w = fresh(load: true);
      final rig = w.find('Rig')!;
      final time = rig.animation.state!.time;
      w.edit(() {
        rig.animation.pause();
        rig.animation.speed = 2;
        expect(rig.get<Animator>()!.playing, isFalse);
        expect(rig.animation.state!.playing, isTrue);
        expect(rig.animation.state!.time, time);
      });
      for (var i = 0; i < hz ~/ 5; i++) {
        w.frame(1 / hz);
      }
      expect(rig.animation.state!.time, time, reason: '$hz Hz paused');
      w.edit(rig.animation.resume);
      expect(rig.animation.state!.time, time);
      for (var i = 0; i < hz ~/ 5; i++) {
        w.frame(1 / hz);
      }
      expect(rig.animation.state!.time, closeTo(time + 0.4, 1e-5), reason: '$hz Hz resumed');
    }
  });

  test('abort and caught invalid or unsupported operations roll back all configuration', () {
    final w = fresh(load: true);
    final rig = w.find('Rig')!;
    final before = rig.get<Animator>();
    expect(
      () => w.edit(() {
        rig.animation.pause();
        rig.animation.weight = 0.25;
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(rig.get<Animator>(), before);
    for (final invalid in <void Function()>[
      () => rig.animation.speed = double.nan,
      () => rig.animation.weight = 2,
      () => rig.animation.play('idle'),
      () => rig.animation.time = -0.5,
    ]) {
      expect(
        () => w.edit(() {
          rig.animation.pause();
          rig.name = 'discard';
          try {
            invalid();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(rig.name, 'Rig');
      expect(rig.get<Animator>(), before);
    }
  });

  const wave = AssetRef('c0000000-0000-4000-8000-000000000005', sub: 'anim/wave');
  const bob = AssetRef('c0000000-0000-4000-8000-000000000005', sub: 'anim/bob');

  test('pending play seek and blend parameters publish once without replaying configuration', () {
    final w = fresh(load: true);
    final go = w.edit(() {
      final go = w.spawn()..set(const Animator());
      addRigTargets(w, go);
      go.animation.play(wave, speed: 2);
      go.animation.time = 0.4;
      go.animation.setParameter('move', 0.75);
      go.animation.pause();
      expect(go.get<Animator>()!.clip, wave);
      expect(go.get<Animator>()!.playing, isFalse);
      expect(go.animation.state, isNull);
      return go;
    });
    w.edit(() => go.set(const Animator(clip: wave, playing: false, speed: 3, weight: 0.25)));
    w.frame(1 / 60);
    expect(go.animation.state!.time, closeTo(0.4, 1e-5));
    expect(go.animation.state!.parameter, 0.75);
    expect(go.animation.state!.speed, 3);
    expect(go.animation.state!.weight, 0.25);
    w.edit(() {
      go.animation.time = 0.8;
      go.animation.stop();
    });
    expect(go.animation.state!.time, closeTo(0.4, 1e-5));
    w.frame(1 / 60);
    expect(go.animation.state!.time, 0);
    expect(go.animation.state!.playing, isFalse);
  });

  test('crossfade preserves outgoing playback and is not restarted at four render rates', () {
    for (final hz in [30, 60, 120, 240]) {
      final w = fresh(load: true);
      final rig = w.find('Rig')!;
      final time = rig.animation.state!.time;
      w.edit(() => rig.animation.crossfade(bob, 0.5, speed: 0));
      expect(rig.get<Animator>()!.clip, bob);
      expect(rig.animation.state!.time, time);
      expect(rig.animation.state!.fadeRemaining, 0);
      for (var i = 0; i < hz ~/ 5; i++) {
        w.frame(1 / hz);
      }
      expect(rig.animation.state!.fadeRemaining, closeTo(0.3, 1e-5), reason: '$hz Hz');
      for (var i = 0; i < hz ~/ 5; i++) {
        w.frame(1 / hz);
      }
      expect(rig.animation.state!.fadeRemaining, closeTo(0.1, 1e-5), reason: '$hz Hz');
    }
  });

  test('failed transport and caught SDK validation publish no playback or data', () {
    final w = fresh(load: true);
    final rig = w.find('Rig')!;
    final before = rig.get<Animator>();
    final time = rig.animation.state!.time;
    for (final invalid in <void Function()>[
      () => rig.animation.crossfade(bob, -1),
      () => rig.animation.play(bob, speed: double.nan),
      () => rig.animation.setParameter('', 1),
      () => rig.animation.play(42),
      () => rig.remove<Animator>(),
    ]) {
      expect(
        () => w.edit(() {
          rig.animation.play(bob);
          rig.name = 'discard';
          try {
            invalid();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(rig.name, 'Rig');
      expect(rig.get<Animator>(), before);
      expect(rig.animation.state!.time, time);
    }
    expect(
      () => w.edit(() {
        rig.animation.crossfade(bob, 0.5);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    w.frame(1 / 60);
    expect(rig.get<Animator>()!.clip, wave);
    expect(rig.animation.state!.fadeRemaining, 0);
  });

  test('caught clip-control mismatch with a controller is sticky', () {
    final w = fresh();
    final go = w.edit(() => w.spawn()..set(const Animator(controller: wave)));
    expect(
      () => w.edit(() {
        go.name = 'discard';
        try {
          go.animation.play(bob);
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(go.name, isNot('discard'));
  });

  test('missing animator and foreign owners fail without publishing pending entities', () {
    final w = fresh(), other = fresh();
    final foreign = other.edit(() => other.spawn()..set(const Animator()));
    for (final foreignOwner in [false, true]) {
      expect(
        () => w.edit(() {
          final pending = w.spawn(name: 'discard');
          try {
            (foreignOwner ? foreign : pending).animation.pause();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(w.find('discard'), isNull);
    }
  });
}
