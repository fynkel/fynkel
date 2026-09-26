// The colour picker as Unity's (docs/ui-refresh): the hue ring, the
// channel sliders and numbers in three modes, the hexadecimal, the default
// palette, the user's swatches, and the old colour to go back to.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/inspector/field_widgets.dart';
import 'package:fynkel_editor/src/inspector/inspector_math.dart';
import 'package:fynkel_editor/src/theme/theme.dart';

void main() {
  late List<List<double>> changes;
  late List<String> saved;

  setUp(() {
    changes = [];
    saved = [];
    ColorSwatches.read = () => saved;
    ColorSwatches.write = (hexes) => saved = hexes;
    ColorPickerDialog.mode = ColorPickerMode.rgb255;
    ColorPickerDialog.defaultsOpen = true;
    ColorPickerDialog.swatchesOpen = true;
  });

  Future<void> pump(WidgetTester tester, List<double> initial, {bool alpha = true}) async {
    tester.view.physicalSize = const Size(900, 1200);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    await tester.pumpWidget(
      MaterialApp(
        theme: fynkelTheme(brightness: Brightness.dark),
        home: Scaffold(
          body: ColorPickerDialog(initial: initial, alpha: alpha, onChanged: changes.add),
        ),
      ),
    );
  }

  String hex() => linearRgbToHex(changes.last);

  Future<void> type(WidgetTester tester, String key, String text) async {
    await tester.enterText(find.byKey(ValueKey(key)), text);
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
  }

  testWidgets('the ring sets the hue; the numbers and modes read the same colour', (tester) async {
    await pump(tester, [1, 0, 0, 1]);
    // The ring's left side is cyan (hue 180), counter-clockwise from red.
    final ring = tester.getRect(find.byKey(const ValueKey('picker.hue')));
    await tester.tapAt(Offset(ring.left + 6, ring.center.dy));
    await tester.pumpAndSettle();
    expect(hex(), '#00FFFFFF');

    // RGB 0–255: G to 128.
    await type(tester, 'picker.value.g', '128');
    expect(hex(), '#0080FFFF');
    // RGB 0–1: the same colour, as fractions.
    await tester.tap(find.byKey(const ValueKey('picker.mode')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('picker.mode.rgb1')));
    await tester.pumpAndSettle();
    expect(find.descendant(of: find.byKey(const ValueKey('picker.value.b')), matching: find.text('1.000')), findsOneWidget);
    // HSV: V to 50 halves the brightness; the hue stays.
    await tester.tap(find.byKey(const ValueKey('picker.mode')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('picker.mode.hsv')));
    await tester.pumpAndSettle();
    await type(tester, 'picker.value.v', '50');
    expect(hex(), '#004080FF');
    // A to 0.
    await type(tester, 'picker.value.a', '0');
    expect(changes.last[3], 0);
  });

  testWidgets('a grey keeps the hue; a default, a swatch and the old colour', (tester) async {
    await pump(tester, [0, 0, 1, 1]);
    // Down to white on the square's top-left and back: the ring stays blue.
    final sv = tester.getRect(find.byKey(const ValueKey('picker.sv')));
    await tester.tapAt(sv.topLeft + const Offset(1, 1));
    await tester.pumpAndSettle();
    await tester.tapAt(sv.topRight + const Offset(-1, 1));
    await tester.pumpAndSettle();
    final (h, _, _) = rgbToHsv(linearToSrgb(changes.last[0]), linearToSrgb(changes.last[1]), linearToSrgb(changes.last[2]));
    expect(h, closeTo(240, 1), reason: 'white did not reset the hue to red');

    // The palette's first colour is red.
    await tester.tap(find.byKey(const ValueKey('picker.default.0')));
    await tester.pumpAndSettle();
    expect(hex(), '#FF0000FF');

    // + keeps the current colour; a click picks it; right-click removes it.
    await tester.tap(find.byKey(const ValueKey('picker.swatch.add')));
    await tester.pumpAndSettle();
    expect(saved, ['#FF0000FF']);
    await type(tester, 'picker.hex', '#00FF00');
    expect(hex(), '#00FF00FF');
    await tester.tap(find.byKey(const ValueKey('picker.swatch.0')));
    await tester.pumpAndSettle();
    expect(hex(), '#FF0000FF');
    await tester.tap(find.byKey(const ValueKey('picker.swatch.0')), buttons: kSecondaryButton);
    await tester.pumpAndSettle();
    expect(saved, isEmpty);

    // The old half puts the starting colour back.
    await tester.tap(find.byKey(const ValueKey('picker.old')));
    await tester.pumpAndSettle();
    expect(hex(), '#0000FFFF');

    // The folds remember themselves.
    await tester.tap(find.byKey(const ValueKey('picker.defaults')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('picker.default.0')), findsNothing);
    expect(ColorPickerDialog.defaultsOpen, isFalse);
  });
}
