// The one picker (item 9d) on its own: groups, the word filter and the
// scored ranking, the keyboard, disabled rows, the None row, the preview of
// the row under the cursor, and the count in the footer.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/picker/picker.dart';

const fruit = [
  PickerItem(value: 'apple', title: 'Apple', group: 'Trees', detail: 'red or green', description: 'Crunchy.'),
  PickerItem(value: 'pear', title: 'Pear', group: 'Trees', keywords: 'conference'),
  PickerItem(value: 'melon', title: 'Melon', group: 'Vines', enabled: false, note: 'out of season'),
  PickerItem(value: 'grape', title: 'Grape', group: 'Vines', facts: [('Colour', 'purple')]),
];

/// Pumps a button that opens the picker; the list gets what it returned.
Future<List<PickerChoice<String>?>> pumpPicker(
  WidgetTester tester, {
  bool allowNone = false,
  String? current,
  PickerScore<String>? score,
}) async {
  final results = <PickerChoice<String>?>[];
  await tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: Builder(
          builder: (context) => TextButton(
            onPressed: () async => results.add(
              await showPicker<String>(
                context,
                title: 'Choose a fruit',
                subject: 'Lunch',
                items: fruit,
                allowNone: allowNone,
                current: current,
                keyPrefix: 'fruit',
                countNoun: 'fruits',
                empty: 'No fruit matches.',
                score: score,
              ),
            ),
            child: const Text('open'),
          ),
        ),
      ),
    ),
  );
  await tester.tap(find.text('open'));
  await tester.pumpAndSettle();
  return results;
}

void main() {
  testWidgets('groups, the title for its subject, the count, and ⏎ on the first row', (tester) async {
    final results = await pumpPicker(tester);
    expect(find.byKey(const Key('fruit')), findsOneWidget);
    expect(find.textContaining('for Lunch'), findsOneWidget);
    expect(find.text('TREES'), findsOneWidget);
    expect(find.text('VINES'), findsOneWidget);
    expect(tester.widget<Text>(find.byKey(const Key('fruit.count'))).data, '4 of 4 fruits');
    expect(find.byKey(const Key('fruit.note.2')), findsOneWidget, reason: 'the held note shows on its row');
    // The row under the cursor explains itself.
    expect(
      find.descendant(of: find.byKey(const Key('fruit.preview')), matching: find.text('Crunchy.')),
      findsOneWidget,
    );
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(results.single!.value, 'apple');
    expect(find.byKey(const Key('fruit')), findsNothing);
  });

  testWidgets('words filter titles, details, groups and keywords; nothing matching says so', (tester) async {
    final results = await pumpPicker(tester);
    await tester.enterText(find.byKey(const Key('fruit.search')), 'conference');
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('fruit.row.1')), findsOneWidget);
    expect(find.byKey(const Key('fruit.row.0')), findsNothing);
    expect(tester.widget<Text>(find.byKey(const Key('fruit.count'))).data, '1 of 4 fruits');
    await tester.enterText(find.byKey(const Key('fruit.search')), 'vines gr');
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('fruit.row.3')), findsOneWidget);
    expect(find.text('VINES'), findsOneWidget, reason: 'a filtered list keeps its groups');
    await tester.enterText(find.byKey(const Key('fruit.search')), 'kiwi');
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('fruit.empty')), findsOneWidget);
    expect(find.text('No fruit matches.'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('fruit')), findsOneWidget, reason: '⏎ on no rows does nothing');
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    expect(results, [null], reason: 'esc cancels');
  });

  testWidgets('↑↓ skip the headers; a disabled row cannot be chosen by key or click', (tester) async {
    final results = await pumpPicker(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pumpAndSettle();
    expect(find.descendant(of: find.byKey(const Key('fruit.preview')), matching: find.text('Melon')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.tap(find.byKey(const Key('fruit.row.2')));
    await tester.pumpAndSettle();
    expect(results, isEmpty);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pumpAndSettle();
    expect(find.descendant(of: find.byKey(const Key('fruit.preview')), matching: find.text('purple')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(results.single!.value, 'grape', reason: 'the cursor stops at the last row');
  });

  testWidgets('the None row clears; the cursor starts on what the field holds', (tester) async {
    final results = await pumpPicker(tester, allowNone: true, current: 'pear');
    expect(find.byKey(const Key('fruit.none')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(results.single!.value, 'pear');
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('fruit.none')));
    await tester.pumpAndSettle();
    expect(results.last!.isNone, isTrue);
    expect(results.last!.value, isNull);
  });

  testWidgets('a score ranks one flat list, ties in list order', (tester) async {
    final results = await pumpPicker(
      tester,
      score: (item, q) => item.title.toLowerCase().contains(q) ? (item.title.length < 5 ? 2 : 1) : 0,
    );
    await tester.enterText(find.byKey(const Key('fruit.search')), 'e');
    await tester.pumpAndSettle();
    expect(find.text('TREES'), findsNothing, reason: 'a ranking has no groups');
    final order = [
      for (final k in ['fruit.row.0', 'fruit.row.1', 'fruit.row.2', 'fruit.row.3'])
        (k, tester.getTopLeft(find.byKey(Key(k))).dy),
    ]..sort((a, b) => a.$2.compareTo(b.$2));
    expect(order.map((o) => o.$1), ['fruit.row.1', 'fruit.row.0', 'fruit.row.2', 'fruit.row.3']);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(results.single!.value, 'pear');
  });
}
