// What `fynkel editor` says about the project's code (ADR-0021 P3): it
// watches lib/, hot reloads this editor on save and writes each outcome to
// `<project>/.fynkel/editor_host/status.json`, whose path it passes as
// `FYNKEL_EDITOR_STATUS`. The editor reads the file (a stat every
// [interval], a read when it changed) so a reload — or a compile error —
// shows here, not only in the terminal that started the editor.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

/// One line of the status file (version 1).
@immutable
class CodeReload {
  const CodeReload({
    required this.seq,
    required this.state,
    this.message,
    this.ms,
    this.files = const [],
    this.pid,
  });

  factory CodeReload.fromJson(Map<String, Object?> json) => CodeReload(
    seq: (json['seq'] as num?)?.toInt() ?? 0,
    state: json['state'] as String? ?? 'unknown',
    message: json['message'] as String?,
    ms: (json['ms'] as num?)?.toInt(),
    files: [for (final f in json['files'] as List? ?? const []) '$f'],
    pid: (json['pid'] as num?)?.toInt(),
  );

  /// Grows with every write; a new value is a new event.
  final int seq;

  /// `building` (a handover, P3c), `starting`, `running`, `reloading`,
  /// `reloaded`, `reloadFailed`, `failed`, `exited`.
  final String state;

  /// Why a reload failed (the compiler's or the VM's words).
  final String? message;
  final int? ms;

  /// The files the reload was for, relative to the project.
  final List<String> files;

  /// The `fynkel editor` process that wrote it.
  final int? pid;

  bool get failed => state == 'reloadFailed';
}

/// Follows the status file; [last] is its newest event.
class CodeReloadStatus extends ChangeNotifier {
  CodeReloadStatus(this.path, {this.interval = const Duration(milliseconds: 400), bool start = true}) {
    if (start) _timer = Timer.periodic(interval, (_) => poll());
  }

  /// `FYNKEL_EDITOR_STATUS` when this editor was started by `fynkel editor`.
  static String? get environmentPath {
    final p = Platform.environment['FYNKEL_EDITOR_STATUS'];
    return p == null || p.isEmpty ? null : p;
  }

  final String path;
  final Duration interval;
  Timer? _timer;
  DateTime? _modified;
  CodeReload? _last;

  CodeReload? get last => _last;

  /// Reads the file when it changed; notifies on a new event.
  void poll() {
    final file = File(path);
    final DateTime modified;
    try {
      modified = file.lastModifiedSync();
    } on FileSystemException {
      return;
    }
    if (modified == _modified) return;
    _modified = modified;
    final Object? json;
    try {
      json = jsonDecode(file.readAsStringSync());
    } on Object {
      return; // Mid-write elsewhere; the rename lands on the next poll.
    }
    if (json is! Map<String, Object?>) return;
    final next = CodeReload.fromJson(json);
    if (next.seq == _last?.seq) return;
    _last = next;
    notifyListeners();
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}
