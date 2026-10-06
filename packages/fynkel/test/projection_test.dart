// Projected world anchors (M3-13, blueprint §10, docs/hud.md): the
// `World.projection` view maps entity-attached points into a viewport with
// the frame's camera — one FFI call per update, bounded to 64 anchors, the
// explicit occlusion policy by collider mask, the FYN-R0012 diagnostic.

import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-projection-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  /// The fixture's Main scene: a camera at (0, 2, 6) looking down -Z with a
  /// vertical field of view of 1 rad; Crate A at (2, 0, 0); the Floor at
  /// (0, -0.5, 0) with a 10 × 0.5 × 10 box collider on layer 1.
  World loaded() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    w.loadProject(fixture, cacheDir: cache);
    w.frame(1 / 60);
    return w;
  }

  /// Where a world point lands in a `width` × `height` viewport for the
  /// fixture camera (perspective, no rotation).
  (double, double) expected(Vec3 p, double width, double height) {
    const camY = 2.0, camZ = 6.0, fovY = 1.0;
    final depth = camZ - p.z;
    final halfH = depth * math.tan(fovY / 2);
    final halfW = halfH * width / height;
    return ((p.x / halfW * 0.5 + 0.5) * width, (0.5 - (p.y - camY) / halfH * 0.5) * height);
  }

  test('manual Step projects the completed hierarchy and preserves clocks and normal interpolation', () {
    for (final fraction in [0.0, 0.5]) {
      final w = loaded();
      final root = w.spawn();
      final child = w.spawn(
        parent: root,
        transform: const Pose(position: Vec3(1, 0, 0)),
      );
      w.frame(1 / 60);
      w.frame(fraction / 60);
      w.timeScale = 0;
      final time = w.context.time;
      final game = time.elapsed, real = time.realElapsed, simulation = time.simulationElapsed;
      final frame = time.frame, tick = w.tick, alpha = w.alpha;
      final anchor = w.projection.add(child);
      w.projection.setViewport(800, 600);
      for (final x in [2.0, 4.0]) {
        w.stepFrame(fixedUpdate: (_) => root.transform.localPosition = Vec3(x, 0, 0));
        w.projection.update();
        final (px, py) = expected(Vec3(x + 1, 0, 0), 800, 600);
        expect(anchor.x, closeTo(px, 0.01));
        expect(anchor.y, closeTo(py, 0.01));
        expect(time.elapsed, game);
        expect(time.realElapsed, real);
        expect(time.frame, frame);
        expect(w.alpha, alpha);
      }
      expect(w.tick, tick + 2);
      expect(time.simulationElapsed - simulation, closeTo(2 / 60, 1e-6));
      w.timeScale = 1;
      w.frame(1 / 60, fixedUpdate: (_) => root.transform.localPosition = const Vec3(6, 0, 0));
      w.projection.update();
      final (px, _) = expected(Vec3(5 + 2 * fraction, 0, 0), 800, 600);
      expect(anchor.x, closeTo(px, 0.01), reason: 'regular frames resume interpolation');
    }
  });

  test('120 Hz presentation interpolates 60 Hz kinematic movement on every frame', () {
    final w = loaded();
    final cube = w.spawn(transform: const Pose(position: Vec3(0, 0, 0)));
    cube.set(const RigidBody(bodyType: BodyType.kinematic));
    cube.set(const Collider());
    w.frame(1 / 60);
    final anchor = w.projection.add(cube);
    w.projection.setViewport(800, 600);
    var elapsed = 0.0;
    var angle = 0.0;
    // Quantized host timestamps and separate Spinner/Mover commits.
    for (var frame = 0; frame < 240; frame++) {
      final dt = (frame % 3 == 0 ? 8334 : 8333) / 1e6;
      elapsed += dt;
      w.frame(
        dt,
        fixedUpdate: (_) {
          angle += 1.5 * w.fixedDt;
          cube.transform.rotation = Quat.axisAngle(Vec3.up, angle);
          cube.transform.position = cube.transform.position + Vec3(2 * w.fixedDt, 0, 0);
        },
      );
      w.projection.update();
      final x = 2.0 * math.max(0.0, elapsed - w.fixedDt);
      final (px, py) = expected(Vec3(x, 0, 0), 800, 600);
      expect(anchor.x, closeTo(px, 0.02), reason: 'frame $frame, alpha ${w.alpha}');
      expect(anchor.y, closeTo(py, 0.02));
    }
  });

  test('an anchor follows its object through the frame camera', () {
    final w = loaded();
    final crate = w.find('Crate A')!;
    final anchor = w.projection.add(crate);
    final top = w.projection.add(crate, offset: const Vec3(0, 1.5, 0));
    w.projection.setViewport(800, 600);
    expect(w.projection.length, 2);
    expect(anchor.isAttached, isTrue);

    final before = w.stats().ffiCalls;
    expect(w.projection.update(), 2);
    expect(w.stats().ffiCalls, before + 2, reason: 'one call for every anchor (and the stats call)');
    expect(w.projection.camera, w.find('Camera')!.entity);

    final (x, y) = expected(const Vec3(2, 0.5, 0), 800, 600);
    expect(anchor.x, closeTo(x, 0.01));
    expect(anchor.y, closeTo(y, 0.01));
    expect(anchor.depth, closeTo(6, 1e-4));
    expect(anchor.inFront, isTrue);
    expect(anchor.onScreen, isTrue);
    expect(anchor.visible, isTrue);
    expect(anchor.occluded, isFalse);
    expect(anchor.missing, isFalse);
    final (tx, ty) = expected(const Vec3(2, 2, 0), 800, 600);
    expect(top.x, closeTo(tx, 0.01));
    expect(top.y, closeTo(ty, 0.01));
    expect(top.y, lessThan(anchor.y), reason: 'above the crate on screen');

    // The crate moves right and up: the label goes with it (the frame
    // interpolates between the last two ticks, so the move is fully on
    // screen one tick after it applied — like the rendered mesh).
    crate.setLocalPosition(3, 1, 0);
    w.frame(1 / 60);
    w.projection.update();
    expect(
      anchor.x,
      closeTo(expected(const Vec3(3, 1, 0), 800, 600).$1, 0.01),
      reason: 'standalone edits commit before the frame',
    );
    w.frame(1 / 60);
    w.projection.update();
    final (mx, my) = expected(const Vec3(3, 1, 0), 800, 600);
    expect(anchor.x, closeTo(mx, 0.01));
    expect(anchor.y, closeTo(my, 0.01));

    // Out of the frustum: in front but off the viewport, then behind.
    crate.setLocalPosition(40, 0, 0);
    w.frame(1 / 60);
    w.frame(1 / 60);
    w.projection.update();
    expect(anchor.inFront, isTrue);
    expect(anchor.onScreen, isFalse);
    expect(anchor.visible, isFalse);
    expect(anchor.x, greaterThan(800));
    crate.setLocalPosition(0, 0, 8);
    w.frame(1 / 60);
    w.frame(1 / 60);
    w.projection.update();
    expect(anchor.inFront, isFalse);
    expect(anchor.visible, isFalse);
    expect(anchor.depth, lessThan(0));

    // The viewport scales the answer; the aspect ratio shapes it.
    crate.setLocalPosition(2, 0.5, 0);
    w.frame(1 / 60);
    w.frame(1 / 60);
    w.projection.setViewport(400, 300);
    w.projection.update();
    expect(anchor.x, closeTo(x / 2, 0.01));
    expect(anchor.y, closeTo(y / 2, 0.01));
  });

  test('occlusion is the explicit collider-mask policy', () {
    final w = loaded();
    // A point under the floor (still in the viewport): hidden only for
    // anchors that ask the floor's layer to occlude them.
    final below = w.spawn(
      transform: const Pose(position: Vec3(0, -1, 0), rotation: Quat.identity, scale: Vec3(1, 1, 1)),
      name: 'Below',
    );
    w.frame(1 / 60);
    final plain = w.projection.add(below);
    final byFloor = w.projection.add(below, occlusionMask: w.layerMask(['default']));
    final byProps = w.projection.add(below, occlusionMask: w.layerMask(['props']));
    w.projection.setViewport(800, 600);
    w.projection.update();
    expect(plain.visible, isTrue, reason: 'mask 0: never hidden by geometry');
    expect(byFloor.occluded, isTrue);
    expect(byFloor.visible, isFalse);
    expect(byFloor.onScreen, isTrue, reason: 'still placed: the position is unaffected');
    expect(byFloor.x, closeTo(plain.x, 1e-6));
    expect(byProps.occluded, isFalse, reason: 'the floor is not on the props layer');
    // The mask is mutable: the same anchor changes policy.
    plain.occlusionMask = 0xffffffff;
    w.projection.update();
    expect(plain.occluded, isTrue);
  });

  test('pending and destroyed objects are missing; prune drops them', () {
    final w = loaded();
    final pending = w.spawn(name: 'Later');
    final anchor = w.projection.add(pending);
    expect(anchor.missing, isFalse, reason: 'ordinary spawn is committed');
    w.projection.setViewport(800, 600);
    expect(w.projection.update(), 1);
    expect(anchor.missing, isFalse);
    expect(anchor.visible, isTrue);
    w.frame(1 / 60);
    w.projection.update();
    expect(anchor.missing, isFalse);
    expect(anchor.visible, isTrue);

    pending.destroy();
    w.frame(1 / 60);
    w.projection.update();
    expect(anchor.missing, isTrue);
    expect(anchor.visible, isFalse);
    final crate = w.projection.add(w.find('Crate A')!);
    w.projection.prune();
    expect(w.projection.anchors, [crate]);
    expect(anchor.isAttached, isFalse);
    // A bare entity handle works too; a bogus one is missing.
    final byHandle = w.projection.add(w.find('Crate A')!.entity);
    final bogus = w.projection.add(const EntityRef(0x7fff, 0xffff, 1));
    w.projection.update();
    expect(byHandle.visible, isTrue);
    expect(bogus.missing, isTrue);
  });

  test('the view is bounded to 4096 anchors and says so once', () {
    // ADR-0062 amendment 1 raised the bound from 64: health bars over a
    // crowd are one request.
    const max = 4096;
    expect(ProjectionView.maxAnchors, max);
    final w = loaded();
    w.throwOnDiagnostics = false;
    final diagnostics = <FynkelDiagnostic>[];
    w.onDiagnostics = diagnostics.addAll;
    final crate = w.find('Crate A')!;
    final anchors = List.generate(max + 1, (_) => w.projection.add(crate));
    w.projection.setViewport(800, 600);
    final calls = w.stats().ffiCalls;
    expect(w.projection.update(), max);
    expect(w.stats().ffiCalls, calls + 2, reason: 'one projection call (the stats call is the other)');
    expect(anchors[max - 1].visible, isTrue);
    expect(anchors[max].dropped, isTrue);
    expect(anchors[max].visible, isFalse);
    expect(diagnostics, hasLength(1));
    expect(diagnostics.single.code, DiagnosticCode.tooManyAnchors);
    expect(diagnostics.single.isInformational, isTrue);
    expect(diagnostics.single.message, startsWith('FYN-R0012'));
    // Reported once per overflow episode, not per frame.
    w.projection.update();
    expect(diagnostics, hasLength(1));
    // Back under the bound the episode ends; a new overflow reports again.
    anchors[max].remove();
    expect(w.projection.update(), max);
    expect(anchors[max].isAttached, isFalse);
    w.projection.add(crate);
    w.projection.update();
    expect(diagnostics, hasLength(2));
    // The engine's own diagnostic does not throw either (informational).
    w.throwOnDiagnostics = true;
    w.frame(1 / 60);
    w.projection.update();
  });

  test('listeners run when an anchor moved, not for a still frame', () {
    final w = loaded();
    final crate = w.find('Crate A')!;
    w.projection.setViewport(800, 600);
    var runs = 0;
    w.projection.addListener(() => runs++);
    final a = w.projection.add(crate);
    expect(w.projection.update(), 1);
    expect(runs, 1, reason: 'a fresh anchor is placed');
    final x = a.x;
    expect(w.projection.update(), 1);
    expect(w.projection.update(), 1);
    expect(runs, 1, reason: 'nothing moved: no repaint');
    expect(w.projection.version, 3, reason: 'the version still counts every update');

    crate.setLocalPosition(2.5, 0, 0);
    w.frame(1 / 60);
    expect(w.projection.update(), 1);
    expect(runs, 2, reason: 'standalone edits publish before the frame');
    w.frame(1 / 60);
    expect(w.projection.update(), 1);
    expect(a.x, isNot(closeTo(x, 0.01)));
    expect(runs, 2);
    w.projection.update();
    expect(runs, 2);

    // A second anchor appearing is a change; so is one going missing.
    final b = w.projection.add(crate, offset: const Vec3(0, 1, 0));
    w.projection.update();
    expect(runs, 3);
    expect(b.visible, isTrue);
    crate.destroy();
    w.frame(1 / 60);
    w.frame(1 / 60);
    w.projection.update();
    expect(runs, 4, reason: 'both anchors are missing now');
    w.projection.update();
    expect(runs, 4);
  });

  test('without a camera nothing is placed; without anchors nothing is called', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.projection.setViewport(800, 600);
    final before = w.stats().ffiCalls;
    var runs = 0;
    w.projection.addListener(() => runs++);
    expect(w.projection.update(), 0);
    expect(w.stats().ffiCalls, before + 1, reason: 'an empty view makes no call (the stats call is one)');
    expect(runs, 0);
    expect(w.projection.version, 0);

    final o = w.spawn(name: 'Lonely');
    w.frame(1 / 60);
    final a = w.projection.add(o);
    expect(w.projection.update(), 1);
    expect(w.projection.camera, isNull);
    expect(a.noCamera, isTrue);
    expect(a.visible, isFalse);
    expect(runs, 1);
    expect(w.projection.version, 1);
    w.projection.clear();
    expect(w.projection.isEmpty, isTrue);
    expect(a.isAttached, isFalse);
  });

  // ADR-0062 step 2: one-off answers beside the per-frame anchors.
  test('a world point goes to the screen without an anchor', () {
    final w = loaded();
    w.projection.setViewport(800, 600);
    final before = w.stats().ffiCalls;
    final p = w.projection.worldToScreen(const Vec3(2, 0, 0))!;
    expect(w.stats().ffiCalls, before + 2, reason: 'one projection call (the stats call is the other)');
    final (ex, ey) = expected(const Vec3(2, 0, 0), 800, 600);
    expect(p.x, closeTo(ex, 0.01));
    expect(p.y, closeTo(ey, 0.01));
    expect(p.depth, closeTo(6, 1e-4));
    expect(p.inFront && p.onScreen, isTrue);
    expect(w.projection.isEmpty, isTrue, reason: 'no anchor was registered');
    // Another viewport than the view's.
    final small = w.projection.worldToScreen(const Vec3(2, 0, 0), width: 400, height: 300)!;
    expect(small.x, closeTo(ex / 2, 0.01));
    // Behind the camera: not placed.
    final behind = w.projection.worldToScreen(const Vec3(0, 2, 8))!;
    expect(behind.inFront, isFalse);
    expect(behind.onScreen, isFalse);
  });

  test('a screen point becomes the ray through what it shows, and picks it', () {
    final w = loaded();
    w.projection.setViewport(800, 600);
    // The ray at a projected point passes through the point.
    const target = Vec3(1.5, 0, 1);
    final (tx, ty) = expected(target, 800, 600);
    final ray = w.projection.screenRay(tx, ty)!;
    expect(ray.camera.isNull, isFalse);
    expect(ray.direction.length, closeTo(1, 1e-5));
    expect(ray.origin.z, lessThan(6), reason: 'from the near plane, in front of the camera');
    final along = (target - ray.origin).dot(ray.direction);
    expect((ray.at(along) - target).length, lessThan(1e-3));

    // Picking: the ray into the physics scene. The Floor's top is y 0 (a
    // box collider's size is its half extents).
    final floor = w.find('Floor')!;
    final hit = w.projection.pick(tx, ty)!;
    expect(hit.entity, floor.entity);
    expect((hit.point - target).length, lessThan(1e-2));
    expect(w.projection.pick(tx, ty, mask: w.layerMask(['props'])), isNull, reason: 'not on that layer');
    // The sky above the horizon: nothing to pick.
    expect(w.projection.pick(400, 1), isNull);
  });

  test('without a camera there is no ray and no screen point', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.projection.setViewport(800, 600);
    expect(w.projection.screenRay(400, 300), isNull);
    expect(w.projection.worldToScreen(Vec3.zero), isNull);
    // No viewport: a size has to come from somewhere.
    final bare = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(bare.dispose);
    expect(() => bare.projection.screenRay(1, 1), throwsStateError);
  });
}
