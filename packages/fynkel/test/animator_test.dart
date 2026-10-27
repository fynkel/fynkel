// A model's clips from Dart (docs/stage-9.md phase 1): what a copy's
// animator plays, the pose it gives the parts after the updates, a
// crossfade, the events, and the game's own turn in onLateUpdate. The
// poses against numbers worked out by hand are the engine's tests
// (crates/fynkel_world/tests/animation.rs); these check the way there.

import 'dart:ffi';

import 'package:ffi/ffi.dart';
import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'model_test.dart' show glb;

/// The truck's wheels clip: 31 keys, 1/24 s apart, 1.25 s.
const second = 1 / 24;

/// The wheels' turn at the second key, as the file has it.
final secondKey = Quat(0, -0.18921537697315216, 0, 0.9819356203079224);

bool sameTurn(Quat a, Quat b) => (a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w).abs() > 1 - 1e-5;

Matcher turnedTo(Quat q) => predicate<Quat>((t) => sameTurn(t, q), 'turned to $q');

void main() {
  late World world;
  late Model truck;

  setUp(() async {
    world = World();
    truck = await Model.load(glb('CesiumMilkTruck.glb'));
  });
  tearDown(() {
    world.dispose();
    truck.dispose();
  });

  test('a model lists its clips, and a copy with clips has an animator once added', () async {
    expect(truck.clips.map((c) => (c.name, c.length)), [('Wheels', 1.25)]);
    final copy = truck.instantiate();
    expect(() => copy.animator, throwsStateError, reason: 'not added yet');
    world.add(copy);
    final animator = copy.animator!;
    expect(identical(copy.animator, animator), isTrue);
    expect(animator.clipNames, ['Wheels']);
    expect(animator.current, isNull);
    expect((animator.time, animator.length, animator.speed), (0, 0, 1));
    // Its parts, plain objects and models without clips have none.
    expect(copy.find('Wheels')!.animator, isNull);
    expect(world.add(GameObject()).animator, isNull);
    final box = await Model.load(glb('Box.glb'));
    addTearDown(box.dispose);
    expect(box.clips, isEmpty);
    expect(world.add(box.instantiate()).animator, isNull);
    expect(
      () => animator.play('run'),
      throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('Wheels'))),
    );
  });

  test('a clip poses the parts it moves after the updates, and a part reads its pose', () {
    final copy = world.add(truck.instantiate());
    final wheels = copy.find('Wheels')!;
    final rest = wheels.transform;
    copy.animator!.play('Wheels');
    // Read in the update: before the clip.
    Quat? inUpdate;
    copy.addComponent(_Call(update: () => inUpdate = wheels.localRotation));
    world.update(second);
    expect(inUpdate, turnedTo(rest.rotation));
    expect(wheels.localRotation, turnedTo(secondKey));
    expect(wheels.rotation, turnedTo(wheels.parent!.rotation * secondKey));
    expect(copy.animator!.time, closeTo(second, 1e-6));
    // A part's child follows it.
    final hub = wheels.add(GameObject(transform: Transform(position: Vec3(1, 0, 0))));
    world.update(second);
    final there = hub.worldTransform.position;
    world.update(second);
    expect(hub.worldTransform.position, isNot(there));
  });

  test("a package's system sees the pose in afterUpdate, and the cameras move after its afterLateUpdate (DC5)", () {
    final copy = world.add(truck.instantiate());
    final wheels = copy.find('Wheels')!;
    final rest = wheels.transform.rotation;
    copy.animator!.play('Wheels');
    final view = world.add(GameObject(components: [Camera()]));
    world.cameras.defaultBlend = Blend.cut;
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 9)),
        components: [VirtualCamera()],
      ),
    );
    // The first a package would sensibly put itself, and the last.
    final seen = <String>[];
    void posed(int order) => seen.add('$order ${sameTurn(wheels.localRotation, rest) ? 'rest' : 'posed'}');
    void late(int order) => seen.add('$order late ${view.position.z}');
    world
      ..system((w) => _Watcher<int>(w, -1 << 30, posed, late))
      ..system((w) => _Watcher<bool>(w, 0, posed, late))
      ..system((w) => _Watcher<String>(w, 1 << 30, posed, late));
    world.update(second);
    expect(seen, [
      '${-1 << 30} posed',
      '0 posed',
      '${1 << 30} posed',
      '${-1 << 30} late 0.0',
      '0 late 0.0',
      '${1 << 30} late 0.0',
    ]);
    expect(view.position.z, 9, reason: 'moved after them');
  });

  test('the game turns a part after the clip, in onLateUpdate', () {
    final copy = world.add(truck.instantiate());
    final wheels = copy.find('Wheels')!;
    final mine = Quat.axisAngle(Vec3(1, 0, 0), 0.5);
    copy.addComponent(_Call(lateUpdate: () => wheels.localRotation = mine));
    copy.animator!.play('Wheels');
    for (var i = 0; i < 3; i++) {
      world.update(second);
      expect(wheels.localRotation, turnedTo(mine));
    }
    // And the engine has it: what a view draws.
    final ids = calloc<Uint64>()..value = wheels.id;
    final out = calloc<Float>(10);
    addTearDown(() => [ids, out].forEach(calloc.free));
    world.readWorldTransforms(ids, 1, out);
    final drawn = Quat(out[3], out[4], out[5], out[6]);
    expect(drawn, turnedTo(wheels.parent!.rotation * mine));
  });

  test('a crossfade mixes two clips, and stop keeps the pose', () {
    final copy = world.add(truck.instantiate());
    final animator = copy.animator!;
    animator.play('Wheels');
    world.update(second);
    animator.crossFade('Wheels', duration: 0.5);
    expect(animator.weight('Wheels'), 1, reason: 'the clip already playing goes on');
    animator.speed = 0.5;
    world.update(second * 2);
    expect(animator.time, closeTo(second * 2, 1e-6));
    expect(animator.isPlaying('Wheels'), isTrue);
    expect(() => animator.crossFade('Wheels', duration: -1), throwsArgumentError);
    expect(() => animator.speed = double.nan, throwsArgumentError);
    animator.time = 9;
    expect(animator.time, 1.25);
    final wheels = copy.find('Wheels')!;
    animator.stop();
    final held = wheels.localRotation;
    world.update(second);
    expect(wheels.localRotation, turnedTo(held));
    expect((animator.current, animator.isPlaying('Wheels')), (null, false));
  });

  test('events at a time come once a pass, and onEnd once at the end', () {
    final copy = world.add(truck.instantiate());
    final animator = copy.animator!;
    final heard = <String>[];
    animator.at('Wheels', 0.5, () => heard.add('half'));
    animator.at('Wheels', 0, () => heard.add('round'));
    animator.play('Wheels');
    for (var i = 0; i < 10; i++) {
      world.update(0.25);
    }
    // 2.5 s: past 0.5 twice, round once at 1.25 (not as it started) and
    // again at 2.5.
    expect(heard, ['half', 'round', 'half', 'round']);
    heard.clear();
    animator.onEnd = (clip) => heard.add('end $clip');
    animator.play('Wheels', loop: false);
    for (var i = 0; i < 10; i++) {
      world.update(0.25);
    }
    expect(heard, ['half', 'end Wheels']);
    expect(animator.time, 1.25);
    expect(() => animator.at('Wheels', 2, () {}), throwsArgumentError);
  });

  // Stage 14's DF9: marks taken away, as a pooled object's must be.
  test('events at a time can be taken away, one or all of a clip', () {
    final copy = world.add(truck.instantiate());
    final animator = copy.animator!;
    var steps = 0, others = 0;
    void step() => steps++;
    // A pooled copy's onSpawn adds its mark each time; its onRelease
    // takes it away.
    for (var reuse = 0; reuse < 3; reuse++) {
      animator.at('Wheels', 0.3, step);
      if (reuse < 2) animator.removeMark('Wheels', 0.3, step);
    }
    animator.at('Wheels', 0.6, () => others++);
    animator.play('Wheels');
    for (var i = 0; i < 5; i++) {
      world.update(0.25);
    }
    expect((steps, others), (1, 1), reason: 'once a pass, not once for each time it was added');
    animator.removeMark('Wheels', 0.6, step);
    animator.removeMark('Wheels', 0.3, () {});
    animator.removeMarks('Wheels');
    for (var i = 0; i < 5; i++) {
      world.update(0.25);
    }
    expect((steps, others), (1, 1));
    expect(() => animator.removeMarks('run'), throwsArgumentError);
    animator.at('Wheels', 0.3, step);
    animator.removeMarks();
    for (var i = 0; i < 5; i++) {
      world.update(0.25);
    }
    expect(steps, 1);
  });

  // Stage 14's WO13: played backwards, an event came only when the clip
  // went round, and then for the span it would have gone through forwards.
  test('events at a time come once a pass played backwards too', () {
    final copy = world.add(truck.instantiate());
    final animator = copy.animator!;
    final heard = <String>[];
    animator.at('Wheels', 0.6, () => heard.add('0.6'));
    animator.at('Wheels', 1.1, () => heard.add('1.1'));
    animator.play('Wheels');
    animator
      ..speed = -1
      ..time = 1;
    // 1 s back to 0.75, 0.5 (past 0.6), 0.25, 0, round to 1 (past 1.1)
    // and 0.75.
    for (var i = 0; i < 6; i++) {
      world.update(0.25);
    }
    expect(heard, ['0.6', '1.1']);
  });

  test('a moved part is in the moved log, so a system following it sees it', () {
    final copy = world.add(truck.instantiate());
    final wheels = copy.find('Wheels')!;
    world.update(second);
    final moved = world.movedReader();
    addTearDown(moved.close);
    world.update(second);
    expect([for (var i = 0; i < moved.length; i++) moved[i]], isNot(contains(wheels)), reason: 'nothing plays');
    copy.animator!.play('Wheels');
    world.update(second);
    expect([for (var i = 0; i < moved.length; i++) moved[i]], contains(wheels));
  });

  test('only animators the engine posed move their parts: not hidden ones, nor ones faded out (DC3, DF3)', () {
    final copy = world.add(truck.instantiate());
    final animator = copy.animator!..play('Wheels');
    world.update(second);
    final moved = world.movedReader();
    addTearDown(moved.close);
    world.update(second);
    expect(moved.length, greaterThan(0), reason: 'playing and shown');

    // Hidden: it keeps time, but poses nothing, so nothing moved.
    copy.active = false;
    world.update(second);
    moved.markRead();
    var version = world.transformVersion;
    world.update(second);
    expect(moved.length, 0, reason: 'hidden');
    expect(world.transformVersion, version, reason: 'hidden');

    // Shown again, it poses again.
    copy.active = true;
    world.update(second);
    moved.markRead();
    world.update(second);
    expect(moved.length, greaterThan(0), reason: 'shown again');

    // Faded out, it's done.
    animator.stop(duration: second);
    for (var i = 0; i < 3; i++) {
      world.update(second);
    }
    moved.markRead();
    version = world.transformVersion;
    world.update(second);
    expect(moved.length, 0, reason: 'faded out');
    expect(world.transformVersion, version, reason: 'faded out');
  });

  test('a destroyed copy takes its animator with it, and many copies play on their own', () {
    final a = world.add(truck.instantiate());
    final b = world.add(truck.instantiate());
    a.animator!.play('Wheels');
    world.update(second);
    b.animator!.play('Wheels');
    world.update(second);
    expect(a.animator!.time, closeTo(second * 2, 1e-6));
    expect(b.animator!.time, closeTo(second, 1e-6));
    final animator = a.animator!;
    a.destroy();
    world.update(second);
    expect(() => animator.play('Wheels'), throwsStateError);
    expect(b.animator!.time, closeTo(second * 2, 1e-6));
  });
}

class _Call extends Behaviour {
  _Call({this.update, this.lateUpdate});

  final void Function()? update;
  final void Function()? lateUpdate;

  @override
  void onUpdate() {
    super.onUpdate();
    update?.call();
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    lateUpdate?.call();
  }
}

// A package's system at [order], calling [after] in its afterUpdate and
// [late] in its afterLateUpdate. A world has one of each type: [T] only
// makes another.
final class _Watcher<T> extends WorldSystem {
  _Watcher(super.world, this._order, this._after, this._late);

  final int _order;
  final void Function(int order) _after, _late;

  @override
  int get order => _order;

  @override
  void afterUpdate() {
    super.afterUpdate();
    _after(_order);
  }

  @override
  void afterLateUpdate() {
    super.afterLateUpdate();
    _late(_order);
  }
}
