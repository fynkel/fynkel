// Projected world anchors (M3-13, blueprint §10, docs/hud.md): the
// `World.projection` view maps entity-attached points into a viewport with
// the frame's camera — one FFI call per update, bounded to 64 anchors, the
// explicit occlusion policy by collider mask, the FYN-R0012 diagnostic.

import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
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

    final (x, y) = expected(const Vec3(2, 0, 0), 800, 600);
    expect(anchor.x, closeTo(x, 0.01));
    expect(anchor.y, closeTo(y, 0.01));
    expect(anchor.depth, closeTo(6, 1e-4));
    expect(anchor.inFront, isTrue);
    expect(anchor.onScreen, isTrue);
    expect(anchor.visible, isTrue);
    expect(anchor.occluded, isFalse);
    expect(anchor.missing, isFalse);
    final (tx, ty) = expected(const Vec3(2, 1.5, 0), 800, 600);
    expect(top.x, closeTo(tx, 0.01));
    expect(top.y, closeTo(ty, 0.01));
    expect(top.y, lessThan(anchor.y), reason: 'above the crate on screen');

    // The crate moves right and up: the label goes with it (the frame
    // interpolates between the last two ticks, so the move is fully on
    // screen one tick after it applied — like the rendered mesh).
    crate.setLocalPosition(3, 1, 0);
    w.frame(1 / 60);
    w.projection.update();
    expect(anchor.x, closeTo(x, 0.01), reason: 'the tick before the move is what this frame shows');
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
    crate.setLocalPosition(2, 0, 0);
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
      transform: const Transform(position: Vec3(0, -1, 0), rotation: Quat.identity, scale: Vec3(1, 1, 1)),
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
    expect(anchor.missing, isTrue, reason: 'not spawned yet');
    w.projection.setViewport(800, 600);
    expect(w.projection.update(), 1);
    expect(anchor.missing, isTrue);
    expect(anchor.visible, isFalse);
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
    final bogus = w.projection.add(0x7fff0000ffff);
    w.projection.update();
    expect(byHandle.visible, isTrue);
    expect(bogus.missing, isTrue);
  });

  test('the view is bounded to 64 anchors and says so once', () {
    final w = loaded();
    w.throwOnDiagnostics = false;
    final diagnostics = <FynkelDiagnostic>[];
    w.onDiagnostics = diagnostics.addAll;
    final crate = w.find('Crate A')!;
    final anchors = List.generate(65, (_) => w.projection.add(crate));
    w.projection.setViewport(800, 600);
    expect(w.projection.update(), 64);
    expect(anchors[63].visible, isTrue);
    expect(anchors[64].dropped, isTrue);
    expect(anchors[64].visible, isFalse);
    expect(diagnostics, hasLength(1));
    expect(diagnostics.single.code, DiagnosticCode.tooManyAnchors);
    expect(diagnostics.single.isInformational, isTrue);
    expect(diagnostics.single.message, startsWith('FYN-R0012'));
    // Reported once per overflow episode, not per frame.
    w.projection.update();
    expect(diagnostics, hasLength(1));
    // Back under the bound the episode ends; a new overflow reports again.
    anchors[64].remove();
    expect(w.projection.update(), 64);
    expect(anchors[64].isAttached, isFalse);
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
    expect(runs, 1, reason: 'the frame interpolates from the tick before the move: still');
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
}
