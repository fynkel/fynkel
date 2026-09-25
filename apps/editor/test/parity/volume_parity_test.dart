// Unity parity: the Volume Profile Inspector — Add Override, a checkbox
// per field — driven through the whole shell over a real project, and the
// saved scene rendered by the real `fynkel render` to see the bloom
// (docs/unity-parity.md § Tier 2).
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/inspector/field_widgets.dart';
import 'package:fynkel_editor/src/scene_view/png.dart';

import 'harness.dart';

const sun = 'c0000000-0000-4000-8000-0000000000e3';

void main() {
  /// A new profile, Look, shown in the Inspector.
  Future<(Editor, String)> onLook(WidgetTester tester) async {
    final e = await Editor.open(tester);
    final look = e.shell.project!.newDocument('volume', 'volumes/look.volume.json', name: 'Look').id!;
    e.shell.browser!.setFolder('volumes');
    await tester.pumpAndSettle();
    await e.click(find.byKey(Key('project.asset.$look')));
    return (e, look);
  }

  Map<String, Object?> saved(Editor e) =>
      jsonDecode(File('${e.dir.path}/assets/volumes/look.volume.json').readAsStringSync()) as Map<String, Object?>;

  Future<void> reveal(WidgetTester tester, Finder f) async {
    await Scrollable.ensureVisible(tester.element(f), alignment: 0.5);
    await tester.pumpAndSettle();
  }

  Finder inRow(String field, Finder f) => find.descendant(of: find.byKey(Key('volume.$field')), matching: f);
  Finder tick(String field) => find.byKey(Key('volume.$field.override'));
  bool ticked(WidgetTester tester, String field) => tester.widget<Checkbox>(tick(field)).value == true;

  /// Types [text] into [field]'s number box.
  Future<void> typeInto(Editor e, String field, String text) async {
    final box = inRow(field, find.byKey(const ValueKey('number.text')));
    await reveal(e.tester, box);
    await e.click(box);
    await e.type(text);
    await e.submit();
  }

  testWidgets('a profile edited in the Inspector: a checkbox per field, Add Override, Remove, ⌘Z', (tester) async {
    final (e, look) = await onLook(tester);
    final scene = e.shell.tabs!.focused!;
    final drawn = e.shell.scene!.current!.preview!;

    // A new profile holds both overrides, every field ticked.
    expect(find.byKey(const Key('volume.ambientOcclusion')), findsOneWidget);
    expect(find.byKey(const Key('volume.bloom')), findsOneWidget);
    expect(ticked(tester, 'bloom.threshold'), isTrue);

    // Untick Threshold: inherited, left out of the file, greyed at the default.
    await reveal(tester, tick('bloom.threshold'));
    await e.click(tick('bloom.threshold'));
    expect((saved(e)['bloom'] as Map).containsKey('threshold'), isFalse);
    expect(ticked(tester, 'bloom.threshold'), isFalse);
    final threshold = tester.widget<NumberField>(inRow('bloom.threshold', find.byType(NumberField)));
    expect((threshold.enabled, threshold.value), (false, 0.9));

    // Intensity typed: saved and drawn at once.
    await typeInto(e, 'bloom.intensity', '2');
    expect((saved(e)['bloom'] as Map)['intensity'], 2);
    final bloom = drawn.volume(look)['bloom'] as Map;
    expect(bloom['intensity'], 2.0, reason: 'the Scene view blends the edit');
    expect(bloom.containsKey('threshold'), isFalse, reason: 'and inherits the threshold');

    // Remove the Ambient Occlusion override; Add Override brings it back empty.
    await reveal(tester, find.byKey(const Key('volume.ambientOcclusion.menu')));
    await e.click(find.byKey(const Key('volume.ambientOcclusion.menu')));
    await e.click(find.byKey(const Key('volume.ambientOcclusion.remove')));
    expect(saved(e).containsKey('ambientOcclusion'), isFalse);
    expect(find.byKey(const Key('volume.ambientOcclusion')), findsNothing);
    await reveal(tester, find.byKey(const Key('volume.addOverride')));
    await e.click(find.byKey(const Key('volume.addOverride')));
    expect(find.byKey(const Key('volume.add.bloom')), findsNothing, reason: 'only what the profile lacks');
    await e.click(find.byKey(const Key('volume.add.ambientOcclusion')));
    expect(saved(e)['ambientOcclusion'], <String, Object?>{});
    expect(ticked(tester, 'ambientOcclusion.radius'), isFalse, reason: 'every field inherited, as Unity adds one');

    // Tick Radius: the default it showed is written.
    await reveal(tester, tick('ambientOcclusion.radius'));
    await e.click(tick('ambientOcclusion.radius'));
    expect(saved(e)['ambientOcclusion'], {'radius': 0.6});

    // ⌘Z in the Inspector undoes the profile, step by step; the scene is untouched.
    expect(e.shell.registry.byId('document.undo')!.label, 'Undo Volume Profile Change');
    for (final undone in [
      () => saved(e)['ambientOcclusion'] is Map && (saved(e)['ambientOcclusion'] as Map).isEmpty,
      () => !saved(e).containsKey('ambientOcclusion'),
      () => (saved(e)['ambientOcclusion'] as Map)['radius'] == 0.6,
      () => (saved(e)['bloom'] as Map)['intensity'] == 0,
      () => (saved(e)['bloom'] as Map)['threshold'] == 0.9,
    ]) {
      await e.press(LogicalKeyboardKey.keyZ, command: true);
      expect(undone(), isTrue);
    }
    expect(ticked(tester, 'bloom.threshold'), isTrue);
    expect(scene.document.history, 0, reason: 'the scene had nothing to undo');
    await e.press(LogicalKeyboardKey.keyZ, command: true, shift: true);
    expect((saved(e)['bloom'] as Map).containsKey('threshold'), isFalse, reason: '⇧⌘Z redoes');
  });

  testWidgets('Color Grading: added as an override, a tone mapper picked, numbers and a channel typed', (tester) async {
    final (e, look) = await onLook(tester);
    final drawn = e.shell.scene!.current!.preview!;
    expect(find.byKey(const Key('volume.colorGrading')), findsNothing, reason: 'a new profile has no grade');

    await reveal(tester, find.byKey(const Key('volume.addOverride')));
    await e.click(find.byKey(const Key('volume.addOverride')));
    await e.click(find.byKey(const Key('volume.add.colorGrading')));
    expect(saved(e)['colorGrading'], <String, Object?>{});
    expect(ticked(tester, 'colorGrading.tonemapper'), isFalse);

    // The tone mapper: ticked (ACES, the default, written), then Neutral.
    await reveal(tester, tick('colorGrading.tonemapper'));
    await e.click(tick('colorGrading.tonemapper'));
    expect(saved(e)['colorGrading'], {'tonemapper': 'aces'});
    await e.click(inRow('colorGrading.tonemapper', find.byType(EnumField)));
    await e.click(find.text('Neutral').last);
    expect((saved(e)['colorGrading'] as Map)['tonemapper'], 'neutral');

    // Saturation, and Lift's blue channel alone.
    await reveal(tester, tick('colorGrading.saturation'));
    await e.click(tick('colorGrading.saturation'));
    await typeInto(e, 'colorGrading.saturation', '-40');
    await reveal(tester, tick('colorGrading.lift'));
    await e.click(tick('colorGrading.lift'));
    final blue = inRow(
      'colorGrading.lift',
      find.descendant(of: find.byKey(const ValueKey('vec.2')), matching: find.byKey(const ValueKey('number.text'))),
    );
    await reveal(tester, blue);
    await e.click(blue);
    await e.type('0.2');
    await e.submit();
    expect(saved(e)['colorGrading'], {
      'tonemapper': 'neutral',
      'saturation': -40,
      'lift': [0, 0, 0.2],
    });
    final grade = drawn.volume(look)['colorGrading'] as Map;
    expect(grade['tonemapper'], 'neutral', reason: 'the Scene view draws the grade');
    expect(grade['saturation'], -40.0);
    expect((grade['lift'] as List).map((v) => (v as num).toDouble()), [0.0, 0.0, closeTo(0.2, 1e-6)]);
  });

  testWidgets('a global volume with the profile: the bloom set in the Inspector is in the rendered picture', (
    tester,
  ) async {
    final (e, look) = await onLook(tester);
    final tab = e.shell.tabs!.focused!;
    tab.document.apply([
      Command.addComponent(sun, {
        'type': 'fynkel.Volume',
        'version': 1,
        'data': {
          'profile': {'asset': look},
          'shape': 'global',
          'size': [1, 1, 1],
          'radius': 1,
          'blend_distance': 0,
          'weight': 1,
          'priority': 0,
          'enabled': true,
        },
      }),
    ]);
    await e.press(LogicalKeyboardKey.keyS, command: true);

    Future<Uint8List> render(String name) async {
      final cli = Platform.environment['FYNKEL_CLI'] ?? 'fynkel';
      final out = '${e.dir.path}/$name.png';
      final r = (await tester.runAsync(
        () => Process.run(cli, [
          'render',
          '--project',
          e.dir.path,
          '--scene',
          mainScene,
          '--out',
          out,
          '--size',
          '320x180',
        ]),
      ))!;
      expect(r.exitCode, 0, reason: '${r.stdout}\n${r.stderr}');
      return decodePng(File(out).readAsBytesSync()).$3;
    }

    final before = await render('before');
    await typeInto(e, 'bloom.threshold', '0');
    await typeInto(e, 'bloom.intensity', '6');
    final after = await render('after');
    var brighter = 0;
    for (var i = 0; i < after.length; i += 4) {
      final a = after[i] + after[i + 1] + after[i + 2];
      final b = before[i] + before[i + 1] + before[i + 2];
      if (a > b + 30) brighter++;
    }
    expect(brighter, greaterThan(after.length ~/ 4 ~/ 10), reason: 'a tenth of the picture glows');
  });
}
