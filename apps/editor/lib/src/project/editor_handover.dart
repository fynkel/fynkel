// Opening a game project hands it over to its project editor (ADR-0021
// P3c), as Unity Hub opens a project in its editor. The editor starts
// `fynkel editor <dir> --handover` on its own (detached: it outlives this
// editor, its output goes to `.fynkel/editor_host/editor.log`), follows the
// project's `status.json` — `building` with Flutter's progress, then
// `running` — and quits once the project editor is up, so Play runs inside
// it. A build that fails says why here, and the project can still open in
// this editor, without the game. A project that is not a game (no
// `pubspec.yaml` or no `lib/fynkel_game.dart`) opens here, as before.

import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show MethodChannel;
import 'package:path/path.dart' as p;

import '../play/code_reload.dart';

/// Whether [dir] is a game `fynkel editor` can compile into an editor: the
/// CLI's rule (`game_package`).
bool isGameProject(String dir) =>
    File(p.join(dir, 'pubspec.yaml')).existsSync() && File(p.join(dir, 'lib', 'fynkel_game.dart')).existsSync();

/// Starts `fynkel editor` detached, its output into [log]; its pid.
typedef StartEditorProcess = Future<int> Function(String executable, List<String> arguments, {required String log});

Future<int> startDetachedEditor(String executable, List<String> arguments, {required String log}) async {
  final process = await Process.start(
    '/bin/sh',
    ['-c', r'exec "$0" "$@" >"$FYNKEL_HANDOVER_LOG" 2>&1 </dev/null', executable, ...arguments],
    mode: ProcessStartMode.detached,
    // This editor's own status file is not the new one's.
    environment: {'FYNKEL_HANDOVER_LOG': log, 'FYNKEL_EDITOR_STATUS': ''},
  );
  return process.pid;
}

/// Ends a detached `fynkel editor` and everything it started (Flutter's
/// tool, the build, the app): they share its process group.
void killEditorProcess(int pid) {
  unawaited(
    Process.run('/bin/sh', ['-c', r'g=$(ps -o pgid= -p "$0" | tr -d " ") && [ -n "$g" ] && kill -TERM -- "-$g"', '$pid']),
  );
}

/// This window's frame, `[x, y, w, h]` in screen points (origin bottom
/// left): the project editor opens in its place, so the handover looks like
/// this window turning into it. Null in full screen, or without the runner.
Future<List<double>?> currentWindowFrame() async {
  try {
    final f = await const MethodChannel(
      'fynkel.editor/window',
    ).invokeListMethod<num>('frame').timeout(const Duration(milliseconds: 300));
    return f == null || f.length != 4 ? null : [for (final v in f) v.toDouble()];
  } on Object {
    // No runner (a test: no answer at all), or one without "frame": the
    // editor opens where it did.
    return null;
  }
}

enum HandoverState { building, running, failed, cancelled }

/// One handover: the process started and what its status file says.
final class EditorHandover extends ChangeNotifier {
  EditorHandover({
    required this.dir,
    required this.cli,
    required this.root,
    required this.flutter,
    required this.start,
    this.kill = killEditorProcess,
    this.windowFrame = currentWindowFrame,
    this.interval = const Duration(milliseconds: 300),
  });

  final String dir;
  final String cli;
  final String root;
  final String flutter;
  final StartEditorProcess start;
  final void Function(int pid) kill;
  final Future<List<double>?> Function() windowFrame;
  final Duration interval;
  List<double>? _frame;

  String get _host => p.join(dir, '.fynkel', 'editor_host');
  String get statusPath => p.join(_host, 'status.json');
  String get logPath => p.join(_host, 'editor.log');

  HandoverState state = HandoverState.building;

  /// Flutter's progress while building; why, once failed.
  String? message;
  int? pid;
  CodeReloadStatus? _status;

  List<String> get arguments => [
    'editor',
    dir,
    '--handover',
    '--root',
    root,
    '--flutter',
    flutter,
    if (_frame case final f?) ...['--window-frame', f.map((v) => v.round()).join(',')],
  ];

  /// Starts the project editor and follows it.
  Future<void> begin() async {
    try {
      Directory(_host).createSync(recursive: true);
      _frame = await windowFrame();
      pid = await start(cli, arguments, log: logPath);
    } on Object catch (e) {
      _set(HandoverState.failed, 'Could not start `fynkel editor`: $e');
      return;
    }
    if (state == HandoverState.cancelled) return;
    _status = CodeReloadStatus(statusPath, interval: interval)..addListener(_onStatus);
  }

  void _onStatus() {
    final s = _status?.last;
    // An older run's file, or a line of another `fynkel editor`.
    if (s == null || s.pid != pid || state != HandoverState.building) return;
    switch (s.state) {
      case 'building' || 'starting':
        if (s.message != null) _set(HandoverState.building, s.message);
      case 'running':
        _set(HandoverState.running, null);
      case 'failed' || 'exited':
        _set(HandoverState.failed, s.message ?? 'The project editor stopped before it started.');
    }
  }

  void _set(HandoverState next, String? why) {
    state = next;
    message = why;
    if (next != HandoverState.building) _stop();
    notifyListeners();
  }

  /// Stops waiting and ends the build.
  void cancel() {
    if (state != HandoverState.building) return;
    final id = pid;
    if (id != null) kill(id);
    _set(HandoverState.cancelled, null);
  }

  void _stop() {
    final status = _status;
    if (status == null) return;
    _status = null;
    status.removeListener(_onStatus);
    // Called from its own notification: disposed once that is over.
    scheduleMicrotask(status.dispose);
  }

  @override
  void dispose() {
    _stop();
    super.dispose();
  }
}

enum HandoverOutcome { handedOver, openHere, cancelled }

/// "Opening `name`…" until the project editor is up (then this editor
/// quits), or why it did not come up.
Future<HandoverOutcome> showHandoverDialog(BuildContext context, EditorHandover handover, {required String name}) async {
  final r = await showDialog<HandoverOutcome>(
    animationStyle: AnimationStyle.noAnimation,
    context: context,
    barrierDismissible: false,
    builder: (context) => _HandoverDialog(handover: handover, name: name),
  );
  return r ?? HandoverOutcome.cancelled;
}

class _HandoverDialog extends StatefulWidget {
  const _HandoverDialog({required this.handover, required this.name});

  final EditorHandover handover;
  final String name;

  @override
  State<_HandoverDialog> createState() => _HandoverDialogState();
}

class _HandoverDialogState extends State<_HandoverDialog> {
  EditorHandover get h => widget.handover;

  @override
  void initState() {
    super.initState();
    h.addListener(_changed);
  }

  @override
  void dispose() {
    h.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    if (!mounted) return;
    if (h.state == HandoverState.running) {
      Navigator.pop(context, HandoverOutcome.handedOver);
    } else {
      setState(() {});
    }
  }

  static const _openHereLabel = 'Open Without the Game';

  void _openHere() {
    h.cancel();
    Navigator.pop(context, HandoverOutcome.openHere);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final small = theme.textTheme.bodySmall;
    final failed = h.state == HandoverState.failed;
    return AlertDialog(
      key: const Key('dialog.handover'),
      title: Text(failed ? '${widget.name} did not open in its project editor' : 'Opening ${widget.name}…'),
      content: SizedBox(
        width: 460,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (!failed) ...[
              const Text(
                'Starting its editor, with the game compiled in, so Play runs inside it. '
                'The first time builds it (15–30 s); later opens take about a second. '
                'The project editor opens in this window\'s place.',
              ),
              const SizedBox(height: 12),
              const LinearProgressIndicator(),
              if (h.message != null) ...[
                const SizedBox(height: 8),
                Text(h.message!, key: const Key('handover.progress'), style: small),
              ],
            ] else ...[
              ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 260),
                child: SingleChildScrollView(
                  child: SelectableText(
                    h.message ?? '',
                    key: const Key('handover.error'),
                    style: small?.copyWith(fontFamily: 'Menlo'),
                  ),
                ),
              ),
              const SizedBox(height: 8),
              Text('The whole log: ${h.logPath}', style: small),
              const SizedBox(height: 4),
              Text('It can still open here, without the game: Play then runs it in its own window.', style: small),
            ],
          ],
        ),
      ),
      actions: [
        if (failed)
          TextButton(
            key: const Key('handover.close'),
            onPressed: () => Navigator.pop(context, HandoverOutcome.cancelled),
            child: const Text('Close'),
          )
        else
          TextButton(
            key: const Key('handover.cancel'),
            onPressed: () {
              h.cancel();
              Navigator.pop(context, HandoverOutcome.cancelled);
            },
            child: const Text('Cancel'),
          ),
        if (failed)
          FilledButton(key: const Key('handover.openHere'), onPressed: _openHere, child: const Text(_openHereLabel))
        else
          TextButton(key: const Key('handover.openHere'), onPressed: _openHere, child: const Text(_openHereLabel)),
      ],
    );
  }
}
