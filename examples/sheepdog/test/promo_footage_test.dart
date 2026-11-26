// The promo film's footage of Sheepdog: the dog herds the flock to the pen
// by the keys a player would press, by day and by night, seen by cameras
// of the film's own.
// FYNKEL_FOOTAGE=<dir> flutter test test/promo_footage_test.dart

import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/material.dart' show Brightness;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show WorldTheme;
import 'package:sheepdog/game.dart';
import 'package:sheepdog/palette.dart';

import 'promo_recorder.dart';

Future<Uint8List> files(String asset) async => File('assets/$asset').readAsBytesSync();

final only = Platform.environment['FOOTAGE_ONLY']?.split(',');

/// A player at the keys: holds W A S D toward where the dog should go.
class Herder {
  Herder(this.world);
  final World world;
  final _held = <KeyCode>{};

  void toward(Vec3 from, Vec3 to, {double near = 0.4}) {
    final d = (to - from).flat;
    final want = <KeyCode>{};
    if (d.length > near) {
      final n = d.normalized;
      if (n.x > 0.38) want.add(KeyCode.d);
      if (n.x < -0.38) want.add(KeyCode.a);
      if (n.z < -0.38) want.add(KeyCode.w);
      if (n.z > 0.38) want.add(KeyCode.s);
    }
    for (final k in _held.difference(want)) {
      world.input.simulate.keyUp(k);
    }
    for (final k in want.difference(_held)) {
      world.input.simulate.keyDown(k);
    }
    _held
      ..clear()
      ..addAll(want);
  }
}

void main() {
  late World world;
  late SheepdogGame game;

  Future<void> start({required Brightness light, required int level, double warm = 0.5}) async {
    world = World(source: files)..quality = Quality.high;
    world.input.simulate.resize(1920, 1080);
    world.theme = sheepdogTheme(light);
    game = world.add(SheepdogGame(seed: 3));
    await game.loaded;
    await game.restart(level: level);
    for (var i = 0; i < (warm * 60).round(); i++) {
      world.update(1 / 60);
    }
  }

  tearDown(() => world.dispose());

  Vec3 middle() {
    final free = game.round.free.toList();
    if (free.isEmpty) return const Vec3(0, 0, -6);
    return free.fold(Vec3.zero, (sum, s) => sum + s.position.flat) / free.length.toDouble();
  }

  /// Where the dog goes to push the flock toward the gate: behind it.
  void herd(Herder keys) {
    final dog = game.round.dog;
    const gate = Vec3(0, 0, penFront - 0.5);
    final m = middle();
    final away = (m - gate).flat;
    final behind = m + (away.length > 0.01 ? away.normalized : Vec3.back) * 3.2;
    keys.toward(dog.position, behind);
  }

  Future<void> shot(String name, Future<void> Function() body) async {
    if (only != null && !only!.contains(name)) return;
    await body();
  }

  final timeout = const Timeout(Duration(minutes: 10));

  test('sheepdog_day: a chase behind the dog as it herds', () async {
    await shot('sheepdog_day', () async {
      await start(light: Brightness.light, level: 1);
      final keys = Herder(world), op = Operator(game.camera);
      await record(
        world,
        'sheepdog_day',
        10,
        before: (t) => herd(keys),
        after: (t) {
          final dog = game.round.dog.position;
          final m = middle();
          // Low behind the dog, looking past it to the flock.
          final back = (dog - m).flat;
          final dir = back.length > 0.01 ? back.normalized : Vec3.back;
          op.frame(dog + dir * 4.2 + const Vec3(0, 2.4, 0), lerp(dog, m, 0.6) + const Vec3(0, 0.3, 0), ease: 0.05);
        },
      );
    });
  }, skip: skipFootage, timeout: timeout);

  test('sheepdog_crane: from the grass up over the whole field', () async {
    await shot('sheepdog_crane', () async {
      await start(light: Brightness.light, level: 2, warm: 2);
      final keys = Herder(world);
      await record(
        world,
        'sheepdog_crane',
        9,
        before: (t) => herd(keys),
        after: (t) {
          final k = smooth(0, 9, t);
          final a = (-60 + 50 * k) * math.pi / 180;
          final r = 7 + 15 * k;
          final at = Vec3(math.sin(a) * r, 0.8 + 19 * k, math.cos(a) * r + 2);
          final look = lerp(middle() + const Vec3(0, 0.4, 0), const Vec3(0, 0, 0), k);
          game.camera
            ..position = at
            ..rotation = Quat.lookRotation(look - at);
        },
      );
    });
  }, skip: skipFootage, timeout: timeout);

  test('sheepdog_night: lamps, glowing eyes, a wolf barked away', () async {
    await shot('sheepdog_night', () async {
      await start(light: Brightness.dark, level: 3, warm: calm - 1.5);
      final keys = Herder(world), op = Operator(game.camera);
      var barked = -9.0;
      await record(
        world,
        'sheepdog_night',
        11,
        before: (t) {
          final dog = game.round.dog;
          final wolves = world.all<Wolf>().toList();
          if (wolves.isNotEmpty) {
            wolves.sort((a, b) => (a.position - dog.position).length.compareTo((b.position - dog.position).length));
            final wolf = wolves.first;
            keys.toward(dog.position, wolf.position);
            if ((wolf.position - dog.position).flat.length < Dog.barkReach - 1 && t - barked > 1.2) {
              dog.bark();
              barked = t;
            }
          } else {
            herd(keys);
          }
        },
        after: (t) {
          final dog = game.round.dog.position;
          final a = (200 + 40 * smooth(0, 11, t)) * math.pi / 180;
          op.frame(dog + Vec3(math.sin(a) * 6.5, 3.6, math.cos(a) * 6.5 + 3), dog + const Vec3(0, 0.4, -1.5), ease: 0.05);
        },
      );
    });
  }, skip: skipFootage, timeout: timeout);

  test('sheepdog_pond: low by the water, the grass in the wind', () async {
    await shot('sheepdog_pond', () async {
      await start(light: Brightness.light, level: 1, warm: 1);
      await record(
        world,
        'sheepdog_pond',
        7,
        after: (t) {
          final k = smooth(0, 7, t);
          final a = (120 + 35 * k) * math.pi / 180;
          final at = pond + Vec3(math.sin(a) * 4.6, 1.1 + 0.6 * k, math.cos(a) * 4.6);
          game.camera
            ..position = at
            ..rotation = Quat.lookRotation(pond + const Vec3(1.5, 0, -2) - at);
        },
      );
    });
  }, skip: skipFootage, timeout: timeout);
}
