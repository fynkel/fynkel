// The editor channel (ADR-0016, blueprint §19.2e item 8): when the game
// process is started with `FYNKEL_EDITOR_CHANNEL=stdio` in its environment
// — which is what the editor's Play does, through `fynkel run` — the SDK
// writes one JSON object per line on stdout for every log, diagnostic,
// stats window and state change; with the variable unset it writes nothing
// and costs nothing (one bool read per call site).
//
// The records, all carrying `fynkel` (the kind) and `seq` (a per-process
// counter, so the editor sees a dropped line):
//
//   { "fynkel": "log",        "tick": 42, "source": "game", "severity": "info", "message": "…" }
//   { "fynkel": "log",        "source": "exception", "severity": "error", "message": "…",
//     "stack": "#0      Player.update (package:game/game/player.dart:42:7)\n…" }
//   { "fynkel": "diagnostic", "tick": 42, "code": "FYN-R0003", "severity": "error",
//     "message": "…", "document": "assets/scenes/main.scene.json", "pointer": "/entities/…",
//     "id": "<authoring id>", "line": 12, "column": 5, "suggestion": "…", "related": [ … ] }
//   { "fynkel": "stats",      "tick": 600, "fps": 119.8, … , "summary": "frames=120 …" }
//   { "fynkel": "state",      "tick": 0, "state": "loaded", "scene": "Playground", … }
//
// The `diagnostic` record is the §14.2 document-diagnostic record (the same
// fields `fynkel validate --json` writes), so the editor's Console shows a
// runtime diagnostic exactly like an authoring one and can open the
// document it names. The channel is one-directional (game → editor): the
// editor's Stop is a signal to the process, never a line on stdin.
import 'dart:convert';
import 'dart:io';

/// Writes the editor channel's records; [enabled] decides whether anything
/// is written at all. Everything here is static: the channel belongs to the
/// process, not to a world (a host with two worlds writes one stream).
abstract final class EditorChannel {
  /// The environment variable the editor sets on the game process.
  static const variable = 'FYNKEL_EDITOR_CHANNEL';

  /// The only value V0 understands: JSON lines on stdout.
  static const stdio = 'stdio';

  static bool _enabled = (Platform.environment[variable] ?? '') == stdio;

  /// True when this process was started by an editor that reads the
  /// channel. Read it before building a record that costs anything.
  static bool get enabled => _enabled;

  /// Where a record goes. The default writes the line to stdout; a test
  /// (or a host that multiplexes its own stream) replaces it.
  static void Function(String line) sink = _writeLine;

  /// This process's id — the `started` record carries it so an editor can
  /// see exactly which process its Play is watching.
  static int get processId => pid;

  static int _seq = 0;

  /// Records written so far — the `seq` of the next one.
  static int get sequence => _seq;

  /// Writes one record of [kind] with [fields] (null values are dropped).
  /// Does nothing when the channel is off.
  static void send(String kind, Map<String, Object?> fields) {
    if (!_enabled) return;
    final record = <String, Object?>{'fynkel': kind, 'seq': _seq++};
    for (final e in fields.entries) {
      if (e.value != null) record[e.key] = e.value;
    }
    try {
      sink(jsonEncode(record));
    } catch (_) {
      // A closed stdout (the editor went away) must never break the game.
    }
  }

  /// A game log line: `Diagnostics.log`, the engine's own notes.
  static void log(String message, {String source = 'game', String severity = 'info', int? tick}) =>
      send('log', {'tick': tick, 'source': source, 'severity': severity, 'message': message});

  /// An uncaught error of the game (a behaviour that threw), with its
  /// Dart [stack]: a `log` record of source `exception`, so the editor can
  /// open the game's own frames in the IDE.
  static void error(String message, {String? stack, int? tick}) =>
      send('log', {'tick': tick, 'source': 'exception', 'severity': 'error', 'message': message, 'stack': stack});

  /// A §14.2 diagnostic record. [related] are the same maps, one level deep.
  static void diagnostic({
    required String code,
    required String severity,
    required String message,
    int? tick,
    String? source,
    String? document,
    String? pointer,
    String? id,
    int? line,
    int? column,
    String? suggestion,
    int? entity,
    List<Map<String, Object?>>? related,
  }) =>
      send('diagnostic', {
        'tick': tick,
        'source': source,
        'code': code,
        'severity': severity,
        'message': message,
        'document': document,
        'pointer': pointer,
        'id': id,
        'line': line,
        'column': column,
        'suggestion': suggestion,
        'entity': entity,
        'related': related == null || related.isEmpty ? null : related,
      });

  /// One reporting window of the host's loop (`FynkelGameView.onStats`).
  static void stats(Map<String, Object?> window, {int? tick}) => send('stats', {'tick': tick, ...window});

  /// A state change of the game process: `started`, `loaded`, `stopping`.
  static void state(String state, {int? tick, Map<String, Object?> fields = const {}}) =>
      send('state', {'tick': tick, 'state': state, ...fields});

  static void _writeLine(String line) => stdout.writeln(line);

  /// Turns the channel on or off and points it at [sink] — for the SDK's
  /// own tests, and for a host that owns its stdout. Pass nothing to put
  /// the process back to what the environment says.
  static void debugUse({bool? enabled, void Function(String line)? sink}) {
    _enabled = enabled ?? (Platform.environment[variable] ?? '') == stdio;
    EditorChannel.sink = sink ?? _writeLine;
    _seq = 0;
  }
}
