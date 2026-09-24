// The Console's Game tab: what the running game said (blueprint §19.2e
// item 8). The rows are the editor channel's records and the process's own
// output — tick, severity, source, code, message, and the entity or asset
// id the record names; the filters, the search and the collapsing work
// exactly as the Project tab's, so one Console behaves one way.
//
// A row that names a document opens it with its entity selected, the same
// double-click (or ↩) as a project diagnostic. A row whose Dart stack has a
// frame in the game's own code (an uncaught error of a behaviour) opens the
// IDE at that line instead, and its details list the frames.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import 'dart_stack.dart';
import 'game_record.dart';
import 'play_session.dart';

/// One row: a record and how many identical ones it stands for.
final class GameEntry {
  GameEntry(this.record, this.count);

  final GameRecord record;
  final int count;
}

/// Filters and selection over a [PlaySession]'s records.
final class GameConsoleModel extends ChangeNotifier {
  GameConsoleModel({
    required this.session,
    this.openRecord,
    this.relative,
    this.projectDir,
    this.ownPackage,
    this.openInIde,
  }) {
    session.addListener(_onSession);
  }

  final PlaySession session;

  /// Takes the user to what a record names (the document with the entity
  /// selected, or the asset in the Project panel).
  final void Function(GameRecord record)? openRecord;

  /// Shortens a path for display (the project's `relative`).
  final String Function(String path)? relative;

  /// Where the game's own frames are: `package:<own>/…` is
  /// `<projectDir>/lib/…`.
  final String? projectDir;

  /// The game's package name (asked once it is known).
  final String? Function()? ownPackage;

  /// Opens a source file at a line; answers the error or null.
  final Future<String?> Function(String file, {int? line, int? column})? openInIde;

  /// The last IDE launch's error, shown in the status line.
  String? lastIdeError;

  String? _own;

  String query = '';
  final severities = {Severity.error, Severity.warning, Severity.info};

  /// Whether the process's own lines (the CLI's, Flutter's) are shown;
  /// off by default, so a Play shows the game, not the build log.
  bool showOutput = false;

  /// Whether the per-second `stats` windows are shown (off: the Profiler
  /// is where they belong; the toolbar shows the last one).
  bool showStats = false;
  bool collapse = true;
  String? selectedKey;

  /// The list follows the run unless the user scrolled up (the panel sets
  /// this); a new row then does not steal the view.
  bool follow = true;

  /// Whether this run's failure has already shown its output.
  bool _showedFailureOutput = false;

  /// A run that dies before the game starts leaves its reason in the
  /// process's own lines — `fynkel run`'s validation, the build, the
  /// device it could not pick. Those are Output rows, which are off while
  /// a game is doing the talking, so the Console turns Output on by itself
  /// for that run. The user can switch it off again; the next Play starts
  /// over.
  void _onSession() {
    if (session.state == PlayState.starting) _showedFailureOutput = false;
    if (session.failedToStart && !_showedFailureOutput) {
      _showedFailureOutput = true;
      showOutput = true;
    }
    notifyListeners();
  }

  void setQuery(String q) {
    query = q;
    notifyListeners();
  }

  void toggleSeverity(Severity s) {
    if (!severities.remove(s)) severities.add(s);
    notifyListeners();
  }

  void setShowOutput(bool v) {
    showOutput = v;
    notifyListeners();
  }

  void setShowStats(bool v) {
    showStats = v;
    notifyListeners();
  }

  void setCollapse(bool v) {
    collapse = v;
    notifyListeners();
  }

  void setFollow(bool v) {
    if (follow == v) return;
    follow = v;
    notifyListeners();
  }

  void select(String? key) {
    selectedKey = key;
    notifyListeners();
  }

  bool _shows(GameRecord r) {
    if (!severities.contains(r.severity)) return false;
    if (r.kind == GameRecordKind.output && !showOutput) return false;
    if (r.kind == GameRecordKind.stats && !showStats) return false;
    final q = query.trim().toLowerCase();
    if (q.isEmpty) return true;
    final hay = [
      r.code ?? '',
      r.message,
      r.source ?? '',
      r.document == null ? '' : (relative?.call(r.document!) ?? r.document!),
      r.pointer ?? '',
      r.id ?? '',
      r.tick == null ? '' : 'tick ${r.tick}',
    ].join(' ').toLowerCase();
    return q.split(RegExp(r'\s+')).every(hay.contains);
  }

  List<GameEntry> get entries {
    final counts = <String, int>{};
    final order = <String, GameRecord>{};
    for (final r in session.records) {
      if (!_shows(r)) continue;
      final key = collapse ? GameRecord.keyOf(r) : '${GameRecord.keyOf(r)}#${counts.length}';
      counts[key] = (counts[key] ?? 0) + 1;
      // The newest of a collapsed group is the one shown (its tick is now).
      order[key] = r;
    }
    return [for (final e in order.entries) GameEntry(e.value, counts[e.key]!)];
  }

  GameEntry? get selected {
    final key = selectedKey;
    if (key == null) return null;
    for (final e in entries) {
      if (keyOf(e) == key) return e;
    }
    return null;
  }

  /// The key of a row, in this model's collapse mode.
  String keyOf(GameEntry e) => GameRecord.keyOf(e.record);

  int countOf(Severity s) => session.records.where((r) => r.severity == s && _shows(r)).length;

  /// The record's document, or its own frame's `lib/…:line`.
  String location(GameRecord r) {
    final at = r.location(relative: relative);
    if (at.isNotEmpty) return at;
    final f = ownFrame(r);
    if (f == null) return '';
    final file = sourceOf(f)!;
    return '${relative?.call(file) ?? file}${f.line == null ? '' : ':${f.line}'}';
  }

  String lineOf(GameEntry e) => '${e.record.text}${e.count > 1 ? ' (×${e.count})' : ''}';

  String textOf(Iterable<GameEntry> es) => es.map(lineOf).join('\n');

  /// Copies the selection, or every visible row.
  Future<int> copy() async {
    final sel = selected;
    final es = sel != null ? [sel] : entries;
    if (es.isEmpty) return 0;
    await Clipboard.setData(ClipboardData(text: textOf(es)));
    return es.length;
  }

  /// The file of [f] when it is the game's own code, else null.
  String? sourceOf(DartFrame f) {
    final dir = projectDir;
    if (dir == null) return null;
    return f.sourceIn(dir, ownPackage: _own ??= ownPackage?.call());
  }

  /// The first frame of [r]'s stack in the game's own code — where its
  /// error was thrown, as far as the game's author is concerned.
  DartFrame? ownFrame(GameRecord r) {
    for (final f in r.frames) {
      if (sourceOf(f) != null) return f;
    }
    return null;
  }

  /// Whether ↩ or a double-click on [r] goes somewhere.
  bool canActivate(GameRecord r) => (r.hasTarget && openRecord != null) || (openInIde != null && ownFrame(r) != null);

  bool get canOpenSelected => selected != null && canActivate(selected!.record);

  void openSelected() {
    final r = selected?.record;
    if (r != null) activate(r);
  }

  void activate(GameRecord r) {
    if (r.hasTarget) {
      openRecord?.call(r);
      return;
    }
    final f = ownFrame(r);
    if (f != null) openFrame(f);
  }

  /// Opens [f]'s line in the IDE (only the game's own frames have a file).
  Future<void> openFrame(DartFrame f) async {
    final file = sourceOf(f);
    final open = openInIde;
    if (file == null || open == null) return;
    lastIdeError = await open(file, line: f.line, column: f.column);
    notifyListeners();
  }

  @override
  void dispose() {
    session.removeListener(_onSession);
    super.dispose();
  }
}
