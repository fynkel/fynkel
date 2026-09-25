// End-to-end SDK tests over the real native library (ABI v1): spawn /
// write / read-your-writes / destroy, stale handles, diagnostics as
// FynkelError, presentation lane, hierarchy, frame plan.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/generated/schema.g.dart' show DestroyCommand, DiagnosticRecord;
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World world() => World.create(library: lib)..throwOnDiagnostics = true;

  test('spawn resolves on the next tick; reads follow writes', () {
    final w = world();
    addTearDown(w.dispose);
    final go = w.spawn(
      transform: const Transform(position: Vec3(1, 2, 3), rotation: Quat.identity, scale: Vec3.one),
      name: 'player',
    );
    expect(go.isPending, isTrue);
    expect(go.transform.position, const Vec3(1, 2, 3), reason: 'initial value readable while pending');
    expect(
      () => go.setPresentationPosition(0, 0, 0),
      throwsStateError,
      reason: 'the presentation lane needs the handle',
    );
    // A simulation write while pending rides in the spawn's batch as a
    // temp reference (M3-08b) and lands in the spawn's tick.
    go.transform.position = const Vec3(1, 2, 3.5);
    expect(go.transform.position, const Vec3(1, 2, 3.5), reason: 'read-your-writes while pending');

    final plan = w.frame(1 / 60);
    expect(plan.steps, 1);
    expect(go.isPending, isFalse);
    expect(w.nameOf(go), 'player');
    expect(go.transform.position, const Vec3(1, 2, 3.5));
    expect(go.worldTransform, isNull, reason: 'subscribed after resolution; first snapshot next tick');

    w.frame(1 / 60);
    expect(go.worldTransform!.position, const Vec3(1, 2, 3.5));

    // Read-your-writes: overlay before the tick, snapshot after.
    go.transform.position = const Vec3(5, 0, 0);
    expect(go.transform.position, const Vec3(5, 0, 0));
    expect(go.worldTransform!.position, const Vec3(1, 2, 3.5), reason: 'world transform is snapshot-only');
    w.frame(1 / 60);
    expect(go.transform.position, const Vec3(5, 0, 0));
    expect(go.worldTransform!.position, const Vec3(5, 0, 0));

    go.destroy();
    expect(go.isAlive, isTrue, reason: 'until the tick');
    w.frame(1 / 60);
    expect(go.isAlive, isFalse);
    expect(() => go.transform.position = Vec3.zero, throwsStateError);
    expect(w.allEntities(), isEmpty);
  });

  test('rejected command clears the overlay and throws FynkelError in debug', () {
    final w = world();
    addTearDown(w.dispose);
    final a = w.spawn();
    w.frame(1 / 60);
    // Destroy a via a second handle path: stale handle after destruction.
    a.destroy();
    w.frame(1 / 60);
    // Submit a raw stale command through the native layer to reach the
    // world's rejection path with a live GameObject target too.
    final b = w.spawn();
    w.frame(1 / 60);
    b.transform.position = const Vec3(9, 9, 9);
    // NaN transform is rejected at the tick with INVALID_PAYLOAD.
    b.setLocalTransform(const Transform(position: Vec3(double.nan, 0, 0), rotation: Quat.identity, scale: Vec3.one));
    expect(b.transform.position.x.isNaN, isTrue, reason: 'overlay shows the staged value');
    expect(
      () => w.frame(1 / 60),
      throwsA(isA<FynkelError>().having((e) => e.diagnostics.single.code, 'code', DiagnosticCode.invalidPayload)),
    );
    expect(b.transform.position, const Vec3(9, 9, 9), reason: 'overlay cleared; earlier applied value from snapshot');

    // Release-mode path: logged, not thrown.
    w.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    b.setLocalTransform(
      const Transform(position: Vec3(double.infinity, 0, 0), rotation: Quat.identity, scale: Vec3.one),
    );
    w.frame(1 / 60);
    expect(seen.single.code, DiagnosticCode.invalidPayload);
    expect(seen.single.entity, b.entity);
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
    cam.setPresentationTransform(const Transform(position: Vec3(0, 3, 0), rotation: Quat.identity, scale: Vec3.one));
    expect(cam.transform.position, const Vec3(0, 3, 0));
    w.flushPresentation();
    expect(w.stats().stagedCommands, 1);
    w.frame(1 / 60);
    expect(cam.worldTransform!.position, const Vec3(0, 3, 0));

    // Structural command on the presentation lane is rejected immediately.
    final r = w.native.submit(
      CommandLane.presentation,
      (CommandBatch()..destroy(DestroyCommand(entity: cam.entity))).encoded(),
    );
    expect(r.rejected, 1);
    final d = w.native.diagnostics
        .fixed(DiagnosticRecord.wireSize, DiagnosticRecord.decode)
        .map(FynkelDiagnostic.fromRecord)
        .toList();
    expect(d.single.code, DiagnosticCode.wrongLane);
  });

  test('hierarchy: parent at spawn, reparent keep-world, subtree destroy', () {
    final w = world();
    addTearDown(w.dispose);
    final root = w.spawn(
      transform: const Transform(position: Vec3(10, 0, 0), rotation: Quat.identity, scale: Vec3.one),
      name: 'root',
    );
    w.frame(1 / 60);
    final child = w.spawn(
      transform: const Transform(position: Vec3(1, 0, 0), rotation: Quat.identity, scale: Vec3.one),
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
      w.spawn();
    }
    w.frame(1 / 60);
    w.frame(1 / 60);
    final s = w.stats();
    expect(s.liveEntities, 10);
    expect(s.subscriptions, 10);
    expect(s.submitCalls, 1, reason: 'ten spawns, one batch');
    expect(s.stepCalls, 2);
    expect(s.dartCopies, 1);
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
    expect(r.entityCount, 6, reason: 'the crate prefab was expanded by the cook (Crate A is its root); Floor and Crate B are physics-only');
    expect(r.rejectedComponents, 0);
    expect(r.assetCount, 8, reason: "the Robot's tone (M3-11) too");
    expect(r.residency.retained, 8);
    expect(r.residency.meshes, 0, reason: 'payloads load on first draw');
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
}
