// A `Process` a test drives by hand: the editor's Play spawns one of these
// instead of `fynkel run`, so a widget test can feed the channel's records
// line by line and end the run with any exit code — no build, no window.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

final class FakePlayProcess implements Process {
  FakePlayProcess({this.pid = 4242});

  @override
  final int pid;

  final _stdout = StreamController<List<int>>();
  final _stderr = StreamController<List<int>>();
  final _exit = Completer<int>();
  final stdinWrites = <String>[];

  /// True once [kill] was called (with which signal).
  final signals = <ProcessSignal>[];

  @override
  Stream<List<int>> get stdout => _stdout.stream;

  @override
  Stream<List<int>> get stderr => _stderr.stream;

  @override
  Future<int> get exitCode => _exit.future;

  @override
  IOSink get stdin => IOSink(_StdinSink(stdinWrites));

  /// Writes one line on the process's stdout (a channel record or not).
  /// The caller pumps (a widget test) or awaits [settle] (a plain test).
  void emit(String line) => _stdout.add(utf8.encode('$line\n'));

  void emitError(String line) => _stderr.add(utf8.encode('$line\n'));

  /// Ends the process, as an exit or a crash (`-9` is a `kill -9`).
  void exitWith(int code) {
    unawaited(_stdout.close());
    unawaited(_stderr.close());
    if (!_exit.isCompleted) _exit.complete(code);
  }

  /// Lets the pending lines through (plain tests only — a widget test
  /// pumps instead).
  static Future<void> settle() => Future<void>.delayed(Duration.zero);

  @override
  bool kill([ProcessSignal signal = ProcessSignal.sigterm]) {
    signals.add(signal);
    return true;
  }
}

class _StdinSink implements StreamConsumer<List<int>> {
  _StdinSink(this.writes);

  final List<String> writes;

  @override
  Future<void> addStream(Stream<List<int>> stream) async {
    await for (final chunk in stream) {
      writes.add(utf8.decode(chunk));
    }
  }

  @override
  Future<void> close() async {}
}
