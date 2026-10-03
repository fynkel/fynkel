// HUD model binding (M3-13, ADR-0008): a widget bound to a `ModelValue`
// rebuilds when the value changes and only then.

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/gameplay.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

class HudModel extends GameModel {
  late final score = field(ModelValue(0));
  late final paused = field(ModelValue(false));
}

void main() {
  testWidgets('ModelBuilder rebuilds on change only', (tester) async {
    final m = HudModel();
    var builds = 0;
    await tester.pumpWidget(
      MaterialApp(
        home: ModelBinding<HudModel>(
          model: m,
          child: Builder(
            builder: (context) {
              final hud = ModelBinding.of<HudModel>(context);
              return ModelBuilder<int>(
                value: hud.score,
                child: const Text('static'),
                builder: (_, score, child) {
                  builds++;
                  return Column(children: [Text('score $score'), child!]);
                },
              );
            },
          ),
        ),
      ),
    );
    expect(find.text('score 0'), findsOneWidget);
    expect(find.text('static'), findsOneWidget);
    expect(builds, 1);
    m.score.value = 0;
    await tester.pump();
    expect(builds, 1, reason: 'same value: nothing');
    m.score.value = 7;
    m.score.value = 8;
    await tester.pump();
    expect(find.text('score 8'), findsOneWidget);
    expect(builds, 2, reason: 'two writes in one frame: one rebuild');
    expect(m.score.hasListeners, isTrue);
    await tester.pumpWidget(const SizedBox());
    expect(m.score.hasListeners, isFalse, reason: 'unsubscribed on dispose');
  });

  testWidgets('ModelBuilder follows a swapped field', (tester) async {
    final a = ModelValue('a');
    final b = ModelValue('b');
    Widget bound(ModelValue<String> v) => MaterialApp(
      home: ModelBuilder<String>(value: v, builder: (_, s, _) => Text(s)),
    );
    await tester.pumpWidget(bound(a));
    expect(find.text('a'), findsOneWidget);
    await tester.pumpWidget(bound(b));
    expect(find.text('b'), findsOneWidget);
    expect(a.hasListeners, isFalse);
    a.value = 'a2';
    b.value = 'b2';
    await tester.pump();
    expect(find.text('b2'), findsOneWidget);
  });

  testWidgets('a field is a ValueListenable', (tester) async {
    final m = HudModel();
    await tester.pumpWidget(
      MaterialApp(
        home: ValueListenableBuilder<bool>(
          valueListenable: m.paused.listenable,
          builder: (_, paused, _) => Text(paused ? 'paused' : 'running'),
        ),
      ),
    );
    expect(find.text('running'), findsOneWidget);
    m.paused.value = true;
    await tester.pump();
    expect(find.text('paused'), findsOneWidget);
    expect(m.paused.listenable, equals(m.paused.listenable));
    await tester.pumpWidget(const SizedBox());
    expect(m.paused.hasListeners, isFalse);
  });

  testWidgets('ModelBinding.of finds the model; maybeOf is null without one', (tester) async {
    final m = HudModel();
    HudModel? seen;
    HudModel? none;
    await tester.pumpWidget(
      ModelBinding<HudModel>(
        model: m,
        child: Builder(
          builder: (context) {
            seen = ModelBinding.of<HudModel>(context);
            return const SizedBox();
          },
        ),
      ),
    );
    expect(seen, same(m));
    await tester.pumpWidget(
      Builder(
        builder: (context) {
          none = ModelBinding.maybeOf<HudModel>(context);
          return const SizedBox();
        },
      ),
    );
    expect(none, isNull);
  });
}
