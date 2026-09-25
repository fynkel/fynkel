// Project › Modules… (ADR-0029 §3): the dialog over the fakes sends only
// what changed, warns that a module turned off keeps its components, and
// the project reads the block back; the real session writes the same.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/project/modules_dialog.dart';

import 'inspector_test.dart' show rig;

void main() {
  test('a project without the block runs every module', () {
    expect(const ProjectModules().has('physics'), isTrue);
    expect(ProjectModules.fromJson({'audio': false}), const ProjectModules(audio: false));
    expect(ProjectModules.fromJson({'strip': true}).strip, isTrue);
  });

  testWidgets('turning animation off warns, applies one step and undoes', (tester) async {
    final r = rig();
    final file = File('${r.project.dir}/fynkel.project.json')..writeAsStringSync(jsonEncode({'name': 'x'}));
    expect(r.project.modules(), const ProjectModules());
    tester.view.physicalSize = const Size(1200, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Builder(
            builder: (context) =>
                TextButton(onPressed: () => showModulesDialog(context, r.project), child: const Text('open')),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    FilledButton apply() => tester.widget<FilledButton>(find.byKey(const Key('modules.apply')));
    expect(apply().onPressed, isNull, reason: 'nothing changed');
    expect(find.byKey(const Key('modules.warning')), findsNothing);

    await tester.tap(find.byKey(const Key('modules.animation')));
    await tester.pump();
    expect(find.byKey(const Key('modules.warning')), findsOneWidget);
    expect(find.textContaining('animation components stay'), findsOneWidget);
    expect(apply().onPressed, isNotNull);

    await tester.tap(find.byKey(const Key('modules.apply')));
    await tester.pumpAndSettle();
    expect(find.byType(ModulesDialog), findsNothing);
    expect(jsonDecode(file.readAsStringSync())['modules'], {'animation': false});
    expect(r.project.modules(), const ProjectModules(animation: false));

    r.project.undoAsset();
    expect(r.project.modules(), const ProjectModules());
  });
}
