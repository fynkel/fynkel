// IDE navigation from the Console (roadmap Phase 2): a game's uncaught
// error arrives with its Dart stack; the frames in the game's own code are
// its location, ↩ opens the IDE there, and the details list every frame.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/play/dart_stack.dart';
import 'package:fynkel_editor/src/play/game_console.dart';
import 'package:fynkel_editor/src/play/game_console_panel.dart';
import 'package:fynkel_editor/src/play/game_record.dart';
import 'package:fynkel_editor/src/play/play_session.dart';

const stack = '''
#0      Player.update (package:my_game/game/player.dart:42:7)
#1      BehaviourRunner.tick (package:fynkel/src/behaviours.dart:310:12)
<asynchronous suspension>
#2      _rootRun (dart:async/zone.dart:1399:13)
#3      main (file:///g/lib/main.dart:9)
''';

String errorLine() => jsonEncode({
  'fynkel': 'log',
  'seq': 5,
  'tick': 88,
  'source': 'exception',
  'severity': 'error',
  'message': 'Bad state: no ground',
  'stack': stack,
});

void main() {
  test('both stack forms parse; only the game\'s own frames have a file', () {
    final frames = parseDartStack(stack);
    expect(frames.map((f) => f.member), ['Player.update', 'BehaviourRunner.tick', '_rootRun', 'main']);
    expect((frames[0].line, frames[0].column), (42, 7));
    expect((frames[3].line, frames[3].column), (9, null));
    expect(frames[0].location, 'package:my_game/game/player.dart:42:7');
    expect(frames[0].sourceIn('/g', ownPackage: 'my_game'), '/g/lib/game/player.dart');
    expect(frames[1].sourceIn('/g', ownPackage: 'my_game'), isNull);
    expect(frames[2].sourceIn('/g', ownPackage: 'my_game'), isNull);
    expect(frames[3].sourceIn('/g'), '/g/lib/main.dart');
    expect(frames[3].sourceIn('/other'), isNull);

    final terse = parseDartStack('package:my_game/enemy.dart 12:3  Enemy.onHit\ndart:core  print');
    expect(terse.single.member, 'Enemy.onHit');
    expect((terse.single.line, terse.single.column), (12, 3));
    // `flutter run`'s prefix on a printed frame (Build and Run's output).
    final printed = GameRecord.parse('flutter: #0      Enemy.onHit (package:my_game/enemy.dart:12:3)');
    expect(printed.frames.single.uri.toString(), 'package:my_game/enemy.dart');
    expect(parseDartStack('just a message'), isEmpty);

    final dir = Directory.systemTemp.createTempSync('fynkel-pubspec');
    addTearDown(() => dir.deleteSync(recursive: true));
    expect(packageNameOf(dir.path), isNull);
    File('${dir.path}/pubspec.yaml').writeAsStringSync('# a game\nname: my_game\nversion: 1.0.0\n');
    expect(packageNameOf(dir.path), 'my_game');
  });

  group('the Game tab', () {
    late PlaySession session;
    late GameConsoleModel model;
    late List<String> opened;

    setUp(() {
      session = PlaySession(projectDir: '/g', cli: () => null);
      opened = [];
      model = GameConsoleModel(
        session: session,
        relative: (path) => path.startsWith('/g/') ? path.substring(3) : path,
        projectDir: '/g',
        ownPackage: () => 'my_game',
        openInIde: (file, {line, column}) async {
          opened.add('$file:$line:$column');
          return null;
        },
      );
      session.beginInEditor();
      session.receive(errorLine());
    });

    tearDown(() {
      model.dispose();
      session.dispose();
    });

    test('an error with a stack is located at, and opens, its first own frame', () async {
      final r = model.entries.singleWhere((e) => e.record.source == 'exception').record;
      expect(r.stack, stack);
      expect(r.frames, hasLength(4));
      expect(model.location(r), 'lib/game/player.dart:42');
      model.select(model.keyOf(model.entries.singleWhere((e) => e.record == r)));
      expect(model.canOpenSelected, isTrue);
      model.openSelected();
      await pumpEventQueue();
      expect(opened, ['/g/lib/game/player.dart:42:7']);

      // Another package's frames alone lead nowhere.
      session.receive(
        jsonEncode({
          'fynkel': 'log',
          'seq': 6,
          'source': 'exception',
          'severity': 'error',
          'message': 'engine',
          'stack': '#0      Engine.step (package:fynkel/src/engine.dart:1:1)',
        }),
      );
      final other = model.entries.singleWhere((e) => e.record.message == 'engine').record;
      expect(model.location(other), '');
      expect(model.canActivate(other), isFalse);
    });

    testWidgets('the details list the frames; the game\'s own are links to the IDE', (tester) async {
      tester.view.physicalSize = const Size(1400, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SizedBox(
              width: 1200,
              height: 600,
              child: GameConsoleView(model: model, onPlay: () {}, onStop: () {}),
            ),
          ),
        ),
      );
      final entry = model.entries.singleWhere((e) => e.record.source == 'exception');
      expect(find.text('lib/game/player.dart:42'), findsOneWidget);
      model.select(model.keyOf(entry));
      await tester.pump();
      expect(tester.widget<Text>(find.byKey(const Key('console.game.frame.1'))).data, contains('BehaviourRunner.tick'));
      await tester.tap(find.byKey(const Key('console.game.frame.1')));
      await tester.tap(find.byKey(const Key('console.game.frame.3')));
      await tester.pump();
      expect(opened, ['/g/lib/main.dart:9:null']);

      model.dispose();
      model = GameConsoleModel(
        session: session,
        projectDir: '/g',
        ownPackage: () => 'my_game',
        openInIde: (file, {line, column}) async => '`zed` was not found on PATH',
      );
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SizedBox(
              width: 1200,
              height: 600,
              child: GameConsoleView(model: model, onPlay: () {}, onStop: () {}),
            ),
          ),
        ),
      );
      model.activate(entry.record);
      await tester.pump();
      expect(tester.widget<Text>(find.byKey(const Key('console.game.status'))).data, contains('`zed` was not found'));
    });
  });
}
