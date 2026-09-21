// Animation from Dart (M3-19, docs/animation.md): the fixture's Rig scene
// — a skinned column under a looping "wave" animator, a Prop under a
// one-shot "bob" — drives named nodes natively; from Dart an object's
// `animation` plays, crossfades, seeks and reads back its state, a
// behaviour hears `onAnimationEnded`, and a missing animator or clip is a
// diagnostic, never an exception.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

const rigScene = 'c0000000-0000-4000-8000-000000000034';
const rigAsset = 'c0000000-0000-4000-8000-000000000005';
const wave = AssetRef(rigAsset, sub: 'anim/0');
const bob = AssetRef(rigAsset, sub: 'anim/1');

/// Logs the clips that end on its object and replays the last one once.
class Listener extends Behaviour {
  final List<String> ended = [];
  bool replayed = false;

  @override
  void onAnimationEnded(AnimationEnded event) {
    ended.add(event.clip.sub ?? '?');
    expect(event.entity, gameObject.entity);
    if (!replayed) {
      replayed = true;
      animation.play(event.clip, looping: false, speed: 4);
    }
  }
}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-animation-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    w.onLoadDiagnostics = (_) {};
    addTearDown(w.dispose);
    return w;
  }

  test('the Rig scene animates its joints and the Prop; state and skin read back', () {
    final w = fresh();
    final report = w.loadScene(rigScene, cacheDir: cache);
    expect(report.ok, isTrue);
    expect(report.sceneName, 'Rig');
    expect(report.entityCount, 15, reason: 'the authored rig and, since M3-19a, its import instanced beside it');
    expect((report.animation.clips, report.animation.skins), (2, 1), reason: 'wave, bob; one skin');
    expect(w.systemTimes().map((s) => s.system), contains(SystemKind.animation));

    final rig = w.find('Rig')!;
    final body = rig.findChild('Body')!;
    final hips = rig.findChild('hips')!;
    final spine = hips.findChild('spine')!;
    final head = spine.findChild('head')!;
    final prop = rig.findChild('Prop')!;
    expect(rig.get<Animator>(), const Animator(clip: wave, speed: 1, looping: true, playing: true));
    expect(
      body.get<Skin>(),
      const Skin(
        skin: AssetRef(rigAsset, sub: 'skin/0'),
        skeleton: 'hips',
      ),
    );
    expect(prop.get<Animator>()!.looping, isFalse);

    // The first tick samples the clips' start; 30 more reach 0.5 s.
    w.frame(1 / 60);
    var s = rig.animation.state!;
    expect(s.clip, wave);
    expect(s.time, closeTo(0, 1e-6));
    expect(s.duration, closeTo(1, 1e-6));
    expect((s.playing, s.looping, s.isFading), (true, true, false));
    for (var i = 0; i < 30; i++) {
      w.frame(1 / 60);
    }
    s = rig.animation.state!;
    expect(s.time, closeTo(0.5, 1e-4));
    expect(s.progress, closeTo(0.5, 1e-4));
    // hips lifted (cubic, midway ≈ 0.125), spine 45° about Z, head 90°
    // about Y (its step key at exactly 0.5 s), the Prop at its peak's
    // half (0.15).
    expect(hips.localTransform.position.y, closeTo(0.125, 1e-3));
    expect(spine.localTransform.rotation.z, closeTo(0.3827, 1e-3), reason: 'sin(22.5°)');
    expect(head.localTransform.rotation.y, closeTo(0.7071, 1e-3), reason: 'sin(45°)');
    expect(prop.localTransform.position.y, closeTo(0.15, 1e-3));
    expect(head.worldTransform!.position.y, closeTo(0.125 + 0.5 + 0.5 * 0.7071, 1e-3), reason: 'hips + spine + head × cos 45°');

    // The one-shot ends at 2 s and holds; the world reports it once (the
    // raw loop here, so the event is seen at the world level — `frame`
    // hands it to the behaviours' fixedUpdate).
    final events = <AnimationEnded>[];
    for (var i = 0; i < 100; i++) {
      w.beginFrame(1 / 60);
      w.step();
      events.addAll(w.drainEvents().whereType<AnimationEnded>());
    }
    expect(events, hasLength(1));
    expect(events.single.entity, prop.entity);
    expect(events.single.clip, bob);
    expect(events.single.tickLo, closeTo(121.5, 1), reason: 'the tick that passed 2 s (the first tick samples 0)');
    final propState = prop.animation.state!;
    expect(propState.playing, isFalse);
    expect(propState.time, closeTo(2, 1e-4));
    expect(prop.localTransform.position.y, closeTo(0, 1e-3));
    expect(rig.animation.isPlaying, isTrue, reason: 'the loop goes on');
  });

  test('an imported model instantiates from its prefab sub-asset and plays a clip (M3-19a)', () {
    final w = fresh();
    w.loadProject(fixture, cacheDir: cache);
    w.frame(1 / 60);
    final before = w.allEntities().length;

    // `rig.glb#prefab`: the anchor is the glTF's own hierarchy — Rig >
    // Body (skinned), hips > spine > head, Prop — spawned like a prefab
    // document's entities, with the source's closure retained.
    const rigPrefab = AssetRef(rigAsset, sub: World.importedPrefabSub);
    final statue = w.instantiate(
      rigPrefab,
      name: 'Statue',
      transform: const Transform(position: Vec3(3, 0, 0), rotation: Quat.identity, scale: Vec3.one),
    );
    final report = w.lastInstantiate!;
    expect(report.ok, isTrue, reason: report.diagnostics.join('\n'));
    expect(report.prefabId, rigAsset);
    expect(report.prefabPath, 'models/rig.glb');
    expect(report.entityCount, 6);
    expect(report.rejectedComponents, 0);
    expect(w.allEntities().length, before + 7, reason: 'anchor + six nodes');
    expect(statue.name, 'Statue');
    final root = statue.findChild('Rig')!;
    final body = root.findChild('Body')!;
    final hips = root.findChild('hips')!;
    final spine = hips.findChild('spine')!;
    final head = spine.findChild('head')!;
    w.frame(1 / 60);
    expect(body.get<Skin>(), const Skin(skin: AssetRef(rigAsset, sub: 'skin/0'), skeleton: 'hips'));
    expect(body.get<MeshRenderer>()!.mesh, const AssetRef(rigAsset, sub: 'mesh/0'));
    expect(head.worldTransform!.position, const Vec3(3, 1, 0), reason: 'bind pose under the anchor');

    // The instance has no animator of its own (a glTF names clips, not
    // players): one added to the anchor finds its targets by name below
    // it — half a second in the statue waves like the scene's rig.
    expect(statue.animation.state, isNull);
    statue.add(const Animator(clip: wave, speed: 1, looping: true, playing: true));
    for (var i = 0; i < 31; i++) {
      w.frame(1 / 60);
    }
    expect(statue.animation.state!.time, closeTo(0.5, 1e-4));
    expect(hips.localTransform.position.y, closeTo(0.125, 1e-3));
    expect(spine.localTransform.rotation.z, closeTo(0.3827, 1e-3), reason: 'sin(22.5°)');
    expect(head.worldTransform!.position.y, closeTo(0.125 + 0.5 + 0.5 * 0.7071, 1e-3));
    expect(head.worldTransform!.position.x, lessThan(3 - 0.3), reason: 'the column leans toward −X, from the anchor at x = 3');

    // Not a prefab: a wrong sub is an ArgumentError before the call, a
    // texture has no hierarchy (FynkelInstantiateError).
    expect(() => w.instantiate(const AssetRef(rigAsset, sub: 'mesh/0')), throwsArgumentError);
    expect(
      () => w.instantiate(const AssetRef('c0000000-0000-4000-8000-000000000010', sub: World.importedPrefabSub)),
      throwsA(isA<FynkelInstantiateError>()),
    );
  });

  test('play, crossfade, pause, speed, time and stop from a behaviour; the end event reaches it', () {
    final w = fresh();
    w.loadScene(rigScene, cacheDir: cache);
    final rig = w.find('Rig')!;
    final hips = rig.findChild('hips')!;
    final prop = rig.findChild('Prop')!;
    final listener = prop.addBehaviour(Listener());
    w.frame(1 / 60);

    // Pause holds the time; resume continues; a negative speed rewinds.
    rig.animation.pause();
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(rig.animation.state!.time, closeTo(0, 1e-6));
    expect(rig.animation.isPlaying, isFalse);
    rig.animation.resume();
    for (var i = 0; i < 10; i++) {
      w.frame(1 / 60);
    }
    expect(rig.animation.state!.time, closeTo(10 / 60, 1e-4));
    rig.animation.speed = -1;
    for (var i = 0; i < 5; i++) {
      w.frame(1 / 60);
    }
    expect(rig.animation.state!.time, closeTo(5 / 60, 1e-4));
    expect(rig.animation.state!.speed, -1);

    // Seek: the pose at 0.5 s is applied without advancing.
    rig.animation.time = 0.5;
    w.frame(1 / 60);
    expect(rig.animation.state!.time, closeTo(0.5, 1e-6));
    expect(hips.localTransform.position.y, closeTo(0.125, 1e-3));

    // A crossfade into "bob" on the Rig: "bob" keys the Prop node by name
    // (found under the Rig) and nothing else; fadeRemaining counts down.
    rig.animation.crossfade(bob, 0.25, looping: false, speed: 1);
    w.frame(1 / 60);
    var s = rig.animation.state!;
    expect(s.clip, bob);
    expect(s.isFading, isTrue);
    expect(s.fadeRemaining, closeTo(0.25 - 1 / 60, 1e-4));
    for (var i = 0; i < 20; i++) {
      w.frame(1 / 60);
    }
    expect(rig.animation.state!.isFading, isFalse);

    // Stop rewinds and holds; the Prop's own animator keeps the Prop.
    rig.animation.stop();
    w.frame(1 / 60);
    s = rig.animation.state!;
    expect((s.playing, s.time), (false, 0));

    // The Prop's one-shot ends at 2 s (tick 122): the behaviour hears it
    // once and replays at 4×, which ends again half a second later.
    while (w.tick < 123) {
      w.frame(1 / 60);
    }
    expect(listener.ended, ['anim/1']);
    for (var i = 0; i < 35; i++) {
      w.frame(1 / 60);
    }
    expect(listener.ended, ['anim/1', 'anim/1']);
    expect(prop.animation.state!.playing, isFalse);
  });

  test('a spawned object with an animator added in the same batch plays; rejections are diagnostics', () {
    final w = fresh();
    w.loadScene(rigScene, cacheDir: cache);
    final rig = w.find('Rig')!;
    // A second rig-less object: the clip resolves its targets under the
    // animator by name, so a child named "Prop" is what "bob" moves.
    final host = w.spawn(name: 'host');
    final target = w.spawn(name: 'Prop', parent: host);
    host.add(const Animator(clip: bob, speed: 2, looping: true, playing: true));
    w.frame(1 / 60);
    expect(host.animation.state!.clip, bob);
    for (var i = 0; i < 30; i++) {
      w.frame(1 / 60);
    }
    expect(host.animation.state!.time, closeTo(1, 1e-4), reason: '30 ticks at 2×');
    expect(target.localTransform.position.y, closeTo(0.3, 1e-3), reason: 'the peak of bob');

    w.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    target.animation.play(wave); // no animator
    host.animation.crossfade(wave, -1); // negative fade
    host.animation.speed = double.nan;
    w.frame(1 / 60);
    expect(seen.map((d) => d.code), [
      DiagnosticCode.missingComponent,
      DiagnosticCode.invalidPayload,
      DiagnosticCode.invalidPayload,
    ]);
    expect(seen.map((d) => d.component), [Animator.componentId, Animator.componentId, Animator.componentId]);
    seen.clear();

    // A clip the world does not know: reported once, the animator waits.
    const unknown = AssetRef('c0000000-0000-4000-8000-0000000000ff', sub: 'anim/0');
    host.animation.play(unknown);
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(seen.map((d) => d.code), [DiagnosticCode.clipUnavailable]);
    expect(seen.single.entity, host.entity);
    expect(host.animation.state!.duration, 0);
    expect(rig.animation.isPlaying, isTrue);

    // A pending object is driven through its temp id: the spawn, the
    // animator and the play land in one tick.
    final pending = w.spawn(name: 'pending')..add(const Animator(clip: wave, speed: 1, looping: true, playing: true));
    pending.animation.play(bob, looping: false);
    w.frame(1 / 60);
    expect(pending.animation.state!.clip, bob);
    expect(w.byEntity(target.entity)!.animation.state, isNull, reason: 'no animator');
  });
}
