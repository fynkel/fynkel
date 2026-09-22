// Play: the project's real app in a separate process (ADR-0016, blueprint
// §19.2e item 8). The editor shells out to `fynkel run --project <dir>`
// with `FYNKEL_EDITOR_CHANNEL=stdio` in its environment — the third and
// last CLI command the editor uses — and reads the process's stdout and
// stderr into the Console's Game tab. Nothing flows the other way: the
// channel is one-directional, and Stop is a signal to the process tree,
// never a line on its stdin.
//
// The editor never loads the game's code and never writes a document for
// it: a crash, or a `kill -9` from the outside, ends in an exit code on the
// toolbar and leaves every document exactly as it was.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import 'game_record.dart';
import 'process_tree.dart';

/// Starts the process — injectable so tests never spawn one.
typedef StartPlayProcess = Future<Process> Function(
  String executable,
  List<String> arguments, {
  String? workingDirectory,
  Map<String, String>? environment,
});

Future<Process> spawnPlayProcess(
  String executable,
  List<String> arguments, {
  String? workingDirectory,
  Map<String, String>? environment,
}) => Process.start(executable, arguments, workingDirectory: workingDirectory, environment: environment);

/// The device Play runs on: the desktop the editor is running on.
///
/// Without `-d`, `flutter run` chooses among everything connected — a
/// phone, every open simulator, Chrome — and when more than one could
/// serve it refuses to choose at all ("More than one device connected",
/// exit code 1) or silently picks a phone. Play means "run it here", so
/// the editor names the desktop and Play means the same thing every time.
/// Running on a device stays the CLI's (`fynkel run -d <id>`) until the
/// editor grows a device picker.
String defaultPlayDevice() {
  if (Platform.isMacOS) return 'macos';
  if (Platform.isWindows) return 'windows';
  return 'linux';
}

enum PlayState {
  /// Nothing has run yet, or the last run is over and was acknowledged.
  idle,

  /// The process is up; the game has not said `started` yet (the CLI is
  /// validating, cooking, building).
  starting,

  /// The game announced itself on the channel.
  running,

  /// Stop asked the tree to go.
  stopping,

  /// The process is gone; [PlaySession.exitCode] says how.
  exited,
}

final class PlaySession extends ChangeNotifier {
  PlaySession({
    required this.projectDir,
    required this.cli,
    this.start = spawnPlayProcess,
    ProcessTree? tree,
    this.environment = const {},
    this.stopGrace = const Duration(seconds: 3),
    this.maxRecords = 5000,
  }) : tree = tree ?? ProcessTree();

  final String projectDir;

  /// The `fynkel` CLI, as the toolchain found it (null: not installed).
  final String? Function() cli;
  final StartPlayProcess start;
  final ProcessTree tree;

  /// Extra environment for the game process (tests inject; the channel
  /// variable is added on top).
  final Map<String, String> environment;
  final Duration stopGrace;

  /// The Game tab keeps this many rows; older ones are dropped (the count
  /// is shown, so nothing disappears silently).
  final int maxRecords;

  PlayState state = PlayState.idle;
  final List<GameRecord> records = [];
  int droppedRecords = 0;

  /// The exit code of the last run (null while one is up, or before any).
  int? exitCode;

  /// Why the last Play could not start (no CLI, the executable is gone).
  String? failure;

  /// The pid of `fynkel run` — the CLI the editor started, the root of the
  /// process tree Stop signals. Not the game: `flutter run` is in between.
  int? pid;

  /// The pid of the game itself, from its `started` record. This is the one
  /// to `kill -9` from a terminal, so it is the one the status line shows.
  int? gamePid;

  /// Whether the game ever announced itself on the channel in this run.
  /// False after a run that died in the CLI — validation, cooking, the
  /// build, or `flutter run` with no device it could pick.
  bool startedGame = false;
  DateTime? startedAt;
  DateTime? endedAt;

  /// The last `stats` record's numbers (the Profiler, item 9, will read
  /// the series; the toolbar shows fps and ticks now).
  Map<String, Object?>? lastStats;

  /// The scene the game says it is in (its `loaded` record).
  String? scene;

  /// A document was edited while this run was up: what is on screen is not
  /// what is playing ("changes apply on next Play").
  bool editedWhilePlaying = false;

  Process? _process;
  Completer<void>? _exited;
  StreamSubscription<String>? _out;
  StreamSubscription<String>? _err;
  bool _stopRequested = false;

  bool get isActive => state == PlayState.starting || state == PlayState.running || state == PlayState.stopping;

  /// True when the last run ended by itself with a non-zero code — a crash,
  /// a build that failed, a `kill -9` from outside.
  bool get crashed => state == PlayState.exited && !_stopRequested && (exitCode ?? 0) != 0;

  /// The run died before the game ever started: the reason is in what
  /// `fynkel run` printed, not in anything the game said. The Console
  /// shows those lines by itself when this is true.
  bool get failedToStart => crashed && !startedGame;

  /// The last run ended because Stop asked it to: the exit code is the
  /// signal that ended it (`-15`), not a fault worth reading as one.
  bool get wasStopped => state == PlayState.exited && _stopRequested;

  /// A game is up and it is running files the project no longer has: what
  /// the banner and the toolbar's suffix are about. Once the run is over
  /// there is nothing stale on screen, so the warning goes with it — the
  /// flag itself stays for the run's record.
  bool get runningOlderFiles => isActive && editedWhilePlaying;

  int get errorCount => records.where((r) => r.severity == Severity.error).length;
  int get warningCount => records.where((r) => r.severity == Severity.warning).length;

  /// What the toolbar says.
  String get statusText => switch (state) {
    PlayState.idle => 'Not playing',
    PlayState.starting => 'Starting…',
    PlayState.running => scene == null ? 'Playing' : 'Playing · $scene',
    PlayState.stopping => 'Stopping…',
    PlayState.exited =>
      wasStopped
          ? 'Stopped'
          : exitCode == 0
          ? 'Exited'
          : failedToStart
          ? 'Did not start (code $exitCode)'
          : 'Exited with code $exitCode',
  };

  /// Starts the game. [extraArguments] go to `fynkel run` after the
  /// project (`--release`, `-d <device>`); the caller has already refused
  /// a project with errors and dealt with dirty documents.
  Future<void> play({List<String> extraArguments = const []}) async {
    if (isActive) return;
    final executable = cli();
    if (executable == null) {
      failure = 'the Fynkel CLI was not found — the launcher\'s checks say where to get it';
      state = PlayState.idle;
      _add(GameRecord.editor('Play needs the `fynkel` CLI: $failure', severity: Severity.error));
      notifyListeners();
      return;
    }
    records.clear();
    droppedRecords = 0;
    failure = null;
    exitCode = null;
    gamePid = null;
    startedGame = false;
    lastStats = null;
    scene = null;
    editedWhilePlaying = false;
    _stopRequested = false;
    state = PlayState.starting;
    startedAt = DateTime.now();
    endedAt = null;
    // The desktop, unless the caller already named a device.
    final named = extraArguments.contains('-d') || extraArguments.contains('--device-id');
    final device = named ? const <String>[] : ['-d', defaultPlayDevice()];
    final arguments = ['run', '--project', projectDir, ...device, ...extraArguments];
    _add(GameRecord.editor('$executable ${arguments.join(' ')}', state: 'launching'));
    notifyListeners();
    final Process process;
    try {
      process = await start(
        executable,
        arguments,
        workingDirectory: projectDir,
        environment: {...environment, 'FYNKEL_EDITOR_CHANNEL': 'stdio'},
      );
    } catch (e) {
      failure = 'could not start `$executable`: $e';
      state = PlayState.idle;
      _add(GameRecord.editor(failure!, severity: Severity.error));
      notifyListeners();
      return;
    }
    _process = process;
    pid = process.pid;
    _exited = Completer<void>();
    // Whatever the previous run still had open ends here.
    unawaited(_out?.cancel());
    unawaited(_err?.cancel());
    _out = process.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen((l) => _line(l));
    _err = process.stderr.transform(utf8.decoder).transform(const LineSplitter()).listen((l) => _line(l, stderr: true));
    unawaited(process.exitCode.then(_reap));
    notifyListeners();
  }

  /// Stops the run: the process tree gets SIGTERM, then SIGKILL after
  /// [stopGrace]. Returns when the process is reaped.
  Future<void> stop() async {
    final process = _process;
    if (process == null || !isActive) return;
    _stopRequested = true;
    state = PlayState.stopping;
    _add(GameRecord.editor('Stop: signalling the process tree of pid ${process.pid}', state: 'stopping'));
    notifyListeners();
    final exited = _exited?.future ?? Future<void>.value();
    final killed = await tree.terminate(process.pid, exited: exited, grace: stopGrace, also: [?gamePid]);
    if (killed.isNotEmpty) {
      _add(
        GameRecord.editor('the tree did not go on SIGTERM; killed ${killed.join(', ')}', severity: Severity.warning),
      );
    }
    await exited;
  }

  /// Empties the Game tab (the run, if any, keeps going).
  void clear() {
    records.clear();
    droppedRecords = 0;
    notifyListeners();
  }

  /// A document changed while the game is up: the toolbar and the window's
  /// banner say the change applies on the next Play.
  void markEdited() {
    if (!isActive || editedWhilePlaying) return;
    editedWhilePlaying = true;
    notifyListeners();
  }

  void _line(String line, {bool stderr = false}) {
    if (line.trim().isEmpty) return;
    final record = GameRecord.parse(line, stderr: stderr);
    switch (record.kind) {
      case GameRecordKind.state:
        switch (record.state) {
          case 'started':
            gamePid = _pidOf(record);
            startedGame = true;
            if (state == PlayState.starting) state = PlayState.running;
          case 'loaded':
            scene = _sceneOf(record);
        }
      case GameRecordKind.stats:
        lastStats = record.stats;
      case _:
        break;
    }
    _add(record);
    notifyListeners();
  }

  static int? _pidOf(GameRecord r) {
    final raw = r.raw;
    if (raw == null) return null;
    final decoded = jsonDecode(raw);
    return decoded is Map ? (decoded['pid'] as num?)?.toInt() : null;
  }

  static String? _sceneOf(GameRecord r) {
    final raw = r.raw;
    if (raw == null) return null;
    final decoded = jsonDecode(raw);
    return decoded is Map ? decoded['scene'] as String? : null;
  }

  void _add(GameRecord record) {
    records.add(record);
    if (records.length > maxRecords) {
      final over = records.length - maxRecords;
      records.removeRange(0, over);
      droppedRecords += over;
    }
  }

  void _reap(int code) {
    exitCode = code;
    state = PlayState.exited;
    endedAt = DateTime.now();
    _process = null;
    // The subscriptions are left alone on purpose: a process that printed
    // and died can still have lines in the pipe, and `exitCode` may well
    // complete before they arrive. They end by themselves when the pipe
    // closes; `dispose` cancels whatever is left.
    _add(
      GameRecord.editor(
        _stopRequested
            ? 'the game stopped (exit code $code)'
            : code == 0
            ? 'the game exited (code 0)'
            : startedGame
            ? 'the game exited with code $code — nothing in the project was changed'
            // The game never ran: `fynkel run` itself gave up. Its own
            // lines say why, and the Console shows them (they are Output
            // rows, which are off while a game is doing the talking).
            : 'the run failed before the game started (exit code $code) — nothing in the project was changed; '
                  'the lines above are what `fynkel run` printed',
        severity: !_stopRequested && code != 0 ? Severity.error : Severity.info,
        state: 'exited',
      ),
    );
    if (!(_exited?.isCompleted ?? true)) _exited!.complete();
    notifyListeners();
  }

  @override
  void dispose() {
    if (_process != null) unawaited(stop());
    unawaited(_out?.cancel());
    unawaited(_err?.cancel());
    super.dispose();
  }
}
