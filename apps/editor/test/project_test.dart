// The real `SessionProject` over the native runtime (the M4-02 gate): the
// fixture project and the reference game open and validate, a broken copy
// lists FYN-D0206 with its exact line and column in the Console, a newer
// lock opens read-only with the reason, and the watcher revalidates.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/console/console_panel.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';

final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute;
final referenceGame = Directory('../../examples/reference_game').absolute;

Directory copyFixture(String tag) {
  final dir = Directory.systemTemp.createTempSync('fynkel_editor_$tag');
  void copy(Directory from, Directory to) {
    to.createSync(recursive: true);
    for (final e in from.listSync()) {
      final name = e.uri.pathSegments.where((s) => s.isNotEmpty).last;
      if (name.startsWith('.')) continue;
      if (e is Directory) {
        copy(e, Directory('${to.path}/$name'));
      } else if (e is File) {
        e.copySync('${to.path}/$name');
      }
    }
  }

  copy(fixture, dir);
  return Directory(dir.resolveSymbolicLinksSync());
}

void main() {
  test('the fixture project opens, is named, and validates clean', () async {
    final dir = copyFixture('clean');
    addTearDown(() => dir.deleteSync(recursive: true));
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);
    expect(project.name, 'Fixture Game');
    expect(project.isReadOnly, isFalse);
    expect(project.validationError, isNull);
    expect(project.errorCount, 0);
    // The fixture has no generated behaviour metadata; that is the one warning.
    expect(project.diagnostics.map((d) => d.code), ['FYN-T0006']);
    expect(project.documents, greaterThan(5));
    expect(project.validatedAt, isNotNull);
  });

  test('the reference game opens and validates clean', () async {
    final project = await SessionProject.open(referenceGame.path, watch: false);
    addTearDown(project.close);
    expect(project.name, 'Fynkel Reference Game');
    expect(project.diagnostics.where((d) => d.severity == Severity.error), isEmpty);
    expect(project.isReadOnly, isFalse);
  });

  testWidgets('a misspelt field is listed in the Console at scenes/main.scene.json:83:13', (tester) async {
    final dir = copyFixture('broken');
    addTearDown(() => dir.deleteSync(recursive: true));
    final scene = File('${dir.path}/assets/scenes/main.scene.json');
    scene.writeAsStringSync(scene.readAsStringSync().replaceFirst('"speed"', '"speeed"'));
    // The native session does real work: run it outside the test's fake clock.
    final project = (await tester.runAsync(() => SessionProject.open(dir.path, watch: false)))!;
    addTearDown(project.close);

    final d = project.diagnostics.singleWhere((d) => d.code == 'FYN-D0206');
    // M3-11 put an AudioSource on the Robot ahead of the Spinner: line 75 → 82;
    // ADR-0020 gave the Camera its culling mask: 83.
    expect(d.line, 83);
    expect(d.column, 13);
    expect(d.pointer, endsWith('/data/speeed'));
    expect(project.relative(d.document!), 'assets/scenes/main.scene.json');

    final opened = <(String, int?, int?)>[];
    final model = ConsoleModel(
      project: project,
      openInIde: (file, {line, column}) async {
        opened.add((file, line, column));
        return null;
      },
    );
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: ConsolePanel(model: model)),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('assets/scenes/main.scene.json:83:13'), findsOneWidget);
    expect(find.text('FYN-D0206'), findsOneWidget);
    expect(
      [
        for (final s in ['error', 'warning'])
          tester
              .widget<Text>(find.descendant(of: find.byKey(Key('console.filter.$s')), matching: find.byType(Text)).last)
              .data,
      ],
      ['1', '1'],
    );
    await tester.tap(find.text('FYN-D0206'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('console.more')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('console.open')));
    await tester.pumpAndSettle();
    expect(opened.single, (d.document, 83, 13));
  });

  test('a lock from a newer Fynkel opens read-only with the reason', () async {
    final dir = copyFixture('newer');
    addTearDown(() => dir.deleteSync(recursive: true));
    final lockFile = File('${dir.path}/fynkel.lock.json');
    final lock = jsonDecode(lockFile.readAsStringSync()) as Map<String, Object?>;
    (lock['runtime'] as Map<String, Object?>)['version'] = '9.0.0';
    lockFile.writeAsStringSync(jsonEncode(lock));
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);
    expect(project.isReadOnly, isTrue);
    expect(project.readOnly, 'the lock names runtime 9.0.0; this build is 0.0.1');
    // The lock check also reports the mismatch itself (FYN-P0011), so the Console explains both.
    expect(project.diagnostics.map((d) => d.code), contains('FYN-P0011'));
  });

  test('an outdated scene is previewed through the session and counted by the project', () async {
    final dir = copyFixture('migrate');
    addTearDown(() => dir.deleteSync(recursive: true));
    File('${fixture.parent.path}/migrate/v0_training.scene.json')
        .copySync('${dir.path}/assets/scenes/training.scene.json');
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);
    expect(project.outdatedDocuments, 1);
    final doc = project.migrations!.documents.single;
    expect(project.relative(doc.path), 'assets/scenes/training.scene.json');
    expect(doc.from, 0);
    expect(doc.to, 5); // scenes are at formatVersion 5 since ADR-0022
    expect(doc.applied.first.id, 'scene/0-1/entities-keyed-by-id');
    expect(doc.diff, contains('formatVersion'));
    expect(project.diagnostics.map((d) => d.code), contains('FYN-D0016'));
  });

  test('a directory that is not a project throws an AuthoringException', () async {
    final dir = Directory.systemTemp.createTempSync('fynkel_editor_empty');
    addTearDown(() => dir.deleteSync(recursive: true));
    expect(isFynkelProject(dir.path), isFalse);
    expect(isFynkelProject(fixture.path), isTrue);
    await expectLater(SessionProject.open(dir.path, watch: false), throwsA(isA<AuthoringException>()));
  });

  test('editing a scene on disk revalidates through the watcher', () async {
    final dir = copyFixture('watch');
    addTearDown(() => dir.deleteSync(recursive: true));
    final project = await SessionProject.open(dir.path);
    addTearDown(project.close);
    expect(project.errorCount, 0);
    final changed = Completer<void>();
    project.addListener(() {
      if (!project.validating && project.errorCount > 0 && !changed.isCompleted) changed.complete();
    });
    await Future<void>.delayed(const Duration(milliseconds: 300)); // the watcher's ready
    final scene = File('${dir.path}/assets/scenes/main.scene.json');
    scene.writeAsStringSync(scene.readAsStringSync().replaceFirst('"speed"', '"speeed"'));
    await changed.future.timeout(const Duration(seconds: 15));
    expect(project.diagnostics.map((d) => d.code), contains('FYN-D0206'));
  }, timeout: const Timeout(Duration(seconds: 40)));
}
