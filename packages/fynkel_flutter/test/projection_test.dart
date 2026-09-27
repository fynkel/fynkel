// World labels follow the scene (M3-13 gate, blueprint §19.2d item 13):
// a `WorldLabel` in a `ProjectionLayer` sits where the fixture's Crate A
// is drawn while a scripted camera pans, and leaves the screen when the
// crate is culled — with no widget rebuilt per frame.

import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart' hide Transform;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-flutter-projection-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  /// The fixture's Main scene: the camera at (0, 2, 6), fov 1 rad, looking
  /// down -Z; Crate A at (2, 0.5, 0) (the box is the prefab's root since ADR-0022).
  World loaded() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    w.loadProject(fixture, cacheDir: cache);
    w.frame(1 / 60);
    return w;
  }

  /// Where a world point lands for a camera at [cam] (no rotation) in a
  /// `width` × `height` viewport.
  Offset expected(Vec3 p, Vec3 cam, double width, double height) {
    final depth = cam.z - p.z;
    final halfH = depth * math.tan(0.5);
    final halfW = halfH * width / height;
    return Offset(((p.x - cam.x) / halfW * 0.5 + 0.5) * width, (0.5 - (p.y - cam.y) / halfH * 0.5) * height);
  }

  /// Two ticks so the frame's interpolation shows the new poses.
  void advance(World w) {
    w.frame(1 / 60);
    w.frame(1 / 60);
    w.projection.update();
  }

  testWidgets('a label follows the crate under a scripted camera and leaves when culled', (tester) async {
    final w = loaded();
    final crate = w.find('Crate A')!;
    final camera = w.find('Camera')!;
    var builds = 0;

    await tester.pumpWidget(
      Directionality(
        textDirection: TextDirection.ltr,
        child: Center(
          child: SizedBox(
            width: 400,
            height: 300,
            child: ProjectionLayer(
              world: w,
              children: [
                WorldLabel(target: crate, offset: const Vec3(0, 1, 0), text: 'Crate A'),
                ProjectedAnchor(
                  target: crate,
                  alignment: Alignment.center,
                  child: Builder(
                    builder: (_) {
                      builds++;
                      return const ColoredBox(
                        key: Key('dot'),
                        color: Color(0xFFFF0000),
                        child: SizedBox(width: 10, height: 10),
                      );
                    },
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
    expect(w.projection.length, 2, reason: 'the layer registered its children');
    expect((w.projection.width, w.projection.height), (400.0, 300.0), reason: 'the viewport comes from layout');
    final origin = tester.getTopLeft(find.byType(ProjectionLayer));

    // Frame 1: the crate is where the scene put it.
    advance(w);
    await tester.pump();
    var at = expected(const Vec3(2, 1.5, 0), const Vec3(0, 2, 6), 400, 300);
    var label = tester.getRect(find.byType(WorldLabel));
    expect(label.bottomCenter - origin, offsetMoreOrLessEquals(at, epsilon: 0.05));
    expect(
      tester.getCenter(find.byKey(const Key('dot'))) - origin,
      offsetMoreOrLessEquals(expected(const Vec3(2, 0.5, 0), const Vec3(0, 2, 6), 400, 300), epsilon: 0.05),
    );
    expect(find.text('Crate A').hitTestable(), findsOneWidget);

    // The camera pans right 1 m per frame: the label slides left with the
    // crate, and nothing rebuilds.
    final buildsAfterFirst = builds;
    for (var i = 1; i <= 3; i++) {
      camera.setLocalPosition(i.toDouble(), 2, 6);
      advance(w);
      await tester.pump();
      at = expected(const Vec3(2, 1.5, 0), Vec3(i.toDouble(), 2, 6), 400, 300);
      label = tester.getRect(find.byType(WorldLabel));
      expect(label.bottomCenter - origin, offsetMoreOrLessEquals(at, epsilon: 0.05), reason: 'frame $i');
    }
    expect(builds, buildsAfterFirst, reason: 'positions are paint transforms, not rebuilds');

    // Culled: the camera keeps going until the crate is off the left edge —
    // the label is not painted and cannot be hit.
    camera.setLocalPosition(30, 2, 6);
    advance(w);
    await tester.pump();
    expect(find.text('Crate A').hitTestable(), findsNothing);
    expect(find.byKey(const Key('dot')).hitTestable(), findsNothing);
    // Behind the camera: gone as well.
    camera.setLocalPosition(0, 2, -6);
    advance(w);
    await tester.pump();
    expect(find.text('Crate A').hitTestable(), findsNothing);
    // Back in view: it returns.
    camera.setLocalPosition(0, 2, 6);
    advance(w);
    await tester.pump();
    expect(find.text('Crate A').hitTestable(), findsOneWidget);
    expect(builds, buildsAfterFirst);

    // Removing the layer unregisters its anchors.
    await tester.pumpWidget(const SizedBox());
    expect(w.projection.isEmpty, isTrue);
  });

  testWidgets('a clamped marker stays on the edge, an occluded label hides', (tester) async {
    final w = loaded();
    final crate = w.find('Crate A')!;
    final camera = w.find('Camera')!;
    // A point under the floor, hidden by the floor's layer.
    final below = w.spawn(
      transform: const Transform(position: Vec3(0, -1, 0), rotation: Quat.identity, scale: Vec3(1, 1, 1)),
      name: 'Below',
    );
    w.frame(1 / 60);

    await tester.pumpWidget(
      Directionality(
        textDirection: TextDirection.ltr,
        child: SizedBox(
          width: 400,
          height: 300,
          child: ProjectionLayer(
            world: w,
            children: [
              ProjectedAnchor(
                target: crate,
                alignment: Alignment.center,
                clampToEdges: true,
                child: const ColoredBox(
                  key: Key('marker'),
                  color: Color(0xFFFF0000),
                  child: SizedBox(width: 20, height: 20),
                ),
              ),
              WorldLabel(target: below, text: 'hidden', occlusionMask: w.layerMask(['default'])),
              WorldLabel(target: below, text: 'shown'),
            ],
          ),
        ),
      ),
    );
    advance(w);
    await tester.pump();
    expect(find.text('hidden').hitTestable(), findsNothing);
    expect(find.text('shown').hitTestable(), findsOneWidget);

    // The crate off the left edge: the marker sits on it.
    camera.setLocalPosition(30, 2, 6);
    advance(w);
    await tester.pump();
    final marker = tester.getRect(find.byKey(const Key('marker')));
    expect(marker.left, 0);
    expect(find.byKey(const Key('marker')).hitTestable(), findsOneWidget);
    // Behind the camera even a clamped marker hides.
    camera.setLocalPosition(0, 2, -6);
    advance(w);
    await tester.pump();
    expect(find.byKey(const Key('marker')).hitTestable(), findsNothing);
  });

  testWidgets('changing children keeps the anchors of unchanged targets', (tester) async {
    final w = loaded();
    final a = w.find('Crate A')!;
    final b = w.find('Crate B')!;
    Widget layer(List<ProjectedAnchor> children) => Directionality(
      textDirection: TextDirection.ltr,
      child: SizedBox(
        width: 400,
        height: 300,
        child: ProjectionLayer(world: w, children: children),
      ),
    );
    await tester.pumpWidget(layer([WorldLabel(target: a, text: 'A')]));
    final first = w.projection.anchors.single;
    await tester.pumpWidget(
      layer([WorldLabel(target: a, text: 'A', offset: const Vec3(0, 2, 0)), WorldLabel(target: b, text: 'B')]),
    );
    expect(w.projection.anchors.first, same(first), reason: 'same target: same anchor');
    expect(first.offset, const Vec3(0, 2, 0), reason: 'offset updated in place');
    expect(w.projection.length, 2);
    await tester.pumpWidget(layer([WorldLabel(target: b, text: 'B')]));
    expect(first.isAttached, isFalse);
    expect(w.projection.length, 1);
    await tester.pumpWidget(const SizedBox());
    expect(w.projection.isEmpty, isTrue);
  });

  testWidgets('FynkelGameView updates the projection view every frame', (tester) async {
    final w = loaded();
    final crate = w.find('Crate A')!;
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    var updates = 0;
    w.projection.addListener(() => updates++);
    await tester.pumpWidget(
      MaterialApp(
        home: SizedBox(
          width: 400,
          height: 300,
          child: FynkelGameView(
            world: w,
            controller: controller,
            overlay: ProjectionLayer(
              world: w,
              children: [WorldLabel(target: crate, text: 'Crate A')],
            ),
          ),
        ),
      ),
    );
    for (var i = 0; i < 5; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
    expect(w.projection.version, greaterThanOrEqualTo(5), reason: 'one update per frame');
    expect(updates, greaterThanOrEqualTo(1), reason: 'the label was placed');
    expect(w.projection.anchors.single.visible, isTrue);
    // A still scene does not notify (nothing to repaint); a moved crate does.
    final placed = updates;
    await tester.pump(const Duration(microseconds: 16667));
    await tester.pump(const Duration(microseconds: 16667));
    expect(updates, placed);
    crate.setLocalPosition(3, 0, 0);
    for (var i = 0; i < 3; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
    expect(updates, greaterThan(placed));
  });
}
