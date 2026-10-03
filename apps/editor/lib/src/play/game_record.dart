// One line of the game process, as the Console's Game tab shows it: either
// a record of the editor channel (`{"fynkel": "log"|"diagnostic"|"stats"|
// "state", …}` — package:fynkel writes them when the process was started
// with `FYNKEL_EDITOR_CHANNEL=stdio`, ADR-0016) or a plain line the process
// printed. `flutter run` wraps the app's stdout in its own prefixes, so a
// record is found anywhere in the line, not only at its start.
import 'dart:convert';

import 'package:fynkel/gameplay.dart' show EntityRef;
import 'package:fynkel_authoring/fynkel_authoring.dart';

import 'dart_stack.dart';

enum GameRecordKind {
  /// `Diagnostics.log` and the engine's own notes.
  log,

  /// A §14.2 diagnostic record: a document the Console can open.
  diagnostic,

  /// One reporting window of the game's loop.
  stats,

  /// `started`, `loaded`, `loadFailed` — and the editor's own notes about
  /// the process (its launch, its exit), which use this kind too.
  state,

  /// A line that is not a record: whatever the process printed.
  output,
}

/// One row of the Game tab.
final class GameRecord {
  GameRecord({
    required this.kind,
    required this.severity,
    required this.message,
    required this.at,
    this.seq,
    this.tick,
    this.code,
    this.source,
    this.document,
    this.pointer,
    this.id,
    this.line,
    this.column,
    this.suggestion,
    this.entity,
    this.state,
    this.stats,
    this.related = const [],
    this.raw,
    this.stack,
  });

  /// The editor's own row (the launch, the exit, a dropped-records note):
  /// a `state` record nothing in the game wrote.
  factory GameRecord.editor(String message, {Severity severity = Severity.info, DateTime? at, String? state}) =>
      GameRecord(
        kind: GameRecordKind.state,
        severity: severity,
        message: message,
        at: at ?? DateTime.now(),
        source: 'editor',
        state: state,
      );

  final GameRecordKind kind;
  final Severity severity;
  final String message;
  final DateTime at;

  /// The record's place in the process's stream (`seq`); null for output.
  final int? seq;
  final int? tick;
  final String? code;

  /// `game`, `step`, `loadProject`, `stdout`, `stderr`, `editor`.
  final String? source;

  /// The document the record is about, as the game named it (absolute, or
  /// relative to the project) — what the Console opens.
  final String? document;
  final String? pointer;

  /// The authoring id of the entity or asset the record is about.
  final String? id;
  final int? line;
  final int? column;
  final String? suggestion;

  /// The runtime entity handle a step diagnostic named (never an authoring
  /// id — [id] is that, when the game knew one).
  final EntityRef? entity;

  /// `state` records only.
  final String? state;

  /// `stats` records only: the window's numbers, as sent.
  final Map<String, Object?>? stats;
  final List<GameRecord> related;

  /// The line as it arrived (a record's JSON, or the printed line).
  final String? raw;

  /// The Dart stack an `exception` record carries (`EditorChannel.error`).
  final String? stack;

  /// The stack's frames — or the one frame a printed line is (Build and
  /// Run's `flutter: #0 …` lines of an error Flutter presented).
  late final List<DartFrame> frames = parseDartStack(stack ?? (kind == GameRecordKind.output ? message : null));

  bool get isDiagnostic => kind == GameRecordKind.diagnostic;

  /// Whether a Console row can take the user somewhere: the document it
  /// names, or the asset/entity id.
  bool get hasTarget => document != null || id != null;

  /// The identity used to collapse repeats (everything but the time, the
  /// sequence and the tick).
  static String keyOf(GameRecord r) =>
      '${r.kind.name}|${r.severity.name}|${r.code}|${r.source}|${r.document}|${r.pointer}|${r.id}|${r.message}';

  /// `playground.scene.json:12:5`, the pointer, or the entity handle.
  String location({String Function(String path)? relative}) {
    final doc = document == null ? '' : (relative?.call(document!) ?? document!);
    if (line != null) return '$doc:$line${column != null ? ':$column' : ''}';
    if (pointer != null && pointer!.isNotEmpty) return doc.isEmpty ? pointer! : '$doc  $pointer';
    if (doc.isNotEmpty) return doc;
    if (id != null) return id!;
    if (entity != null) return 'entity 0x${entity!.toString()}';
    return '';
  }

  /// One text line, as the Console copies it.
  String get text {
    final where = location();
    final at = tick == null ? '' : 'tick $tick ';
    final what = code == null ? '' : '$code: ';
    return '$at${severity.name} $what$message${where.isEmpty ? '' : ' ($where)'}';
  }

  /// The record inside [line] (a `flutter run` prefix before it is fine),
  /// or the line itself as [GameRecordKind.output]. [stderr] marks what
  /// the process wrote to its error stream.
  static GameRecord parse(String line, {DateTime? at, bool stderr = false}) {
    final when = at ?? DateTime.now();
    final start = line.indexOf('{"fynkel"');
    if (start >= 0) {
      try {
        final decoded = jsonDecode(line.substring(start));
        if (decoded is Map<String, Object?>) {
          final record = fromJson(decoded, at: when);
          if (record != null) return record;
        }
      } on FormatException {
        // Not a record after all (a line that merely mentions one).
      }
    }
    return GameRecord(
      kind: GameRecordKind.output,
      severity: stderr ? Severity.warning : Severity.info,
      message: line,
      at: when,
      source: stderr ? 'stderr' : 'stdout',
      raw: line,
    );
  }

  /// A decoded record, or null when the map is not one.
  static GameRecord? fromJson(Map<String, Object?> j, {DateTime? at}) {
    final kind = switch (j['fynkel']) {
      'log' => GameRecordKind.log,
      'diagnostic' => GameRecordKind.diagnostic,
      'stats' => GameRecordKind.stats,
      'state' => GameRecordKind.state,
      _ => null,
    };
    if (kind == null) return null;
    final when = at ?? DateTime.now();
    final severity = j['severity'] == null
        ? (kind == GameRecordKind.diagnostic ? Severity.error : Severity.info)
        : Severity.parse(j['severity']);
    return GameRecord(
      kind: kind,
      severity: severity,
      message: switch (kind) {
        GameRecordKind.stats => '${j['summary'] ?? 'stats'}',
        GameRecordKind.state => _stateMessage(j),
        _ => '${j['message'] ?? ''}',
      },
      at: when,
      seq: (j['seq'] as num?)?.toInt(),
      tick: (j['tick'] as num?)?.toInt(),
      code: j['code'] as String?,
      source: j['source'] as String? ?? (kind == GameRecordKind.log ? 'game' : null),
      document: j['document'] as String?,
      pointer: j['pointer'] as String?,
      id: j['id'] as String?,
      line: (j['line'] as num?)?.toInt(),
      column: (j['column'] as num?)?.toInt(),
      suggestion: j['suggestion'] as String?,
      entity: j['entity'] == null ? null : EntityRef.fromJson(j['entity']),
      state: j['state'] as String?,
      stats: kind == GameRecordKind.stats ? j : null,
      related: [
        for (final r in (j['related'] as List?) ?? const [])
          if (r is Map<String, Object?>)
            GameRecord(
              kind: GameRecordKind.diagnostic,
              severity: Severity.parse(r['severity']),
              message: '${r['message'] ?? ''}',
              at: when,
              code: r['code'] as String?,
              document: r['document'] as String?,
              pointer: r['pointer'] as String?,
            ),
      ],
      raw: jsonEncode(j),
      stack: j['stack'] as String?,
    );
  }

  /// `state` in words: what the Console shows without opening the record.
  static String _stateMessage(Map<String, Object?> j) {
    final state = '${j['state'] ?? ''}';
    return switch (state) {
      'started' => 'the game process started (pid ${j['pid'] ?? '?'})',
      'loaded' =>
        'loaded ${j['scene'] ?? 'a scene'} — ${j['entities'] ?? 0} entities, '
            '${j['behaviours'] ?? 0} behaviours, ${j['assets'] ?? 0} assets',
      'loadFailed' => 'the load of ${j['scene'] ?? j['document'] ?? 'a scene'} failed',
      _ => state.isEmpty ? 'state' : state,
    };
  }
}
