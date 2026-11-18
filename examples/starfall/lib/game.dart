import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:fynkel/fynkel.dart';

import 'basket.dart';
import 'falling.dart';
import 'night.dart';
import 'sparkle.dart';

/// Where the game is: the title, playing, or over.
enum Phase { title, playing, over }

/// The game: stars and rocks fall, the basket catches them. A star is a
/// point and a missed star costs a life; a rock caught costs a life.
class StarfallGame extends GameObject {
  StarfallGame({int best = 0, this.seed}) : best = ValueNotifier(best);

  /// For tests: the same falls each time.
  final int? seed;

  final phase = ValueNotifier(Phase.title);
  final score = ValueNotifier(0);
  final lives = ValueNotifier(3);

  /// The best score: the app saves it whenever it changes.
  final ValueNotifier<int> best;

  late final Basket basket;
  late final falling = Pool(this, Falling.new);
  late final sparkles = Pool(this, Sparkle.new);
  late math.Random _random;

  /// The top of the field, where things start, and the bottom, past which
  /// they're missed.
  static const top = 12.0, bottom = -2.0;

  @override
  void onLoad() {
    super.onLoad();
    world.sky = null; // a night: the camera's clear colour, and our own lights
    add(
      GameObject(
        transform: Transform.lookAt(Vec3(0, 5, 15), Vec3(0, 5, 0)),
        components: [Camera(clearColor: const Color(0xFF101830))],
      ),
    );
    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.3, -1, -0.6))),
        components: [Light.sun(intensity: 2)],
      ),
    );
    add(GameObject(components: [Light.ambient(intensity: 0.5)]));
    add(night());
    basket = add(Basket());
    sparkles.prewarm(8);
    falling.prewarm(20);
  }

  /// Starts a new game, from the title or after one is over.
  void start() {
    for (var i = falling.countActive - 1; i >= 0; i--) {
      falling.activeAt(i).release();
    }
    _random = math.Random(seed);
    score.value = 0;
    lives.value = 3;
    basket.position = Vec3.zero;
    phase.value = Phase.playing;
    startRoutine(_drop);
  }

  /// Drops something every so often, faster as the score goes up, until
  /// the game is over.
  Future<void> _drop() async {
    while (phase.value == Phase.playing) {
      final kind = _random.nextDouble() < 0.25 ? Kind.rock : Kind.star;
      final x = (_random.nextDouble() * 2 - 1) * Basket.reach;
      final speed = 4 + score.value * 0.08;
      falling.get((thing) => thing.launch(kind, Vec3(x, top, 0), speed));
      await wait(math.max(0.35, 1.1 - score.value * 0.02));
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (phase.value != Phase.playing) return;
    // Walk from the end: a release moves the last one into its place.
    for (var i = falling.countActive - 1; i >= 0; i--) {
      final thing = falling.activeAt(i);
      final at = thing.position;
      final caught = (at.y - basket.position.y).abs() < 0.4 && (at.x - basket.position.x).abs() < 1.1;
      if (caught) {
        _caught(thing);
      } else if (at.y < bottom) {
        thing.release();
        if (thing.kind == Kind.star) _loseLife();
      }
    }
  }

  void _caught(Falling thing) {
    final star = thing.kind == Kind.star;
    sparkles.get(
      (sparkle) => sparkle
        ..position = thing.position
        ..color = star ? const Color(0xFFFFD45A) : const Color(0xFFFF5A4A),
    );
    thing.release();
    if (star) {
      score.value++;
      basket.punchScale(Vec3(0.3, 0.1, 0.3), seconds: 0.2);
    } else {
      _loseLife();
    }
  }

  void _loseLife() {
    lives.value--;
    if (lives.value > 0) return;
    phase.value = Phase.over;
    if (score.value > best.value) best.value = score.value;
  }
}
