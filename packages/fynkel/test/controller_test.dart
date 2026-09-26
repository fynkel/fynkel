// The Animator Controller from Dart (ADR-0032 step 4, docs/animation.md):
// an Animator that names a controller runs its states; `animation`
// sets typed parameters by name (read-your-writes), plays and crossfades
// states, reads each layer's state and the parameters back; behaviours
// hear `onAnimatorStateEntered`/`Exited`; a wrong name or type throws with
// the names there, and the clip calls refuse a controlled animator.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

const rigScene = 'c0000000-0000-4000-8000-000000000034';
const rigAsset = 'c0000000-0000-4000-8000-000000000005';
const controllerId = 'c0000000-0000-4000-8000-000000000052';
const controller = AssetRef(controllerId);

/// Idle (the rig's `wave`, looping 1 s) and Jump (`bob`, once, 2 s):
/// `jump` fades into Jump over 0.1 s, Jump returns at its end when
/// `grounded`.
const controllerJson = '''
{
  "\$schema": "https://fynkel.dev/schemas/controller.schema.json",
  "formatVersion": 1,
  "id": "$controllerId",
  "name": "Rig",
  "parameters": [
    { "name": "speed", "type": "float", "default": 1 },
    { "name": "weapon", "type": "int" },
    { "name": "grounded", "type": "bool", "default": true },
    { "name": "jump", "type": "trigger" }
  ],
  "layers": [{
    "name": "Base Layer",
    "defaultState": "Idle",
    "states": [
      { "name": "Idle", "motion": { "asset": "$rigAsset", "sub": "anim/wave" }, "speedParameter": "speed" },
      { "name": "Jump", "motion": { "asset": "$rigAsset", "sub": "anim/bob" }, "loop": false }
    ],
    "transitions": [
      { "from": "Idle", "to": "Jump", "duration": 0.1, "conditions": [{ "parameter": "jump" }] },
      { "from": "Jump", "to": "Idle", "exitTime": 1, "duration": 0, "conditions": [{ "parameter": "grounded", "is": true }] }
    ]
  }]
}
''';

/// Logs its object's state events in order.
class States extends Behaviour {
  final List<String> log = [];

  @override
  void onAnimatorStateEntered(AnimatorStateEntered event) {
    expect((event.entity, event.layer), (gameObject.entity, 0));
    log.add('enter ${event.from} → ${event.state}');
  }

  @override
  void onAnimatorStateExited(AnimatorStateExited event) {
    expect(event.entity, gameObject.entity);
    log.add('exit ${event.state}');
  }

  @override
  void onAnimationEnded(AnimationEnded event) {
    expect(event.entity, gameObject.entity);
    log.add('ended ${event.clip.sub ?? event.clip.asset}');
  }

  @override
  void onAnimationMarker(AnimationMarker event) => log.add('marker ${event.name}');
}

void copyTree(Directory from, Directory to) {
  to.createSync(recursive: true);
  for (final e in from.listSync()) {
    final name = e.uri.pathSegments.lastWhere((s) => s.isNotEmpty);
    if (name == '.fynkel') continue;
    if (e is Directory) {
      copyTree(e, Directory('${to.path}/$name'));
    } else if (e is File) {
      e.copySync('${to.path}/$name');
    }
  }
}

void main() {
  late Directory temp;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    temp = Directory.systemTemp.createTempSync('fynkel-controller-');
    final project = Directory('${temp.path}/project');
    copyTree(Directory('../../crates/fynkel_document/fixtures/project').absolute, project);
    File('${project.path}/assets/rigs/rig.controller.json').writeAsStringSync(controllerJson);
    cache = '${temp.path}/cache';
    cookProject(project.path, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    temp.deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    w.onLoadDiagnostics = (_) {};
    addTearDown(w.dispose);
    return w;
  }

  /// The Rig scene with its Animator running the controller.
  (World, GameObject, States) withController() {
    final w = fresh();
    expect(w.loadScene(rigScene, cacheDir: cache).ok, isTrue);
    final rig = w.find('Rig')!;
    final states = rig.addBehaviour(States());
    rig.set(rig.get<Animator>()!.copyWith(controller: controller));
    w.frame(1 / 60);
    return (w, rig, states);
  }

  void ticks(World w, int n) {
    for (var i = 0; i < n; i++) {
      w.frame(1 / 60);
    }
  }

  test('a whole jump: a trigger, a crossfade, the return at the end when grounded', () {
    final (w, rig, states) = withController();
    expect(states.log, ['enter  → Idle']);
    var s = rig.animation.stateOf()!;
    expect((s.layer, s.name, s.isInTransition, s.length), (0, 'Idle', false, 1.0));
    ticks(w, 30);
    expect(rig.animation.stateOf()!.normalizedTime, closeTo(0.5, 1e-3));

    rig.animation.setTrigger('jump');
    expect(rig.animation.getBool('jump'), isTrue, reason: 'read-your-writes');
    w.frame(1 / 60);
    expect(states.log.last, 'enter Idle → Jump');
    expect(rig.animation.getBool('jump'), isFalse, reason: 'the transition used it up');
    ticks(w, 2);
    s = rig.animation.stateOf()!;
    // Unity's view mid-fade: the state fading out, the next fading in.
    expect((s.name, s.next, s.isInTransition), ('Idle', 'Jump', true));
    expect(s.transitionProgress, inExclusiveRange(0, 1));
    ticks(w, 8);
    expect(states.log.last, 'exit Idle');
    expect(rig.animation.stateOf()!.name, 'Jump');

    // In the air: Jump holds its end until grounded.
    rig.animation.setBool('grounded', false);
    ticks(w, 150);
    s = rig.animation.stateOf()!;
    expect(s.name, 'Jump');
    expect(s.normalizedTime, greaterThanOrEqualTo(1));
    rig.animation.setBool('grounded', true);
    w.frame(1 / 60);
    expect(states.log.sublist(states.log.length - 2), ['exit Jump', 'enter Jump → Idle']);
    expect(rig.animation.stateOf()!.name, 'Idle');
  });

  test('parameters by name and type; states by name; the clip calls refuse a controller', () {
    final (w, rig, states) = withController();
    final a = rig.animation;
    expect([for (final p in a.parameters) '${p.name} ${p.kind.name} ${p.value}'],
        ['speed float 1.0', 'weapon int 0', 'grounded bool true', 'jump trigger false']);

    a.setFloat('speed', 2);
    a.setInteger('weapon', 3);
    expect((a.getFloat('speed'), a.getInteger('weapon')), (2.0, 3));
    w.frame(1 / 60);
    expect((a.getFloat('speed'), a.getInteger('weapon')), (2.0, 3), reason: 'the tick applied them');
    // The speed parameter scales Idle: 2 × 30 ticks over 1 s.
    final before = a.stateOf()!.normalizedTime;
    ticks(w, 30);
    expect(a.stateOf()!.normalizedTime - before, closeTo(1, 1e-3));

    expect(
        () => a.setFloat('run', 1),
        throwsA(isA<ArgumentError>().having(
            (e) => e.message, 'message', contains('the parameters: speed, weapon, grounded, jump'))));
    expect(() => a.setInteger('speed', 1),
        throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('`speed` is a float'))));
    expect(() => a.getBool('weapon'), throwsArgumentError);
    expect(() => a.play(const AssetRef(rigAsset, sub: 'anim/wave')),
        throwsA(isA<StateError>().having((e) => e.message, 'message', contains('play(stateName)'))));
    expect(() => a.setParameter('energy', 1), throwsStateError);
    expect(() => a.time = 0.5, throwsStateError);
    expect(() => a.play(42), throwsArgumentError);

    // Play: at once, sampled at its start.
    a.play('Jump');
    w.frame(1 / 60);
    expect(states.log.sublist(states.log.length - 2), ['exit Idle', 'enter Idle → Jump']);
    expect(a.stateOf()!.name, 'Jump');
    // CrossFade from the middle of Idle.
    a.crossfade('Idle', 0.2, offset: 0.5);
    w.frame(1 / 60);
    final s = a.stateOf()!;
    expect((s.name, s.next), ('Jump', 'Idle'));
    expect(s.nextNormalizedTime, closeTo(0.5, 1e-6));
    expect(a.layers, hasLength(1));
    expect(a.stateOf(layer: 1), isNull);
    // An unknown state is the engine's diagnostic, with the states.
    w.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    a.play('Fly');
    w.frame(1 / 60);
    expect(seen.single.code, DiagnosticCode.invalidPayload);
    expect(seen.single.message, contains('the states: Idle, Jump'));
  });

  test('a one-shot in the base slot: over the machine, stopped, played to its end', () {
    final (w, rig, states) = withController();
    final a = rig.animation;
    const bob = AssetRef(rigAsset, sub: 'anim/bob');
    expect(a.isPlayingOneShot(), isFalse);
    expect(a.stateOf()!.slot, 'Base Layer');

    a.playOneShot(bob, blendIn: 0.1, blendOut: 0.2);
    expect(a.isPlayingOneShot(), isTrue, reason: 'read-your-writes');
    expect(a.isPlayingOneShot(slot: 'Base Layer'), isFalse, reason: 'asked by the empty name');
    ticks(w, 3);
    var s = a.stateOf()!;
    expect((s.name, s.oneShot), ('Idle', bob), reason: 'the machine keeps running under it');
    expect(s.oneShotWeight, inExclusiveRange(0, 1), reason: 'blending in');
    ticks(w, 10);
    s = a.stateOf()!;
    expect(s.oneShotWeight, closeTo(1, 1e-6));
    expect(s.oneShotTime, inExclusiveRange(0, 0.5));
    expect(a.isPlayingOneShot(slot: 'Base Layer'), isTrue);

    a.stopOneShot(blendOut: 0.1);
    expect(a.isPlayingOneShot(), isFalse);
    w.frame(1 / 60);
    expect(a.isPlayingOneShot(), isFalse, reason: 'one blending out does not count');
    expect(a.stateOf()!.oneShot, isNull);
    ticks(w, 10);
    expect(states.log.where((l) => l.startsWith('ended')), isEmpty, reason: 'a stop is not an end');

    // Played to its end (2 s): AnimationEnded, once.
    a.playOneShot(bob);
    ticks(w, 125);
    expect(a.isPlayingOneShot(), isFalse);
    expect(states.log.where((l) => l.startsWith('ended')), ['ended anim/bob']);
    expect(a.stateOf()!.name, 'Idle');

    // An unknown slot is the engine's diagnostic, with the slots.
    w.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    a.playOneShot(bob, slot: 'Arms');
    w.frame(1 / 60);
    expect(seen.single.message, contains('Base Layer'));
  });

  test('a clip made in code plays as a one-shot, changes and is refused when wrong', () {
    final (w, rig, states) = withController();
    final hips = rig.findChild('hips')!;
    final lift = w.createClip((ClipBuilder()
          ..position('hips', {0: const Vec3(0, 3, 0), 1: const Vec3(0, 3, 0)})
          ..marker(0.5, 'top'))
        .build());
    expect(lift.sub, isNull);

    rig.animation.playOneShot(lift, blendIn: 0.05, blendOut: 0.05);
    ticks(w, 30);
    expect(rig.animation.stateOf()!.oneShot, lift);
    expect(hips.localTransform.position.y, closeTo(3, 1e-4), reason: 'the one-shot covers the machine');
    ticks(w, 5);
    expect(states.log, contains('marker top'));
    ticks(w, 30);
    expect(states.log.last, 'ended ${lift.asset}');
    expect(hips.localTransform.position.y, lessThan(1), reason: 'the machine shows again');

    // Changed: the next play reads the new keys.
    w.setClip(lift, (ClipBuilder()..position('hips', {0: const Vec3(0, 5, 0), 1: const Vec3(0, 5, 0)})).build());
    rig.animation.playOneShot(lift, blendIn: 0.05);
    ticks(w, 20);
    expect(hips.localTransform.position.y, closeTo(5, 1e-4));

    // The builder's own checks, then the engine's.
    expect(() => ClipBuilder().build(), throwsStateError);
    expect(() => ClipBuilder()..position('hips', {}), throwsArgumentError);
    expect(() => (ClipBuilder(length: 0.5)..position('hips', {0: Vec3.zero, 1: Vec3.zero})).build(), throwsArgumentError);
    expect(() => ClipBuilder()..marker(0.5, ''), throwsArgumentError);
    expect(() => (ClipBuilder()..scale('hips', {0: const Vec3(double.nan, 1, 1)})).build(), throwsArgumentError);
    expect(() => w.setClip(const AssetRef(rigAsset, sub: 'anim/wave'), (ClipBuilder()..scale('hips', {0: Vec3.one})).build()),
        throwsArgumentError);

    w.destroyClip(lift);
    final fresh2 = World.create(library: lib);
    addTearDown(fresh2.dispose);
    expect(() => fresh2.createClip((ClipBuilder()..scale('hips', {0: Vec3.one})).build()), throwsStateError,
        reason: 'no asset store before a load');
  });

  test('IK targets from code reach the engine (ADR-0033 § 5)', () {
    final (w, rig, _) = withController();
    final a = rig.animation;
    expect(() => a.setIKTarget('hand', const Vec3(1, 2, 3), weight: 2), throwsArgumentError);
    // The controller has no goals: the engine says so, naming none.
    w.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    a.setIKTarget('hand', const Vec3(1, 2, 3));
    w.frame(1 / 60);
    expect(seen.single.code, DiagnosticCode.invalidPayload);
    expect(seen.single.message, contains('no IK goal `hand` (the controller has none)'));
    seen.clear();
    a.clearIKTarget('hand');
    w.frame(1 / 60);
    expect(seen.single.message, contains('no IK goal `hand`'));
  });

  test('an animator without a controller has no states or parameters', () {
    final w = fresh();
    expect(w.loadScene(rigScene, cacheDir: cache).ok, isTrue);
    final rig = w.find('Rig')!;
    w.frame(1 / 60);
    expect(rig.animation.stateOf(), isNull);
    expect(rig.animation.parameters, isEmpty);
    expect(() => rig.animation.setTrigger('jump'),
        throwsA(isA<StateError>().having((e) => e.message, 'message', contains('has no Animator Controller'))));
    // The clip calls still work.
    rig.animation.crossfade(const AssetRef(rigAsset, sub: 'anim/bob'), 0.1, looping: false);
    w.frame(1 / 60);
    expect(rig.animation.state!.clip, const AssetRef(rigAsset, sub: 'anim/bob'));
    expect(() => rig.animation.playOneShot(const AssetRef(rigAsset, sub: 'anim/bob')),
        throwsA(isA<StateError>().having((e) => e.message, 'message', contains('play the clip instead'))));
    expect(rig.animation.isPlayingOneShot(), isFalse);
    expect(() => rig.animation.setIKTarget('hand', Vec3.zero),
        throwsA(isA<StateError>().having((e) => e.message, 'message', contains('IK goals live in a controller'))));
  });
}
