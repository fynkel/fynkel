// The Profiler (roadmap Phase 2): the stats windows of a Play parsed and
// kept in a ring, Record / Clear / Clear on Play, the chart's series and
// the selected window's breakdown.
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/play/play_session.dart';
import 'package:fynkel_editor/src/profiler/profiler_model.dart';
import 'package:fynkel_editor/src/profiler/profiler_panel.dart';

/// A `stats` record as `FynkelGameView` sends it (`channelRecord`).
String statsLine(int tick, {double work = 4, int draws = 30, bool gpu = false}) => jsonEncode({
  'fynkel': 'stats',
  'seq': tick,
  'tick': tick,
  'fps': 120.0,
  'frames': 120,
  'ticks': 60,
  'missed': 0,
  'dropped': 0,
  'entities': 12,
  'draws': draws,
  'memNative': 1048576,
  'summary': [
    'frames=120',
    'ticks=60',
    'missed=0',
    'entities=12',
    'interval=8.300/8.400/9.000',
    'simulate=1.000/1.500/2.000',
    'render=2.000/2.500/3.000',
    'behaviours=0.400/0.600/0.800',
    'native=0.500/0.700/0.900',
    'work=${(work - 1).toStringAsFixed(3)}/${work.toStringAsFixed(3)}/${(work + 1).toStringAsFixed(3)}',
    'mem_native=1048576',
    'mem_resident=73400320',
    'draws=$draws',
    'shadow=5/9',
    if (gpu) 'gpu_frame=3.000/3.500/4.000',
  ].join(' '),
});

PlaySession inEditorSession() => PlaySession(projectDir: '/fake/game', cli: () => null);

void main() {
  test('a stats record parses into timers, counts and values', () {
    final f = ProfilerFrame.parse(jsonDecode(statsLine(600, work: 5)) as Map<String, Object?>, index: 0)!;
    expect(f.tick, 600);
    expect(f.timers['work'], (p50: 4.0, p95: 5.0, p99: 6.0));
    expect(f.metric('simulate'), 1.5);
    expect(f.metric('simulate', pct: 'p50'), 1.0);
    expect(f.metric('simulate', pct: 'p99'), 2.0);
    expect(f.values['mem_resident'], 73400320);
    expect((f.values['shadow.0'], f.values['shadow.1']), (5.0, 9.0));
    expect(f.values['fps'], 120);
    expect(f.values.containsKey('seq'), isFalse);
    expect(ProfilerFrame.parse({'fynkel': 'stats'}, index: 0), isNull);
  });

  test('the model keeps each window of a Play: a ring, Record, Clear, Clear on Play, selection', () {
    final session = inEditorSession();
    addTearDown(session.dispose);
    final model = ProfilerModel(session: session, capacity: 3);
    addTearDown(model.dispose);
    expect(model.live, isFalse);

    session.beginInEditor();
    expect(model.live, isTrue);
    for (var i = 1; i <= 4; i++) {
      session.receive(statsLine(i * 60, work: i.toDouble()));
    }
    // A log line is not a window.
    session.receive(jsonEncode({'fynkel': 'log', 'seq': 9, 'message': 'hi'}));
    expect(model.frames.map((f) => f.tick), [120, 180, 240]);
    expect(model.shown!.tick, 240);

    model.select(0);
    expect(model.shown!.tick, 120);
    model.step(5);
    expect(model.selected, 2);
    // A window that pushes the oldest out keeps the selection on its window.
    model.select(1);
    session.receive(statsLine(300));
    expect(model.selected, 0);
    expect(model.shown!.tick, 180);
    model.select(null);
    expect(model.shown!.tick, 300);

    model.setRecording(false);
    session.receive(statsLine(360));
    expect(model.frames.last.tick, 300);
    model.setRecording(true);

    // A new Play starts over; with Clear on Play off it keeps going.
    session.endInEditor();
    session.beginInEditor();
    expect(model.frames, isEmpty);
    session.receive(statsLine(60));
    model.setClearOnPlay(false);
    session.endInEditor();
    session.beginInEditor();
    expect(model.frames, hasLength(1));
    model.clear();
    expect(model.frames, isEmpty);
  });

  testWidgets('the panel: empty, the chart and its legend, a click selects a window, the breakdown', (tester) async {
    final session = inEditorSession();
    addTearDown(session.dispose);
    final model = ProfilerModel(session: session);
    addTearDown(model.dispose);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SizedBox(width: 1000, height: 500, child: ProfilerPanel(model: model)),
        ),
      ),
    );
    expect(find.text('Press Play to profile the game.'), findsOneWidget);

    session.beginInEditor();
    await tester.pump();
    expect(find.text('Waiting for the first stats window…'), findsOneWidget);
    for (var i = 1; i <= 3; i++) {
      session.receive(statsLine(i * 60, work: i * 2.0, draws: 10 * i));
    }
    await tester.pump();
    expect(find.byKey(const Key('profiler.plot')), findsOneWidget);
    expect(tester.widget<Text>(find.byKey(const Key('profiler.legend.work'))).data, '6.00 ms');
    // No GPU timings in these windows: the GPU series is not listed.
    expect(find.byKey(const Key('profiler.legend.gpu_frame')), findsNothing);
    expect(tester.widget<Text>(find.byKey(const Key('profiler.value.work'))).data, '5.00 / 6.00 / 7.00 ms');
    expect(tester.widget<Text>(find.byKey(const Key('profiler.value.mem_resident'))).data, '70.00 MB');
    expect(tester.widget<Text>(find.byKey(const Key('profiler.value.shadow.1'))).data, '9');
    expect(find.textContaining('Playing · 3 s kept'), findsOneWidget);

    // A click at the plot's left edge selects the first window.
    final plot = tester.getRect(find.byKey(const Key('profiler.plot')));
    await tester.tapAt(plot.centerLeft + const Offset(9, 0));
    await tester.pump();
    expect(model.selected, 0);
    expect(tester.widget<Text>(find.byKey(const Key('profiler.legend.work'))).data, '2.00 ms');
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    await tester.pump();
    expect(model.selected, 1);
    await tester.sendKeyEvent(LogicalKeyboardKey.end);
    await tester.pump();
    expect(model.selected, isNull);

    // Other charts, other series; the percentile menu only for timings.
    await tester.ensureVisible(find.byKey(const Key('profiler.chart.rendering')));
    await tester.tap(find.byKey(const Key('profiler.chart.rendering')));
    await tester.pump();
    expect(tester.widget<Text>(find.byKey(const Key('profiler.legend.draws'))).data, '30');
    expect(find.byKey(const Key('profiler.percentile')), findsNothing);
    await tester.ensureVisible(find.byKey(const Key('profiler.chart.memory')));
    await tester.tap(find.byKey(const Key('profiler.chart.memory')));
    await tester.pump();
    expect(tester.widget<Text>(find.byKey(const Key('profiler.legend.mem_resident'))).data, '70.00 MB');

    await tester.ensureVisible(find.byKey(const Key('profiler.clear')));
    await tester.tap(find.byKey(const Key('profiler.clear')));
    await tester.pump();
    expect(model.frames, isEmpty);
    expect(find.text('Waiting for the first stats window…'), findsOneWidget);
  });
}
