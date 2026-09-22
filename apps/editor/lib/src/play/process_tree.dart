// Stop kills the process *tree*: the editor starts `fynkel run`, which
// starts `flutter run`, which starts the game — killing only the child the
// editor holds would leave the game on screen. macOS has no `setsid` to
// put the child in its own group, so the tree is read from `ps` and
// signalled leaf-first (a parent that dies first would leave orphans
// reparented to launchd, out of reach of the next pass).
import 'dart:io';

/// Reads the machine's process table as `pid ppid` lines.
typedef PsReader = Future<String> Function();

/// Sends a signal to one process; false when it is gone already.
typedef SignalSender = bool Function(int pid, ProcessSignal signal);

final class ProcessTree {
  ProcessTree({PsReader? readTable, SignalSender? send})
    : readTable = readTable ?? _ps,
      send = send ?? Process.killPid;

  final PsReader readTable;
  final SignalSender send;

  static Future<String> _ps() async {
    final r = await Process.run('ps', ['-A', '-o', 'pid=,ppid=']);
    return r.exitCode == 0 ? '${r.stdout}' : '';
  }

  /// `parent → children` from `ps -A -o pid=,ppid=` output; lines that are
  /// not two numbers are ignored (a locale's header, a truncated read).
  static Map<int, List<int>> parse(String table) {
    final children = <int, List<int>>{};
    for (final line in table.split('\n')) {
      final parts = line.trim().split(RegExp(r'\s+'));
      if (parts.length < 2) continue;
      final pid = int.tryParse(parts[0]);
      final ppid = int.tryParse(parts[1]);
      if (pid == null || ppid == null || pid == ppid) continue;
      (children[ppid] ??= []).add(pid);
    }
    return children;
  }

  /// [root] and everything under it, deepest first — the order to signal
  /// in. A cycle (which `ps` cannot really report) cannot loop: every pid
  /// is visited once.
  static List<int> flatten(Map<int, List<int>> children, int root) {
    final out = <int>[];
    final seen = <int>{};
    void walk(int pid) {
      if (!seen.add(pid)) return;
      for (final c in children[pid] ?? const <int>[]) {
        walk(c);
      }
      out.add(pid);
    }

    walk(root);
    return out;
  }

  /// The pids of [root]'s tree, deepest first (including [root]).
  Future<List<int>> of(int root) async => flatten(parse(await readTable()), root);

  /// Signals [root]'s whole tree, deepest first; returns the pids that
  /// took the signal (a process that exited between `ps` and the signal
  /// simply is not in the list).
  Future<List<int>> signal(int root, ProcessSignal signal) async {
    final signalled = <int>[];
    for (final pid in await of(root)) {
      if (send(pid, signal)) signalled.add(pid);
    }
    return signalled;
  }

  /// Every pid the table lists — who is still alive after a signal.
  static Set<int> pids(String table) {
    final out = <int>{};
    for (final line in table.split('\n')) {
      final first = line.trim().split(RegExp(r'\s+')).first;
      final pid = int.tryParse(first);
      if (pid != null) out.add(pid);
    }
    return out;
  }

  /// The blueprint's Stop: SIGTERM the tree (plus [also] — the game's own
  /// pid, which the channel's `started` record named), give it [grace] to
  /// go, then SIGKILL whatever of *that same list* is still alive. The
  /// list is taken before the first signal on purpose: once the root dies
  /// its children are reparented to launchd and no walk would find them.
  /// Returns the pids of the second pass — empty when the tree went down
  /// politely.
  Future<List<int>> terminate(
    int root, {
    required Future<void> exited,
    Duration grace = const Duration(seconds: 3),
    Iterable<int> also = const [],
  }) async {
    // A set: the game's own pid is usually in the walk already, and a
    // process must not take the same signal twice.
    final tree = <int>{...await of(root), ...also}.toList();
    for (final pid in tree) {
      send(pid, ProcessSignal.sigterm);
    }
    var quiet = false;
    await Future.any([exited.then((_) => quiet = true), Future<void>.delayed(grace)]);
    final alive = pids(await readTable());
    final killed = <int>[];
    for (final pid in tree) {
      if (!alive.contains(pid)) continue;
      if (send(pid, ProcessSignal.sigkill)) killed.add(pid);
    }
    if (killed.isEmpty && !quiet) await exited;
    return killed;
  }
}
