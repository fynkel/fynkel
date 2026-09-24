// Unity parity: the material Inspector, driven through the whole shell
// over a real project (docs/unity-parity.md § Tier 2).
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const rusted = 'c0000000-0000-4000-8000-000000000020';

void main() {
  Future<Editor> onRusted(WidgetTester tester) async {
    final e = await Editor.open(tester);
    e.shell.browser!.setFolder('materials');
    await tester.pumpAndSettle();
    await e.click(find.byKey(const Key('project.asset.$rusted')));
    return e;
  }

  Map<String, Object?> saved(Editor e) =>
      jsonDecode(File('${e.dir.path}/assets/materials/rusted.material.json').readAsStringSync())
          as Map<String, Object?>;

  /// What the Scene view draws for the material now.
  Map<String, Object?> drawn(Editor e) => e.shell.scene!.current!.preview!.material(rusted);

  /// Scrolls [f] to the middle of the Inspector (the top edge is under the page's header).
  Future<void> reveal(WidgetTester tester, Finder f) async {
    await Scrollable.ensureVisible(tester.element(f), alignment: 0.5);
    await tester.pumpAndSettle();
  }

  Finder inRow(String field, Finder f) => find.descendant(of: find.byKey(Key('material.$field')), matching: f);

  testWidgets('a material is edited in the Inspector: saved at once, drawn in the Scene view, ⌘Z undoes it', (
    tester,
  ) async {
    final e = await onRusted(tester);
    final scene = e.shell.tabs!.focused!;

    // Metallic typed in: one change, written, drawn.
    final metallic = inRow('metallic', find.byKey(const ValueKey('number.text')));
    await reveal(tester, metallic);
    await e.click(metallic);
    await e.type('0.8');
    await e.submit();
    expect(saved(e)['metallic'], 0.8, reason: 'saved at once, as Unity writes a material');
    expect(drawn(e)['metallic'], closeTo(0.8, 1e-6), reason: 'the Scene view draws it');

    // The slider is live: the world follows the thumb, the file waits for the release.
    final slider = inRow('roughness', find.byType(Slider));
    await reveal(tester, slider);
    final g = await tester.startGesture(tester.getCenter(slider));
    await tester.pump();
    await g.moveBy(Offset(tester.getSize(slider).width / 3, 0));
    await tester.pump();
    final during = (drawn(e)['roughness'] as num).toDouble();
    expect(during, greaterThan(0.7), reason: 'drawn while dragging');
    expect(saved(e)['roughness'], 0.65, reason: 'nothing written while dragging');
    await g.up();
    await tester.pumpAndSettle();
    expect((saved(e)['roughness'] as num).toDouble(), closeTo(during, 1e-6));

    // Rendering Mode → Cutout: the cutoff appears.
    expect(find.byKey(const Key('material.alpha_cutoff')), findsNothing);
    await reveal(tester, find.byKey(const Key('material.alpha_mode')));
    await e.click(inRow('alpha_mode', find.text('Opaque')));
    await e.click(find.text('Cutout').last);
    expect(saved(e)['alpha_mode'], 'mask');
    expect(find.byKey(const Key('material.alpha_cutoff')), findsOneWidget);

    // A map cleared: None, unbound in the file and the picture.
    final clear = inRow('base_color_texture', find.byTooltip('Clear'));
    await reveal(tester, clear);
    await e.click(clear);
    expect(saved(e)['base_color_texture'], isNull);
    expect(drawn(e)['base_color_texture'], isNull);

    // ⌘Z in the Inspector undoes the material, step by step; the scene is untouched.
    for (final undone in [
      () => saved(e)['base_color_texture'] != null,
      () => saved(e)['alpha_mode'] == null || saved(e)['alpha_mode'] == 'opaque',
      () => saved(e)['roughness'] == 0.65,
      () => saved(e)['metallic'] == 0.25,
    ]) {
      await e.press(LogicalKeyboardKey.keyZ, command: true);
      expect(undone(), isTrue);
    }
    expect(drawn(e)['metallic'], closeTo(0.25, 1e-6));
    expect(scene.document.history, 0, reason: 'the scene had nothing to undo');
    await e.press(LogicalKeyboardKey.keyZ, command: true, shift: true);
    expect(saved(e)['metallic'], 0.8, reason: '⇧⌘Z redoes');
  });

  testWidgets('the Base Color picker lights the Scene view while picking; Cancel writes nothing, OK writes once', (
    tester,
  ) async {
    final e = await onRusted(tester);
    final before = saved(e)['base_color'];
    final swatch = inRow('base_color', find.byKey(const ValueKey('color.swatch')));
    Future<void> pick() async {
      await reveal(tester, swatch);
      await e.click(swatch);
      final sv = find.byKey(const ValueKey('picker.sv'));
      final g = await tester.startGesture(tester.getTopLeft(sv) + const Offset(4, 4));
      await tester.pump();
      await g.moveTo(tester.getBottomRight(sv) - const Offset(4, 60));
      await tester.pump();
      await g.up();
      await tester.pump();
    }

    await pick();
    final during = drawn(e)['base_color'];
    expect(during, isNot(before), reason: 'the world follows the picker');
    expect(saved(e)['base_color'], before);

    await e.click(find.text('Cancel'));
    expect(drawn(e)['base_color'], before);
    expect(saved(e)['base_color'], before);

    await pick();
    await e.click(find.text('OK'));
    final after = (saved(e)['base_color'] as List).cast<num>();
    for (var i = 0; i < 4; i++) {
      expect(after[i], closeTo((during as List)[i] as num, 1e-4));
    }
  });
}
