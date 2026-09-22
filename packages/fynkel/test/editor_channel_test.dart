// The editor channel (ADR-0016, blueprint §19.2e item 8): with
// `FYNKEL_EDITOR_CHANNEL=stdio` the SDK writes one JSON line per log,
// diagnostic, stats window and state change; with it unset it writes
// nothing at all — the game the editor never launched behaves as before.

import 'dart:convert';
import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

const playground = 'c0000000-0000-4000-8000-000000000031';

List<Map<String, Object?>> records(List<String> lines) =>
    [for (final l in lines) jsonDecode(l) as Map<String, Object?>];

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;
  final lines = <String>[];

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-channel-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  setUp(() {
    lines.clear();
    EditorChannel.debugUse(enabled: true, sink: lines.add);
  });
  tearDown(() => EditorChannel.debugUse());

  test('a world announces itself, its load and its diagnostics as JSON lines', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    final started = records(lines).single;
    expect(started['fynkel'], 'state');
    expect(started['state'], 'started');
    expect(started['seq'], 0);
    expect(started['pid'], pid);

    lines.clear();
    final report = w.loadScene(playground, cacheDir: cache);
    expect(report.ok, isTrue);
    final loaded = records(lines).lastWhere((r) => r['state'] == 'loaded');
    expect(loaded['fynkel'], 'state');
    expect(loaded['scene'], 'Playground');
    expect(loaded['document'], endsWith('playground.scene.json'));
    expect(loaded['entities'], report.entityCount);
    // Every record is a complete line of JSON and the sequence never repeats.
    final seqs = [for (final r in records(lines)) r['seq'] as int];
    expect(seqs, equals([...seqs]..sort()));
    expect(seqs.toSet().length, seqs.length);
  });

  test('a load failure and a missed deadline are diagnostic records', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 2, maxFrameDt: 0.25);
    addTearDown(w.dispose);
    lines.clear();
    // A scene that is not in the cache: the load throws, and every §14.2
    // record the loader produced is on the channel with the file it names.
    expect(
      () => w.loadScene('c0000000-0000-4000-8000-0000000000ff', cacheDir: cache),
      throwsA(isA<FynkelLoadError>()),
    );
    final failed = records(lines);
    final error = failed.firstWhere((r) => r['fynkel'] == 'diagnostic');
    expect(error['severity'], 'error');
    expect('${error['code']}', startsWith('FYN-'));
    expect(error['message'], isA<String>());
    expect(failed.last['state'], 'loadFailed');

    // A stall drops time: `missed_deadline` is informational, never an error.
    lines.clear();
    w.frame(1 / 60);
    w.frame(2);
    final missed = records(lines).where((r) => r['fynkel'] == 'diagnostic').toList();
    expect(missed, isNotEmpty, reason: '$lines');
    expect(missed.first['code'], 'missedDeadline');
    expect(missed.first['severity'], 'info');
    expect(missed.first['source'], 'begin_frame');
    expect(missed.first['tick'], isA<int>());
  });

  test('Diagnostics.log is a log record and PrintDiagnostics stays quiet', () {
    final context = GameContext(time: TestTime(tick: 7));
    context.run(() => Diagnostics.log('coin collected', source: 'player'));
    final log = records(lines).single;
    expect(log['fynkel'], 'log');
    expect(log['message'], 'coin collected');
    expect(log['source'], 'player');
    expect(log['severity'], 'info');
    expect(log['tick'], 7);
  });

  test('with the channel off nothing is written and the sequence stands still', () {
    EditorChannel.debugUse(enabled: false, sink: lines.add);
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    w.loadScene(playground, cacheDir: cache);
    GameContext(time: TestTime()).run(() => Diagnostics.log('quiet'));
    EditorChannel.log('quiet too');
    expect(lines, isEmpty);
    expect(EditorChannel.sequence, 0);
  });

  test('the environment decides: the variable must say stdio', () {
    EditorChannel.debugUse();
    expect(EditorChannel.enabled, (Platform.environment[EditorChannel.variable] ?? '') == 'stdio');
  });
}
