// Unity parity: a model's import settings — Scale Factor, Normals, Import
// Animation — and the "Unapplied import settings" question when the
// Inspector leaves a model whose settings changed, driven through the
// whole shell over a real project; the scaled model is rendered by the
// real `fynkel render` (docs/unity-parity.md § Tier 2).
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/scene_view/png.dart';

import 'harness.dart';

const robot = 'c0000000-0000-4000-8000-000000000001';
const rig = 'c0000000-0000-4000-8000-000000000005';

void main() {
  Future<Editor> inModels(WidgetTester tester) async {
    final e = await Editor.open(tester);
    e.shell.browser!.setFolder('models');
    await settleCooks(tester);
    return e;
  }

  Map<String, Object?> sidecar(Editor e, String file) =>
      (jsonDecode(File('${e.dir.path}/assets/models/$file.meta.json').readAsStringSync()) as Map)['settings']
          as Map<String, Object?>;

  Future<void> reveal(WidgetTester tester, Finder f) async {
    await Scrollable.ensureVisible(tester.element(f), alignment: 0.5);
    await settleCooks(tester);
  }

  Future<Uint8List> render(Editor e, String name) async {
    final cli = Platform.environment['FYNKEL_CLI'] ?? 'fynkel';
    final out = '${e.dir.path}/$name.png';
    final r = (await e.tester.runAsync(
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

  testWidgets('Scale Factor typed and applied: the sidecar holds it and the robot is drawn bigger', (tester) async {
    final e = await inModels(tester);
    final before = await render(e, 'before');
    await e.click(find.byKey(const Key('project.asset.$robot')));

    final scale = find.descendant(
      of: find.byKey(const Key('asset.settings.scaleFactor')),
      matching: find.byKey(const ValueKey('number.text')),
    );
    await reveal(tester, scale);
    await e.click(scale);
    await e.type('2.5');
    await e.submit();
    expect(sidecar(e, 'robot.glb'), isEmpty, reason: 'nothing written before Apply');

    // Normals → Calculate shows the Smoothing Angle.
    expect(find.byKey(const Key('asset.settings.smoothingAngle')), findsNothing);
    await reveal(tester, find.byKey(const Key('asset.settings.normals')));
    await e.click(find.byKey(const Key('asset.settings.normals')));
    await e.click(find.text('Calculate').last);
    expect(find.byKey(const Key('asset.settings.smoothingAngle')), findsOneWidget);

    await e.click(find.byKey(const Key('asset.applySettings')));
    await settleCooks(tester);
    expect(sidecar(e, 'robot.glb'), {'scaleFactor': 2.5, 'normals': 'calculate'});

    final after = await render(e, 'after');
    var changed = 0;
    for (var i = 0; i < after.length; i += 4) {
      if ((after[i] - before[i]).abs() + (after[i + 1] - before[i + 1]).abs() + (after[i + 2] - before[i + 2]).abs() >
          30) {
        changed++;
      }
    }
    expect(changed, greaterThan(after.length ~/ 4 ~/ 50), reason: 'the robot is drawn two and a half times its size');
  });

  testWidgets('leaving a model with unapplied settings asks: Revert keeps the file, Apply writes it', (tester) async {
    final e = await inModels(tester);
    Future<void> untickAnimation() async {
      await e.click(find.byKey(const Key('project.asset.$rig')));
      final box = find.descendant(
        of: find.byKey(const Key('asset.settings.importAnimation')),
        matching: find.byType(Checkbox),
      );
      await reveal(tester, box);
      await e.click(box);
      await e.click(find.byKey(const Key('project.asset.$robot')));
    }

    await untickAnimation();
    expect(find.byKey(const Key('asset.unapplied')), findsOneWidget);
    expect(find.textContaining("Unapplied import settings for 'assets/models/rig.glb'"), findsOneWidget);
    await e.click(find.byKey(const Key('asset.unapplied.revert')));
    expect(find.byKey(const Key('asset.unapplied')), findsNothing);
    expect(sidecar(e, 'rig.glb')['importAnimation'], isNull);

    await untickAnimation();
    await e.click(find.byKey(const Key('asset.unapplied.apply')));
    await settleCooks(tester);
    expect(sidecar(e, 'rig.glb')['importAnimation'], false);

    // Nothing changed: no question.
    await e.click(find.byKey(const Key('project.asset.$rig')));
    await e.click(find.byKey(const Key('project.asset.$robot')));
    expect(find.byKey(const Key('asset.unapplied')), findsNothing);
  });
}
