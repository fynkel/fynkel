// Layers and blends from Dart (docs/stage-9.md phase 3), on the KayKit
// Knight: a layer from a joint down plays over the base, an additive one
// adds, and a blend mixes its clips by a number. The poses against
// numbers worked out by hand are the engine's tests
// (crates/fynkel_world/tests/blending.rs); these check the way there.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'animator_test.dart' show sameTurn;
import 'model_test.dart' show glb;

const dt = 1 / 60;

void main() {
  late World world;
  late Model knight;

  setUp(() async {
    world = World();
    knight = await Model.load(glb('Knight.glb'));
  });
  tearDown(() {
    world.dispose();
    knight.dispose();
  });

  GameObject copy() => world.add(knight.instantiate());

  // Each part's turn on its parent: the chest under the arm may walk.
  Quat turn(GameObject knight, String part) => knight.find(part)!.localRotation;

  test('a layer from a joint down plays its clip there, over the base', () {
    final walker = copy()..animator!.play('Walking_A');
    final cheerer = copy()..animator!.play('Cheer');
    final both = copy();
    final animator = both.animator!..play('Walking_A');
    final layer = animator.layer(1, mask: 'upperarm.r')..play('Cheer');
    expect((layer.index, layer.mask, layer.additive, layer.strength), (1, 'upperarm.r', false, 1.0));
    expect(identical(animator.layer(1), layer), isTrue);
    for (var i = 0; i < 20; i++) {
      world.update(dt);
    }
    // The arm from the shoulder down cheers; the rest walks.
    for (final part in ['upperarm.r', 'lowerarm.r', 'hand.r']) {
      expect(sameTurn(turn(both, part), turn(cheerer, part)), isTrue, reason: part);
    }
    for (final part in ['upperleg.r', 'chest', 'upperarm.l']) {
      expect(sameTurn(turn(both, part), turn(walker, part)), isTrue, reason: part);
    }
    expect((animator.current, layer.current), ('Walking_A', 'Cheer'));
    expect((animator.weight('Cheer'), layer.weight('Cheer')), (0, 1));

    // None of it showing, the arm walks too.
    layer.strength = 0;
    world.update(dt);
    expect(sameTurn(turn(both, 'upperarm.r'), turn(walker, 'upperarm.r')), isTrue);

    // Stopped with a fade, it's gone when the fade is done.
    layer
      ..strength = 1
      ..stop(seconds: 0.1);
    for (var i = 0; i < 8; i++) {
      world.update(dt);
    }
    expect(layer.current, isNull);
    expect(sameTurn(turn(both, 'upperarm.r'), turn(walker, 'upperarm.r')), isTrue);
  });

  test("an additive layer adds its clip's movement from its first pose", () {
    final walker = copy();
    final animator = walker.animator!..play('Walking_A');
    final still = copy();
    final adding = still.animator!..play('Walking_A');
    final layer = adding.layer(2, additive: true)..play('Cheer');
    for (final a in [animator, adding]) {
      a
        ..speed = 0
        ..time = 0.4;
    }
    // At its first pose, the cheer adds nothing.
    layer.time = 0;
    world.update(dt);
    for (final part in ['upperarm.r', 'chest', 'upperleg.l']) {
      expect(sameTurn(turn(still, part), turn(walker, part)), isTrue, reason: part);
    }
    // Further in, it moves the arm off the walk.
    layer.time = 0.5;
    world.update(dt);
    expect(sameTurn(turn(still, 'upperarm.r'), turn(walker, 'upperarm.r')), isFalse);
  });

  test('a blend mixes its clips by a number, and keeps its pace', () {
    final animator = copy().animator!;
    final lengths = {for (final c in knight.clips) c.name: c.length};
    const clips = ['Idle', 'Walking_A', 'Running_A'];
    const speeds = [0.0, 1.1, 3.0];
    animator.blend('move', clips, thresholds: speeds, at: 0.55, seconds: 0);
    expect(animator.current, 'move');
    expect(animator.weight('Idle'), closeTo(0.5, 1e-6));
    expect(animator.weight('Walking_A'), closeTo(0.5, 1e-6));
    expect(animator.weight('Running_A'), 0);
    expect(animator.length, closeTo((lengths['Idle']! + lengths['Walking_A']!) / 2, 1e-5));
    world.update(0.25);
    expect(animator.time, closeTo(0.25, 1e-5));

    // Asked again each frame, with the number moving: the same blend.
    animator.blend('move', clips, thresholds: speeds, at: 3);
    expect((animator.weight('Running_A'), animator.weight('Walking_A')), (1, 0));
    expect(animator.length, closeTo(lengths['Running_A']!, 1e-5));
    // Thresholds left out: 0, 1, 2.
    animator.blend('steps', const ['Idle', 'Walking_A', 'Running_A'], at: 1.5, seconds: 0);
    expect((animator.current, animator.weight('Walking_A'), animator.weight('Running_A')), ('steps', 0.5, 0.5));

    expect(() => animator.blend('move', const ['Idle'], at: 0), throwsArgumentError, reason: 'other clips');
    expect(() => animator.blend('Idle', const ['Idle'], at: 0), throwsArgumentError, reason: "a clip's name");
    expect(() => animator.blend('new', const [], at: 0), throwsArgumentError);
    expect(() => animator.blend('new', const ['Idle'], thresholds: const [0, 1], at: 0), throwsArgumentError);
    expect(() => animator.blend('new', const ['run'], at: 0), throwsArgumentError);
    expect(() => animator.blend('move', clips, thresholds: speeds, at: double.nan), throwsArgumentError);
  });

  test("what a layer can't be", () {
    final animator = copy().animator!;
    expect(() => animator.layer(0), throwsRangeError, reason: 'the base is the animator');
    expect(() => animator.layer(Animator.maxLayers), throwsRangeError);
    expect(
      () => animator.layer(1, mask: 'tail'),
      throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('no part'))),
    );
    expect(() => animator.layer(1).strength = double.nan, throwsArgumentError);
    expect(() => animator.layer(1).crossFade('Idle', seconds: -1), throwsArgumentError);
    expect(animator.layer(3).current, isNull);
    // Clamped to 0 to 1.
    expect((animator.layer(1)..strength = 3).strength, 1);
  });
}
