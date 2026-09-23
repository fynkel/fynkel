import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/env/tools.dart';
import 'package:fynkel_editor/src/launcher/launcher.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'support/fakes.dart';

void main() {
  late Preferences prefs;
  late Directory dir;

  setUp(() {
    prefs = Preferences.inMemory();
    dir = fakeProjectDir('launch');
  });
  tearDown(() => dir.deleteSync(recursive: true));

  testWidgets('the launcher shows checks, recents and opens a chosen project', (tester) async {
    final project = FakeProject(dir: dir.path, name: 'Launched');
    prefs.addRecent('/gone/project', 'Gone');
    await pumpLarge(
      tester,
      shellApp(
        prefs: prefs,
        projects: {dir.path: project},
        chosenDirectory: dir.path,
        toolchain: fakeToolchain(flutter: null),
      ),
    );
    expect(find.text('Fynkel Editor'), findsOneWidget);
    expect(find.byKey(const Key('launcher.check.Fynkel CLI')), findsOneWidget);
    expect(find.byKey(const Key('launcher.check.Flutter')), findsOneWidget);
    expect(find.byKey(const Key('launcher.check.Authoring ABI')), findsOneWidget);
    expect(find.textContaining('missing'), findsOneWidget); // the gone recent
    final gone = tester.widget<ListTile>(find.byKey(const Key('launcher.recent./gone/project')));
    expect(gone.enabled, isFalse);

    await tester.tap(find.byKey(const Key('launcher.recent.remove./gone/project')));
    await tester.pumpAndSettle();
    expect(prefs.recents, isEmpty);

    await tester.tap(find.byKey(const Key('launcher.open')));
    await tester.pumpAndSettle();
    expect(find.text('Launched'), findsOneWidget);
    expect(find.byKey(const Key('window.projectName')), findsOneWidget);
    expect(prefs.recents.single.dir, dir.path);

    // Close Project lives in the title bar's project menu.
    await tester.tap(find.byKey(const Key('window.project')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('window.close')));
    await tester.pumpAndSettle();
    expect(project.closed, isTrue);
    expect(find.byKey(const Key('launcher.open')), findsOneWidget);
    expect(find.byKey(Key('launcher.recent.${dir.path}')), findsOneWidget);
  });

  testWidgets('the launcher fits a small window with long paths and failing checks', (tester) async {
    // The first live run overflowed: the button row in a narrow window, and the
    // check tiles whose path + fix wrap to more lines than a ListTile allows.
    // Any RenderFlex overflow is an exception, which fails the test.
    final long = '/Users/someone/Documents/Projects/Games/${'deep/' * 12}my_game';
    prefs.addRecent(long, 'A game with a rather long title that keeps going');
    tester.view.physicalSize = const Size(800, 500);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(shellApp(prefs: prefs, toolchain: fakeToolchain(cli: null, root: null, flutter: null)));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('launcher.new')), findsOneWidget);
    expect(find.byKey(const Key('launcher.check.Fynkel CLI')), findsOneWidget);
    expect(find.textContaining('cargo build -p fynkel'), findsOneWidget); // the fix is shown in full
    expect(find.byKey(Key('launcher.recent.$long')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('a directory without fynkel.project.json is refused with a message', (tester) async {
    final other = Directory.systemTemp.createTempSync('not_a_project');
    addTearDown(() => other.deleteSync(recursive: true));
    await pumpLarge(tester, shellApp(prefs: prefs, chosenDirectory: other.path));
    await tester.tap(find.byKey(const Key('launcher.open')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('launcher.error')), findsOneWidget);
    expect(find.textContaining('not a Fynkel project'), findsOneWidget);
  });

  testWidgets('a project named at launch opens once the tools are found, so its codegen check has the CLI', (
    tester,
  ) async {
    // The 9a gate's finding: `Fynkel Editor <dir>` opened the project on the
    // first frame, the codegen check asked for the CLI before `flutter
    // --version` had come back, and the Inspector said none was configured.
    final detected = Completer<Toolchain>();
    final project = FakeProject(dir: dir.path);
    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      shellApp(
        prefs: prefs,
        projects: {dir.path: project},
        initialProject: dir.path,
        detectToolchain: (_) => detected.future,
      ),
    );
    // Frames go by while the tools are looked for (the launcher's spinner
    // never settles, so step them by hand).
    for (var i = 0; i < 5; i++) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    expect(shell.toolchain, isNull);
    expect(shell.project, isNull, reason: 'the project waits for the tools');
    detected.complete(fakeToolchain());
    await tester.pumpAndSettle();
    expect(shell.toolchain?.fynkelCli, isNotNull);
    expect(shell.project?.dir, dir.path);
  });

  testWidgets('a read-only project shows the banner with the lock reason', (tester) async {
    final project = FakeProject(dir: dir.path, readOnly: "the lock's formatVersion 2 is newer than the supported 1");
    await pumpLarge(tester, shellApp(prefs: prefs, projects: {dir.path: project}, initialProject: dir.path));
    expect(find.byKey(const Key('window.readOnly')), findsOneWidget);
    expect(find.byKey(const Key('window.readOnlyChip')), findsOneWidget);
    expect(find.textContaining("formatVersion 2 is newer"), findsOneWidget);
  });

  testWidgets('outdated documents show a banner whose preview lists steps, diff and the command', (tester) async {
    final project = FakeProject(dir: dir.path)
      ..migrations = MigrationPreview.fromJson({
        'outdated': 1,
        'steps': [
          {'id': 'scene/0-1/entities-keyed-by-id', 'kind': 'scene', 'from': 0, 'to': 1, 'description': 'keyed'},
        ],
        'documents': [
          {
            'path': '${dir.path}/assets/scenes/old.scene.json',
            'name': 'Old',
            'kind': 'scene',
            'from': 0,
            'to': 2,
            'ok': true,
            'applied': [
              {'from': 0, 'to': 1, 'name': 'scene/0-1/entities-keyed-by-id', 'description': 'keyed'},
              {'from': 1, 'to': 2, 'name': 'scene/1-2/x', 'description': 'x'},
            ],
            'diff': '-  "formatVersion": 0,\n+  "formatVersion": 2,',
            'diagnostics': <Object?>[],
          },
        ],
      });
    await pumpLarge(tester, shellApp(prefs: prefs, projects: {dir.path: project}, initialProject: dir.path));
    expect(find.byKey(const Key('window.migrations')), findsOneWidget);
    expect(find.textContaining('1 document use an older format'), findsOneWidget);
    await tester.tap(find.text('Preview migration…'));
    await tester.pumpAndSettle();
    expect(find.byKey(Key('migrations.doc.${dir.path}/assets/scenes/old.scene.json')), findsOneWidget);
    expect(find.text('scene v0 → v2'), findsOneWidget);
    expect(find.textContaining('scene/0-1/entities-keyed-by-id'), findsOneWidget);
    expect(find.textContaining('"formatVersion": 2'), findsOneWidget);
    expect(find.text('fynkel migrate ${dir.path}'), findsOneWidget);
    // A lock from another build: the preview's command rewrites it too.
    project.diagnostics = [diag('FYN-P0011', message: 'the lock says 6, this build is 7')];
    await tester.pumpAndSettle();
    expect(find.text('fynkel migrate ${dir.path} && fynkel project lock --project ${dir.path}'), findsOneWidget);
    await tester.tap(find.text('Close'));
    await tester.pumpAndSettle();

    // The command opens the same dialog; a current project says so.
    project.migrations = MigrationPreview.fromJson({'outdated': 0, 'steps': <Object?>[], 'documents': <Object?>[]});
    project.notifyListeners();
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('window.migrations')), findsNothing);
    tester.state<EditorShellState>(find.byType(EditorShell)).registry.execute('project.migrations');
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('migrations.none')), findsOneWidget);
  });

  testWidgets('New Project runs `fynkel create` with --json and opens the result', (tester) async {
    final created = Directory('${dir.path}/my_game');
    final runner = RecordingRunner(stdout: '{"ok":true,"dir":"${created.path}","package":"my_game"}');
    final project = FakeProject(dir: created.path, name: 'My Game');
    await pumpLarge(
      tester,
      shellApp(prefs: prefs, runner: runner, projects: {created.path: project}, chosenDirectory: dir.path),
    );
    await tester.tap(find.byKey(const Key('launcher.new')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('new.title')), findsOneWidget);
    await tester.enterText(find.byKey(const Key('new.title')), 'My Game');
    await tester.pumpAndSettle();
    expect(tester.widget<TextField>(find.byKey(const Key('new.package'))).controller!.text, 'my_game');
    await tester.tap(find.byKey(const Key('new.choose')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('new.create')));
    await tester.pumpAndSettle();
    // The runner made no directory, so the shell opens the fake project by path.
    created.createSync();
    File('${created.path}/fynkel.project.json').writeAsStringSync('{}');
    await tester.pumpAndSettle();
    expect(runner.calls.single.$1, '/fake/fynkel');
    expect(runner.calls.single.$2, [
      'create',
      created.path,
      '--name',
      'my_game',
      '--title',
      'My Game',
      '--org',
      'com.example',
      '--root',
      '/fake/root',
      '--json',
    ]);
  });

  testWidgets('a failing `fynkel create` shows its diagnostics and stays open', (tester) async {
    final runner = RecordingRunner(
      exitCode: 1,
      stdout: '{"ok":false,"diagnostics":[{"code":"FYN-P0001","message":"the directory is not empty"}]}',
    );
    await pumpLarge(tester, shellApp(prefs: prefs, runner: runner, chosenDirectory: dir.path));
    await tester.tap(find.byKey(const Key('launcher.new')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('new.choose')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('new.create')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('new.error')), findsOneWidget);
    expect(find.textContaining('FYN-P0001: the directory is not empty'), findsOneWidget);
    expect(find.byKey(const Key('new.title')), findsOneWidget);
  });

  test('package names derive from titles and are validated', () {
    expect(NewProjectDialog.packageFromTitle('My Game'), 'my_game');
    expect(NewProjectDialog.packageFromTitle('  2D Racer! '), 'game_2d_racer');
    expect(NewProjectDialog.packageName.hasMatch('my_game'), isTrue);
    expect(NewProjectDialog.packageName.hasMatch('MyGame'), isFalse);
    expect(NewProjectDialog.orgName.hasMatch('dev.fynkel'), isTrue);
    expect(NewProjectDialog.orgName.hasMatch('fynkel'), isFalse);
    expect(NewProjectDialog.arguments(dir: '/d', package: 'g', title: 'G', org: 'a.b', root: '/r', flutter: false), [
      'create',
      '/d',
      '--name',
      'g',
      '--title',
      'G',
      '--org',
      'a.b',
      '--root',
      '/r',
      '--no-flutter',
      '--json',
    ]);
  });

  testWidgets('New Project without a CLI explains what is missing', (tester) async {
    await pumpLarge(tester, shellApp(prefs: prefs, toolchain: fakeToolchain(cli: null)));
    await tester.tap(find.byKey(const Key('launcher.new')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('launcher.error')), findsOneWidget);
    expect(find.byKey(const Key('new.title')), findsNothing);
  });

  testWidgets('preferences open from the launcher and change the theme', (tester) async {
    await pumpLarge(tester, shellApp(prefs: prefs));
    await tester.tap(find.byKey(const Key('launcher.preferences')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('prefs.theme')), findsOneWidget);
    await tester.tap(find.text('Dark'));
    await tester.pumpAndSettle();
    expect(prefs.theme, EditorTheme.dark);
    expect(Theme.of(tester.element(find.byKey(const Key('prefs.theme')))).brightness, Brightness.dark);
    expect(prefs.zoomToPointer, isTrue);
    await tester.tap(find.byKey(const Key('prefs.zoomToPointer')));
    await tester.pumpAndSettle();
    expect(prefs.zoomToPointer, isFalse);
    expect(prefs.toJson()['zoomToPointer'], isFalse);
    await tester.enterText(find.byKey(const Key('prefs.ide')), 'code -g {file}:{line}:{column}');
    await tester.tap(find.byKey(const Key('prefs.save')));
    await tester.pumpAndSettle();
    expect(prefs.ideCommand, 'code -g {file}:{line}:{column}');
    expect(find.byKey(const Key('prefs.theme')), findsNothing);
  });

  testWidgets('the shell layout store is cleared by Reset layout from preferences without a project', (tester) async {
    final store = MemoryLayoutStore()..write(const LayoutSnapshot(layout: 'V1:1:1(I;5:Scene;;F)', selected: []));
    await pumpLarge(tester, shellApp(prefs: prefs, store: store));
    tester.state<EditorShellState>(find.byType(EditorShell)).showPreferences();
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('prefs.resetLayout')));
    await tester.pumpAndSettle();
    expect(store.read(), isNull);
  });
}
