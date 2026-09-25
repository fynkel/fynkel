// Item 9b's title bar: the project menu (with the recent projects), the
// document tabs, Play in the middle, the codegen state, the command search, and — with the macOS half answering on
// `fynkel.editor/window` — room for the traffic lights and a bar whose empty
// space moves and zooms the window while its controls stay controls.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/shell/window_chrome.dart';

import 'support/fakes.dart';

const _channel = MethodChannel('fynkel.editor/window');

void main() {
  late Preferences prefs;
  setUp(() => prefs = Preferences.inMemory());

  Future<void> pumpShell(WidgetTester tester) async {
    final project = FakeProject(dir: '/fake/game');
    await pumpLarge(tester, shellApp(prefs: prefs, projects: {'/fake/game': project}, initialProject: '/fake/game'));
  }

  testWidgets('one bar: the project menu, the tabs, Play, and the search that opens the palette', (tester) async {
    await pumpShell(tester);
    final bar = find.byKey(const Key('window.titleBar'));
    expect(tester.getSize(bar).height, 38);
    for (final key in ['window.projectName', 'tabs.bar', 'tabs.open', 'window.play', 'window.commands']) {
      expect(
        find.descendant(of: bar, matching: find.byKey(Key(key))),
        findsOneWidget,
        reason: key,
      );
    }
    // The path moved into the project menu, and the status bar lost its hint.
    expect(find.text('/fake/game'), findsNothing);
    expect(find.text('⇧⌘P commands'), findsNothing);

    // Play sits in the middle of the window, whatever is on either side.
    final play = tester.getCenter(find.byKey(const Key('window.play')));
    expect((play.dx - 800).abs(), lessThan(40));

    await tester.tap(find.byKey(const Key('window.project')));
    await tester.pumpAndSettle();
    expect(find.text('/fake/game'), findsOneWidget);
    expect(find.byKey(const Key('window.revalidate')), findsOneWidget);
    expect(find.byKey(const Key('window.reveal')), findsOneWidget);
    expect(find.byKey(const Key('window.close')), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('window.commands')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('palette.search')), findsOneWidget);
  });

  testWidgets('the project menu switches to a recent project', (tester) async {
    final other = fakeProjectDir('title_other');
    addTearDown(() => other.deleteSync(recursive: true));
    prefs.addRecent(other.path, 'Other Game');
    prefs.addRecent('/fake/gone', 'Gone Game'); // no fynkel.project.json there
    final otherProject = FakeProject(dir: other.path);
    await pumpLarge(
      tester,
      shellApp(
        prefs: prefs,
        projects: {
          '/fake/game': FakeProject(dir: '/fake/game'),
          other.path: otherProject,
        },
        initialProject: '/fake/game',
      ),
    );
    await tester.tap(find.byKey(const Key('window.project')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('window.openProject')), findsOneWidget);
    // The current project is not offered; a missing one is, disabled.
    expect(find.text('Other Game'), findsOneWidget);
    expect(find.text('Gone Game'), findsOneWidget);
    final gone = tester.widget<PopupMenuItem<String>>(
      find.ancestor(of: find.text('Gone Game'), matching: find.byType(PopupMenuItem<String>)),
    );
    expect(gone.enabled, isFalse);
    await tester.tap(find.text('Other Game'));
    await tester.pumpAndSettle();
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    expect(shell.project, same(otherProject));
  });

  testWidgets('the codegen state: quiet when current, a word when stale or failed, a click checks again', (
    tester,
  ) async {
    final dir = fakeProjectDir('title_codegen');
    addTearDown(() => dir.deleteSync(recursive: true));
    File('${dir.path}/pubspec.yaml').writeAsStringSync('name: game\n');
    String report(String status) => jsonEncode({
      'project': {
        'status': status,
        if (status == 'stale') 'stale': ['lib/generated/behaviours.g.json'],
      },
    });
    final runner = RecordingRunner(stdout: report('stale'), exitCode: 2);
    await pumpLarge(
      tester,
      shellApp(
        prefs: prefs,
        runner: runner,
        projects: {dir.path: FakeProject(dir: dir.path)},
        initialProject: dir.path,
      ),
    );
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    // Nothing checked yet: nothing to say.
    expect(find.byKey(const Key('window.codegen')), findsNothing);

    shell.registry.execute('inspector.checkCodegen');
    await tester.pumpAndSettle();
    final chip = find.byKey(const Key('window.codegen'));
    expect(find.descendant(of: chip, matching: find.text('Run codegen')), findsOneWidget);

    runner.stdout = report('upToDate');
    runner.exitCode = 0;
    await tester.tap(chip);
    await tester.pumpAndSettle();
    expect(runner.calls.where((c) => c.$2.contains('--check')).length, 2);
    expect(chip, findsOneWidget);
    expect(find.descendant(of: chip, matching: find.byType(Text)), findsNothing); // an icon, no word

    runner.stdout = '';
    runner.stderr = 'build_runner failed';
    runner.exitCode = 1;
    await tester.tap(chip);
    await tester.pumpAndSettle();
    expect(find.descendant(of: chip, matching: find.text('Codegen check failed')), findsOneWidget);
  });

  testWidgets('with the macOS half: room for the traffic lights, and only empty space drags or zooms', (tester) async {
    final calls = <String>[];
    final messenger = tester.binding.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(_channel, (call) async {
      calls.add(call.method);
      return call.method == 'isFullScreen' ? false : null;
    });
    addTearDown(() {
      messenger.setMockMethodCallHandler(_channel, null);
      WindowChrome.instance.reset();
    });
    await WindowChrome.instance.refresh();
    await pumpShell(tester);

    final chrome = WindowChrome.instance;
    expect(chrome.ownsTitleBar.value, isTrue);
    expect(chrome.leadingInset, 76);
    final bar = tester.getRect(find.byKey(const Key('window.titleBar')));
    expect(tester.getTopLeft(find.byKey(const Key('window.project'))).dx, greaterThanOrEqualTo(bar.left + 76));
    // The controls sit on the bar's middle, level with the traffic lights.
    for (final key in ['window.project', 'window.play']) {
      final at = find.byKey(Key(key));
      if (at.evaluate().isEmpty) continue;
      expect(tester.getCenter(at).dy, moreOrLessEquals(bar.center.dy, epsilon: 1), reason: key);
    }

    // Empty space between the tabs and Play.
    final empty = Offset(bar.left + 560, bar.center.dy);
    calls.clear();
    await tester.tapAt(empty);
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tapAt(empty);
    await tester.pumpAndSettle();
    expect(calls, ['titleBarDoubleClick']);

    calls.clear();
    await tester.dragFrom(empty, const Offset(80, 40));
    await tester.pumpAndSettle();
    expect(calls, ['startDrag']);

    // A control is a control: a drag or a double-click on it is not the window's.
    calls.clear();
    await tester.dragFrom(tester.getCenter(find.byKey(const Key('tabs.open'))), const Offset(80, 40));
    await tester.pumpAndSettle();
    expect(calls, isEmpty);

    // Full screen: the traffic lights are gone, and so is their room.
    await messenger.handlePlatformMessage(
      _channel.name,
      const StandardMethodCodec().encodeMethodCall(const MethodCall('fullScreen', true)),
      (_) {},
    );
    await tester.pump();
    expect(chrome.leadingInset, 0);
    expect(tester.getTopLeft(find.byKey(const Key('window.project'))).dx, lessThan(bar.left + 20));
  });
}
