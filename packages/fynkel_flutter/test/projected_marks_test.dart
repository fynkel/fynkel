// Many marks over objects (ADR-0062 amendment 1, step 2): health bars and
// dots drawn by one painter at their objects' projected points — no widget
// per mark, nothing rebuilt when the camera or a value moves.

import 'dart:io';

import 'package:flutter/material.dart' hide Transform;
import 'package:flutter/rendering.dart' show RenderCustomPaint;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-flutter-marks-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  /// The fixture's Main scene (camera at (0, 2, 6) looking down -Z) and a
  /// 25 × 20 crowd in front of it.
  (World, List<GameObject>) crowd() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    w.loadProject(fixture, cacheDir: cache);
    final objects = w.edit(
      () => [
        for (var i = 0; i < 25; i++)
          for (var j = 0; j < 20; j++) w.spawn(transform: Pose(position: Vec3(-6 + i * 0.5, 0, -2 - j * 0.5))),
      ],
    );
    w.frame(1 / 60);
    w.frame(1 / 60);
    return (w, objects);
  }

  Widget host(Widget child) => Directionality(
    textDirection: TextDirection.ltr,
    child: Align(
      alignment: Alignment.topLeft,
      child: SizedBox(width: 800, height: 600, child: child),
    ),
  );

  testWidgets('500 health bars: one painter, at their points, updated without rebuilds', (tester) async {
    final (w, objects) = crowd();
    final bars = WorldMarks(
      w,
      style: const MarkStyle.bar(size: Size(40, 6), color: Color(0xFFE53935), background: Color(0x99000000)),
      offset: const Vec3(0, 1, 0),
    );
    addTearDown(bars.dispose);
    final marks = [for (final o in objects) bars.add(o)];
    expect(w.projection.length, 500, reason: 'one anchor per mark');
    var builds = 0;
    await tester.pumpWidget(
      host(
        Builder(
          builder: (context) {
            builds++;
            return ProjectedMarks(marks: bars);
          },
        ),
      ),
    );
    final watch = Stopwatch()..start();
    w.projection.update();
    final projectMs = watch.elapsedMicroseconds / 1000;
    await tester.pump();

    final visible = marks.where((m) => m.anchor.visible).toList();
    expect(visible.length, greaterThan(300), reason: 'most of the crowd is in view');
    // Two rounded rectangles per visible bar (background, fill), nothing
    // for the rest.
    final layer = find.byType(ProjectedMarks);
    expect(layer, paintsExactlyCountTimes(#drawRRect, visible.length * 2));
    // A bar sits centred over its point, its bottom on it.
    final m = visible.first;
    final rect = Rect.fromLTWH(m.anchor.x - 20, m.anchor.y - 6, 40, 6);
    expect(layer, paints..rrect(rrect: RRect.fromRectAndRadius(rect, const Radius.circular(1.5))));

    // A value changes: that bar's fill shrinks, nothing rebuilds.
    m.value = 0.25;
    await tester.pump();
    expect(
      layer,
      paints
        ..rrect(rrect: RRect.fromRectAndRadius(rect, const Radius.circular(1.5)))
        ..rrect(rrect: RRect.fromRectAndRadius(Rect.fromLTWH(rect.left, rect.top, 10, 6), const Radius.circular(1.5))),
    );

    // The crowd moves: the bars follow on the next update, nothing rebuilds.
    w.edit(() {
      for (final o in objects) {
        o.transform.position = o.transform.position + const Vec3(0.5, 0, 0);
      }
    });
    w.frame(1 / 60);
    final before = m.anchor.x;
    w.projection.update();
    await tester.pump();
    expect(m.anchor.x, greaterThan(before));
    final first = marks.firstWhere((x) => x.anchor.visible);
    expect(
      layer,
      paints..rrect(
        rrect: RRect.fromRectAndRadius(
          Rect.fromLTWH(first.anchor.x - 20, first.anchor.y - 6, 40, 6),
          const Radius.circular(1.5),
        ),
      ),
    );
    expect(builds, 1, reason: 'values and positions repaint; they never rebuild');

    // A destroyed object's bar is not drawn; remove() drops its anchor.
    final gone = marks.lastWhere((x) => x.anchor.visible);
    w.edit(() => (gone.target as GameObject).destroy());
    w.frame(1 / 60);
    w.projection.update();
    await tester.pump();
    expect(gone.anchor.visible, isFalse);
    final still = marks.where((x) => x.anchor.visible).length;
    expect(layer, paintsExactlyCountTimes(#drawRRect, still * 2));
    gone.remove();
    expect(w.projection.length, 499);
    expect(bars.length, 499);

    final warm = Stopwatch()..start();
    for (var i = 0; i < 50; i++) {
      w.projection.update();
    }
    // ignore: avoid_print
    print(
      '[bench] 500 marks, projection update: first ${projectMs.toStringAsFixed(2)} ms, '
      'warm ${(warm.elapsedMicroseconds / 50 / 1000).toStringAsFixed(3)} ms (JIT, test host)',
    );
  });

  testWidgets('dots, per-mark colours, hidden marks and a distance cut-off', (tester) async {
    final (w, objects) = crowd();
    // The camera is at z 6, the crowd from z -2 back: 10 m reaches its
    // first rows.
    final dots = WorldMarks(w, style: const MarkStyle.dot(radius: 3, color: Color(0xFF42A5F5), maxDistance: 10));
    addTearDown(dots.dispose);
    final marks = [for (final o in objects) dots.add(o)];
    await tester.pumpWidget(host(ProjectedMarks(marks: dots)));
    w.projection.update();
    await tester.pump();
    final near = marks.where((m) => m.anchor.visible && m.anchor.depth <= 10).toList();
    final far = marks.where((m) => m.anchor.visible && m.anchor.depth > 10).length;
    expect(near.length, greaterThan(2));
    expect(far, greaterThan(0), reason: 'the crowd reaches past 10 m');
    near[0].color = const Color(0xFFFFEB3B);
    near[1].hidden = true;
    await tester.pump();
    final layer = find.byType(ProjectedMarks);
    expect(layer, paintsExactlyCountTimes(#drawCircle, near.length - 1));
    expect(layer, paints..circle(x: near[0].anchor.x, y: near[0].anchor.y, radius: 3, color: const Color(0xFFFFEB3B)));
    // Taps go through to the game.
    expect(
      tester
          .hitTestOnBinding(Offset(near[2].anchor.x, near[2].anchor.y))
          .path
          .any((e) => e.target is RenderCustomPaint),
      isFalse,
    );
  });
}
