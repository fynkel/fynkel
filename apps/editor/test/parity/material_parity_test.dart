// Unity parity: the material Inspector, driven through the whole shell
// over a real project (docs/unity-parity.md § Tier 2).
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const rusted = 'c0000000-0000-4000-8000-000000000020';
const robot = 'c0000000-0000-4000-8000-0000000000e1';
const crateA = 'c0000000-0000-4000-8000-0000000000e2';

void main() {
  Future<Editor> onRusted(WidgetTester tester) async {
    final e = await Editor.open(tester);
    e.shell.browser!.setFolder('materials');
    await settleCooks(tester);
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
    await settleCooks(tester);
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
    await settleCooks(tester);
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

  /// A second material, Blue, shown in the Project's materials folder.
  Future<(Editor, String)> withBlue(WidgetTester tester) async {
    final e = await Editor.open(tester);
    final blue = e.shell.project!.newDocument('material', 'materials/blue.material.json', name: 'Blue').id!;
    e.shell.browser!.setFolder('materials');
    await settleCooks(tester);
    return (e, blue);
  }

  Map<String, Object?> entity(Editor e, String id) =>
      (e.shell.tabs!.focused!.document.root()['entities'] as Map)[id] as Map<String, Object?>;

  Object? robotMaterial(Editor e) => [
    for (final c in (entity(e, robot)['components'] as Map).values)
      if ((c as Map)['type'] == 'fynkel.MeshRenderer') (c['data'] as Map)['material'],
  ].single;

  List<Object?> crateOverrides(Editor e) => [
    for (final o in ((entity(e, crateA)['prefab'] as Map)['overrides'] as List))
      if ((o as Map)['field'] == 'material') o['value'],
  ];

  testWidgets('a material dropped on a Hierarchy row goes onto its MeshRenderer — an override on a prefab instance', (
    tester,
  ) async {
    final (e, blue) = await withBlue(tester);
    final row = find.byKey(Key('project.asset.$blue'));
    await e.drag(row, tester.getCenter(e.row('Robot')));
    expect(robotMaterial(e), {'asset': blue});
    expect(e.selectedNames, isEmpty, reason: 'the selection stays');

    await e.drag(row, tester.getCenter(e.row('Crate A')));
    expect(crateOverrides(e), [
      {'asset': blue},
    ], reason: 'the instance keeps its prefab, with an override');

    // Not onto something without a renderer: the Sun keeps its components.
    final sunBefore = entity(e, 'c0000000-0000-4000-8000-0000000000e3');
    await e.drag(row, tester.getCenter(e.row('Sun')));
    expect(entity(e, 'c0000000-0000-4000-8000-0000000000e3'), sunBefore);
    expect(e.shell.tabs!.focused!.document.history, 2);
  });

  testWidgets('a material dragged over the Scene view draws on the object under the pointer; the drop assigns it', (
    tester,
  ) async {
    final (e, blue) = await withBlue(tester);
    final sv = e.shell.scene!;
    await e.click(e.row('Robot'));
    sv.frameSelected();
    await settleCooks(tester);
    final world = sv.current!.preview!;
    final m = world.worldMatrices([robot])[robot]!;
    final px = sv.viewProjection!.project([m[12], m[13], m[14]])!;
    final picture = find.byWidgetPredicate((w) => w.key is GlobalKey && '${w.key}'.contains('scene.picture'));
    final at = tester.getTopLeft(picture) + Offset(px[0], px[1]);

    Object? drawnMaterial() {
      final entities = (world.dump()['entities'] as List).cast<Map<String, Object?>>();
      final comps = (entities.singleWhere((x) => x['id'] == robot)['components'] as List).cast<Map<String, Object?>>();
      return (comps.singleWhere((c) => c['type'] == 'fynkel.MeshRenderer')['data'] as Map)['material'];
    }

    // Hovering: drawn with Blue, nothing written.
    final g = await tester.startGesture(tester.getCenter(find.byKey(Key('project.asset.$blue'))));
    await tester.pump(const Duration(milliseconds: 50));
    await g.moveBy(const Offset(20, 0));
    await tester.pump(const Duration(milliseconds: 50));
    await g.moveTo(at);
    await tester.pump(const Duration(milliseconds: 50));
    await g.moveTo(at + const Offset(1, 0));
    await tester.pump(const Duration(milliseconds: 50));
    expect((drawnMaterial() as Map)['asset'], blue, reason: 'the hover preview');
    expect(robotMaterial(e), {'asset': rusted});

    await g.up();
    await settleCooks(tester);
    expect(robotMaterial(e), {'asset': blue});
    expect(e.shell.tabs!.focused!.document.history, 1, reason: 'one undo step');
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(robotMaterial(e), {'asset': rusted});
    expect((drawnMaterial() as Map)['asset'], rusted);
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
