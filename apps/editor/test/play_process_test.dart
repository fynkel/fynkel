// Play against a real process (blueprint §19.2e item 8): a shell script
// stands in for `fynkel run` — it announces itself on the editor channel,
// starts a grandchild the way `flutter run` starts the game, and waits.
//
// Stop must take the whole tree down, and a `kill -9` from outside must
// end in an exit code on the toolbar with every document in the project
// byte-identical: the editor never writes a document for the game.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/play/game_record.dart';
import 'package:fynkel_editor/src/play/play_session.dart';

const _script = r'''#!/bin/sh
# Stands in for `fynkel run --project <dir>`: the channel's records on
# stdout, a grandchild (what `flutter run` would be), then a wait.
dir="$3"
echo "{\"fynkel\":\"state\",\"seq\":1,\"state\":\"started\",\"pid\":$$}"
echo "{\"fynkel\":\"log\",\"seq\":2,\"message\":\"channel=$FYNKEL_EDITOR_CHANNEL\",\"source\":\"game\"}"
echo "{\"fynkel\":\"diagnostic\",\"seq\":3,\"code\":\"staleEntity\",\"severity\":\"error\",\"message\":\"the entity is gone\"}"
sh -c 'while :; do sleep 1; done' &
echo $! > "$dir/child.pid"
while :; do sleep 1; done
''';

/// A project directory with two documents and the fake CLI.
({Directory dir, String cli, Map<String, String> documents}) project(String name) {
  final dir = Directory.systemTemp.createTempSync('fynkel_editor_$name');
  Directory('${dir.path}/assets/scenes').createSync(recursive: true);
  File('${dir.path}/fynkel.project.json').writeAsStringSync('{"name":"$name"}\n');
  File('${dir.path}/assets/scenes/main.scene.json').writeAsStringSync('{"id":"s1","entities":{}}\n');
  final cli = '${dir.path}/fynkel.sh';
  File(cli).writeAsStringSync(_script);
  Process.runSync('chmod', ['+x', cli]);
  return (dir: dir, cli: cli, documents: documentsOf(dir));
}

/// Every document of the project, by relative path, byte for byte.
Map<String, String> documentsOf(Directory dir) => {
  for (final f in dir.listSync(recursive: true).whereType<File>())
    if (f.path.endsWith('.json'))
      f.path.substring(dir.path.length + 1): base64Encode(f.readAsBytesSync()),
};

bool alive(int pid) => Process.runSync('ps', ['-p', '$pid']).exitCode == 0;

/// What the session knows, for a failure's reason.
String report(PlaySession s) =>
    '${s.state} exit=${s.exitCode} failure=${s.failure}\n${s.records.map((r) => r.text).join('\n')}';

/// Waits for [ready], up to [within]; false when it never came true. The
/// budget is generous: the whole suite runs its files at once, and a
/// machine with every core busy takes its time to schedule a shell.
Future<bool> until(bool Function() ready, {Duration within = const Duration(seconds: 60)}) async {
  final deadline = DateTime.now().add(within);
  while (DateTime.now().isBefore(deadline)) {
    if (ready()) return true;
    await Future<void>.delayed(const Duration(milliseconds: 20));
  }
  return ready();
}

void main() {
  test('Stop takes the grandchild with it and leaves every document byte-identical', () async {
    final p = project('stop');
    addTearDown(() => p.dir.deleteSync(recursive: true));
    final session = PlaySession(
      projectDir: p.dir.path,
      cli: () => p.cli,
      stopGrace: const Duration(milliseconds: 500),
    );
    addTearDown(session.dispose);

    await session.play();
    expect(await until(() => session.state == PlayState.running), isTrue, reason: report(session));
    expect(session.gamePid, session.pid); // the script is the process the editor started
    // The environment reached the game: `package:fynkel` only writes when it did.
    expect(await until(() => session.records.any((r) => r.message == 'channel=stdio')), isTrue);
    final diagnostic = await until(() => session.records.any((r) => r.code == 'staleEntity'));
    expect(diagnostic, isTrue);
    expect(session.records.firstWhere((r) => r.code == 'staleEntity').severity, Severity.error);

    final childFile = File('${p.dir.path}/child.pid');
    expect(await until(childFile.existsSync), isTrue);
    final child = int.parse(childFile.readAsStringSync().trim());
    expect(alive(child), isTrue);

    await session.stop();
    expect(session.state, PlayState.exited);
    expect(session.exitCode, isNotNull);
    expect(session.crashed, isFalse); // it was asked to stop
    expect(await until(() => !alive(child)), isTrue, reason: 'the grandchild survived Stop');
    expect(documentsOf(p.dir), p.documents);
  });

  test('a `kill -9` from outside shows the exit code, keeps the records and changes no document', () async {
    final p = project('kill');
    addTearDown(() => p.dir.deleteSync(recursive: true));
    final session = PlaySession(projectDir: p.dir.path, cli: () => p.cli);
    addTearDown(session.dispose);

    await session.play();
    expect(await until(() => session.state == PlayState.running), isTrue, reason: report(session));
    expect(await until(() => session.records.any((r) => r.code == 'staleEntity')), isTrue, reason: report(session));
    final childFile = File('${p.dir.path}/child.pid');
    expect(await until(childFile.existsSync), isTrue);
    final child = int.parse(childFile.readAsStringSync().trim());

    // Somebody else kills the run — Activity Monitor, a `kill -9`, a crash.
    expect(Process.killPid(session.pid!, ProcessSignal.sigkill), isTrue);
    expect(await until(() => session.state == PlayState.exited), isTrue);
    expect(session.exitCode, isNot(0));
    expect(session.crashed, isTrue);
    expect(session.statusText, 'Exited with code ${session.exitCode}');
    expect(session.records.last.kind, GameRecordKind.state);
    expect(session.records.last.message, contains('nothing in the project was changed'));
    // What the game said before it died is still in the Console.
    expect(session.records.any((r) => r.code == 'staleEntity'), isTrue);
    expect(documentsOf(p.dir), p.documents);

    // The orphaned grandchild is nobody's now; the test does not leave it.
    Process.killPid(child, ProcessSignal.sigkill);
  });
}
