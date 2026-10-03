import 'package:fynkel/src/native/bridge.dart' show NativeGameplayRef;
// End-to-end SDK tests over the real native library (ABI v3): spawn /
// write / read-your-writes / destroy, stale handles, diagnostics as
// FynkelError, presentation lane, hierarchy, frame plan.

import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World world() => World.create(library: lib)..throwOnDiagnostics = true;

  test('spawn and data edits publish before returning; pending identities stay inside edits', () {
    final w = world();
    addTearDown(w.dispose);
    final go = w.spawn(
      transform: const Pose(position: Vec3(1, 2, 3), rotation: Quat.identity, scale: Vec3.one),
      name: 'player',
    );
    expect(go.isPending, isFalse);
    expect(go.transform.position, const Vec3(1, 2, 3), reason: 'initial value readable while pending');
    go.setPresentationPosition(0, 0, 0);
    go.transform.position = const Vec3(1, 2, 3.5);
    expect(go.transform.position, const Vec3(1, 2, 3.5), reason: 'read-your-writes while pending');

    final plan = w.frame(1 / 60);
    expect(plan.steps, 1);
    expect(go.isPending, isFalse);
    expect(w.nameOf(go), 'player');
    expect(go.transform.position, const Vec3(1, 2, 3.5));
    expect(
      go.worldTransform!.position,
      const Vec3(1, 2, 3.5),
      reason: 'asked from the world until its first snapshot (foundation 4.4)',
    );

    w.frame(1 / 60);
    expect(go.worldTransform!.position, const Vec3(1, 2, 3.5));

    // Read-your-writes: overlay before the tick, snapshot after.
    go.transform.position = const Vec3(5, 0, 0);
    expect(go.transform.position, const Vec3(5, 0, 0));
    expect(go.worldTransform!.position, const Vec3(5, 0, 0), reason: 'world transform shares logical authority');
    w.frame(1 / 60);
    expect(go.transform.position, const Vec3(5, 0, 0));
    expect(go.worldTransform!.position, const Vec3(5, 0, 0));

    go.destroy();
    expect(go.isAlive, isFalse, reason: 'destroy commits before returning');
    w.frame(1 / 60);
    expect(go.isAlive, isFalse);
    expect(() => go.transform.position = Vec3.zero, throwsStateError);
    expect(w.allEntities(), isEmpty);
  });

  test('invalid ordinary writes reject synchronously and never replace committed data', () {
    final w = world();
    addTearDown(w.dispose);
    final b = w.spawn();
    b.transform.position = const Vec3(9, 9, 9);
    for (final invalid in [double.nan, double.infinity]) {
      expect(() => b.setLocalPosition(invalid, 0, 0), throwsStateError);
      expect(b.transform.position, const Vec3(9, 9, 9));
      w.throwOnDiagnostics = false;
    }
    w.frame(1 / 60);
    expect(b.transform.position, const Vec3(9, 9, 9));
    expect(
      () => w.native.editGameplay(
        (edit) => edit.set(NativeGameplayRef.live(b.entity), const Pose(position: Vec3(double.nan, 0, 0))),
      ),
      throwsStateError,
    );
    w.step();
    expect(b.transform.position, const Vec3(9, 9, 9));
  });

  test('presentation lane: visible now, authoritative next tick, rejected for physics-owned', () {
    final w = world();
    addTearDown(w.dispose);
    final cam = w.spawn(name: 'camera');
    w.frame(1 / 60);
    w.frame(1 / 60);
    // A 120 Hz frame with no tick.
    final plan = w.beginFrame(1 / 240);
    expect(plan.steps, 0);
    cam.setPresentationTransform(const Pose(position: Vec3(0, 3, 0), rotation: Quat.identity, scale: Vec3.one));
    expect(cam.transform.position, const Vec3(0, 3, 0));
    w.frame(1 / 60);
    expect(cam.worldTransform!.position, const Vec3(0, 3, 0));
  });

  test('hierarchy: parent at spawn, reparent keep-world, subtree destroy', () {
    final w = world();
    addTearDown(w.dispose);
    final root = w.spawn(
      transform: const Pose(position: Vec3(10, 0, 0), rotation: Quat.identity, scale: Vec3.one),
      name: 'root',
    );
    w.frame(1 / 60);
    final child = w.spawn(
      transform: const Pose(position: Vec3(1, 0, 0), rotation: Quat.identity, scale: Vec3.one),
      parent: root,
      name: 'child',
    );
    final loose = w.spawn(name: 'loose');
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(root.children, [child]);
    expect(child.parent, root);
    expect(child.worldTransform!.position, const Vec3(11, 0, 0));

    loose.setParent(root, keepWorld: true);
    final events = <EngineEvent>[];
    w.frame(1 / 60, update: () {});
    events.addAll(w.drainEvents());
    w.frame(1 / 60);
    expect(loose.parent, root);
    expect(loose.transform.localPosition.x, closeTo(-10, 1e-5), reason: 'local adjusted to keep world');
    expect(loose.transform.position.x, closeTo(0, 1e-5), reason: 'position is the world one');
    expect(loose.worldTransform!.position.x, closeTo(0, 1e-5));

    root.destroy();
    w.frame(1 / 60);
    expect(root.isAlive || child.isAlive || loose.isAlive, isFalse);
    expect(w.allEntities(), isEmpty);
  });

  test('frame plan: catch-up cap and MissedDeadline surface as diagnostics', () {
    final w = World.create(library: lib, maxStepsPerFrame: 2)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    // Informational: reported, never thrown, even with throwOnDiagnostics.
    final reported = <FynkelDiagnostic>[];
    w.onDiagnostics = reported.addAll;
    final plan = w.beginFrame(0.25);
    expect(plan.missedDeadline, isTrue);
    expect(reported.single.code, DiagnosticCode.missedDeadline);
    expect(reported.single.isInformational, isTrue);
    final events = w.drainEvents();
    expect(events.single, isA<MissedDeadline>());
    expect(w.step().tick, 1);
    expect(w.step().tick, 2);
  });

  test('bridge stats count calls and bytes', () {
    final w = world();
    addTearDown(w.dispose);
    for (var i = 0; i < 10; i++) {
      w.spawn().localTransform; // read: it rides the snapshot (foundation 4.4)
    }
    w.frame(1 / 60);
    w.frame(1 / 60);
    final s = w.stats();
    expect(s.liveEntities, 10);
    expect(s.subscriptions, 10);
    expect(s.stepCalls, 2);
    expect(s.dartCopies, greaterThan(0));
    expect(s.bytesIn, greaterThan(0));
    expect(s.bytesOut, greaterThan(0));
    expect(s.ffiCalls, greaterThan(0));
  });

  test('loadProject / loadScene: the fixture project reaches the world from its cache', () {
    final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
    final cache = Directory.systemTemp.createTempSync('fynkel-sdk-cache-').path;
    addTearDown(() => Directory(cache).deleteSync(recursive: true));
    expect(cookProject(fixture, cacheDir: cache), cache);
    expect(File('$cache/index.json').existsSync(), isTrue);
    final w = world();
    addTearDown(w.dispose);
    final r = w.loadProject(fixture, cacheDir: cache);
    expect(r.ok, isTrue);
    expect(r.projectName, 'Fixture Game');
    expect(r.projectDir, fixture);
    expect(r.cacheDir, cache);
    expect(r.cacheProfile, 'dev');
    expect(r.sceneName, 'Main');
    expect(r.sceneId, 'c0000000-0000-4000-8000-000000000030');
    expect(r.startupScene, r.sceneId);
    expect(r.scenePath, 'scenes/main.scene.json');
    expect(r.sceneKey, hasLength(64));
    expect(
      r.entityCount,
      6,
      reason: 'the crate prefab was expanded by the cook (Crate A is its root); Floor and Crate B are physics-only',
    );
    expect(r.rejectedComponents, 0);
    expect(r.assetCount, 8, reason: "the Robot's tone (M3-11) too");
    expect(r.residency.retained, 8);
    expect(r.residency.meshes, greaterThan(0), reason: 'required CPU payloads are ready before activation');
    expect(r.readiness.cpu, 'ready');
    expect(r.readiness.simulation, 'ready');
    expect(r.readiness.gpu, 'deferred');
    expect(r.readiness.required, ['cpu', 'simulation']);
    expect(r.residency.bytes, lessThanOrEqualTo(r.residency.cacheBudgetBytes));
    expect(w.allEntities(), hasLength(6));
    w.frame(1 / 60);

    // Any scene by id from the same cache (the world remembers it); the
    // same one again replaces the first copy (M3-12).
    expect(w.cacheDir, cache);
    final again = w.loadScene(r.sceneId!);
    expect(again.sceneName, 'Main');
    expect(again.mode, 'replace');
    expect(again.projectDir, isNull);
    expect(again.scenes.map((s) => s.name), ['Main']);
    expect(w.allEntities(), hasLength(6));

    // A prefab is not a scene; an id the cache lacks; not a UUID at all.
    expect(
      () => w.loadScene('c0000000-0000-4000-8000-000000000040', cacheDir: cache),
      throwsA(isA<FynkelLoadError>().having((e) => e.report.errors.map((d) => d.code), 'codes', contains('FYN-R0006'))),
    );
    expect(
      () => w.loadScene('c0000000-0000-4000-8000-0000000000ff'),
      throwsA(isA<FynkelLoadError>().having((e) => e.report.errors.map((d) => d.code), 'codes', contains('FYN-R0003'))),
    );
    expect(() => w.loadScene('robot'), throwsA(isA<FynkelBridgeError>()));
    expect(w.allEntities(), hasLength(6), reason: 'a failed load leaves the world alone');

    // No cache: the runtime never falls back to the source tree.
    expect(
      () => w.loadProject(fixture, cacheDir: '$cache/nope'),
      throwsA(isA<FynkelLoadError>().having((e) => e.report.errors.map((d) => d.code), 'codes', contains('FYN-R0001'))),
    );
    expect(() => w.loadProject('/nonexistent'), throwsA(isA<FynkelLoadError>()));
    expect(w.isPoisoned, isFalse);
  });

  test('a panic in the world is a FynkelPanicError with its message, and the world stays stopped (foundation 4.1)', () {
    final w = World.create(library: lib);
    addTearDown(w.dispose);
    w.spawn(name: 'Box');
    w.frame(1 / 60);
    expect(w.panicMessage, isNull);

    final panic = isA<FynkelPanicError>().having((e) => e.panic, 'panic', contains('fynkel_world_debug_panic'));
    expect(w.debugPanic, throwsA(panic));
    expect(w.isPoisoned, isTrue);
    expect(w.panicMessage, contains('fynkel_world_debug_panic'));
    // Every later call says why, not a bare status.
    expect(() => w.frame(1 / 60), throwsA(panic));
    expect(() => w.frame(1 / 60), throwsA(panic));
    expect(w.debugPanic, throwsA(panic));
  });

  test('after dispose the objects are dead, keep their last pose, and every call throws, never touching freed memory (foundation 4.2)', () {
    final w = World.create(library: lib);
    final go = w.spawn(
      name: 'Box',
      transform: identityTransform.copyWith(position: const Vec3(1, 2, 3)),
    );
    w.frame(1 / 60);
    go.transform.position = const Vec3(4, 5, 6);
    w.frame(1 / 60);
    final pose = go.localTransform;
    expect(pose.position, const Vec3(4, 5, 6));
    expect(go.worldTransform, isNotNull);

    w.dispose();
    expect(w.isDisposed, isTrue);
    expect(go.isAlive, isFalse);
    expect(go.localTransform, pose, reason: 'the last pose, read from Dart');
    expect(go.worldTransform, isNull);
    final disposed = isA<StateError>().having((e) => e.message, 'message', contains('disposed'));
    expect(() => w.frame(1 / 60), throwsA(disposed));
    expect(() => w.spawn(name: 'Late'), throwsA(disposed));
    expect(() => w.tick, throwsA(disposed));
    expect(() => w.allEntities(), throwsA(disposed));
    w.dispose(); // twice is fine
  });

  test('the snapshot carries only the objects Dart reads; a handle alone costs nothing (foundation 4.4)', () {
    final w = World.create(library: lib);
    addTearDown(w.dispose);
    final parent = w.spawn(
      name: 'Parent',
      transform: identityTransform.copyWith(position: const Vec3(10, 0, 0)),
    );
    final all = [
      for (var i = 0; i < 50; i++)
        w.spawn(
          name: 'o$i',
          parent: parent,
          transform: identityTransform.copyWith(position: Vec3(i.toDouble(), 0, 0)),
        ),
    ];
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(w.native.snapshot.recordCount, 0, reason: 'spawned, never read');
    expect(w.find('o7'), same(all[7]));
    w.frame(1 / 60);
    expect(w.native.snapshot.recordCount, 0, reason: 'found, never read');

    // A read is right straight away (the world is asked), and the object
    // rides the snapshot from the next tick.
    final o7 = all[7];
    expect(o7.localTransform.position, const Vec3(7, 0, 0));
    expect(o7.worldTransform!.position, const Vec3(17, 0, 0));
    w.frame(1 / 60);
    expect(w.native.snapshot.recordCount, 2);
    expect(o7.transform.position, const Vec3(17, 0, 0), reason: 'the parent is read too');
    w.frame(1 / 60);
    expect(w.native.snapshot.recordCount, 2);

    // Not read for a while: dropped again; a later read is still right.
    for (var i = 0; i < 130; i++) {
      w.frame(1 / 60);
    }
    expect(w.native.snapshot.recordCount, 0);
    parent.transform.localPosition = const Vec3(20, 0, 0);
    w.frame(1 / 60);
    expect(o7.worldTransform!.position, const Vec3(27, 0, 0));
    expect(all[3].transform.position, const Vec3(23, 0, 0));
  });
}
