part of 'world.dart';

/// Host phases surround the native registered tick. Native simulation/post-
/// physics systems run inside that tick; cross-boundary ordering is by phase.
enum GameplayPhase { fixedUpdate, afterPhysics, update, lateUpdate }

class GameplaySystemContext {
  const GameplaySystemContext(this.world, this.phase, this.tick, this.dt);
  final World world;
  final GameplayPhase phase;
  final int tick;
  final double dt;
}

class GameplaySystem {
  GameplaySystem({
    required this.id,
    required this.phase,
    required this.run,
    List<String> before = const [],
    List<String> after = const [],
    List<String> reads = const [],
    List<String> writes = const [],
    this.structural = false,
  }) : before = List.unmodifiable(before),
       after = List.unmodifiable(after),
       reads = List.unmodifiable(reads),
       writes = List.unmodifiable(writes);
  final String id;
  final GameplayPhase phase;
  final void Function(GameplaySystemContext) run;
  final List<String> before, after, reads, writes;
  final bool structural;
}

class GameplaySystemTime {
  const GameplaySystemTime(this.id, this.phase, this.microseconds, this.failed);
  final String id;
  final GameplayPhase phase;
  final int microseconds;
  final bool failed;
}

class GameplaySystemFailure {
  const GameplaySystemFailure(this.id, this.error, this.stack);
  final String id;
  final Object error;
  final StackTrace stack;
}

extension RegisteredGameplaySystems on World {
  /// Validates and installs a batch as one setup operation. Dependencies can
  /// refer to another member of the batch. Ties use stable lexical IDs.
  void registerSystems(List<GameplaySystem> systems) {
    _rejectDuringEdit('system registration');
    if (isDisposed || _runningSystems) {
      throw StateError('cannot register systems now');
    }
    final all = <String, GameplaySystem>{
      for (final phase in GameplayPhase.values)
        'fynkel.behaviours.${phase.name}': GameplaySystem(
          id: 'fynkel.behaviours.${phase.name}',
          phase: phase,
          run: (_) {},
          reads: const ['*'],
          writes: const ['*'],
          structural: true,
        ),
      for (final system in _systems) system.id: system,
    };
    for (final system in systems) {
      if (!RegExp(r'^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)+$')
              .hasMatch(system.id) ||
          system.id.startsWith('fynkel.')) {
        throw ArgumentError('invalid or reserved system ID ${system.id}');
      }
      if (all.containsKey(system.id)) {
        throw StateError('duplicate system ${system.id}');
      }
      for (final type in [...system.reads, ...system.writes]) {
        if (type != '*' &&
            !_componentTypes.containsKey(type) &&
            !gen.componentCodecs.values.any((c) => c.name == type)) {
          throw StateError('${system.id}: unknown component $type');
        }
      }
      all[system.id] = system;
    }
    final edges = {for (final id in all.keys) id: <String>{}};
    for (final system in all.values) {
      void edge(String from, String to) {
        if (!all.containsKey(from) || !all.containsKey(to)) {
          throw StateError('${system.id}: unknown dependency $from -> $to');
        }
        if (all[from]!.phase.index > all[to]!.phase.index) {
          throw StateError(
            'dependency crosses a phase backwards: $from -> $to',
          );
        }
        edges[from]!.add(to);
      }

      for (final id in system.before) {
        edge(system.id, id);
      }
      for (final id in system.after) {
        edge(id, system.id);
      }
      for (final other in all.values) {
        if (system.phase.index < other.phase.index) edge(system.id, other.id);
      }
    }
    final degree = {for (final id in all.keys) id: 0};
    for (final targets in edges.values) {
      for (final id in targets) {
        degree[id] = degree[id]! + 1;
      }
    }
    final result = <GameplaySystem>[];
    while (result.length < all.length) {
      final ready = degree.keys.where((id) => degree[id] == 0).toList()..sort();
      if (ready.isEmpty) {
        throw StateError('system dependency cycle: ${degree.keys.join(', ')}');
      }
      final id = ready.first;
      degree.remove(id);
      result.add(all[id]!);
      for (final target in edges[id]!) {
        degree[target] = degree[target]! - 1;
      }
    }
    _systems = List.unmodifiable(
      result.where((s) => !s.id.startsWith('fynkel.')),
    );
    _systemOrder = List.unmodifiable(result.map((s) => s.id));
  }

  List<GameplaySystemTime> get gameplaySystemTimes =>
      List.unmodifiable(_systemTimes.values);
  List<GameplaySystemFailure> drainSystemFailures() {
    final result = List<GameplaySystemFailure>.of(_systemFailures);
    _systemFailures.clear();
    return result;
  }

  void _runPhase(GameplayPhase phase, void Function() behaviours) {
    if (_systemOrder.isEmpty) {
      behaviours();
      return;
    }
    _runningSystems = true;
    try {
      for (final id in _systemOrder) {
        if (isDisposed || _callbackEntryBlocked) break;
        if (id == 'fynkel.behaviours.${phase.name}') {
          behaviours();
          continue;
        }
        final system = _systems
            .where((s) => s.id == id && s.phase == phase)
            .firstOrNull;
        if (system == null) continue;
        final watch = Stopwatch()..start();
        var failed = false;
        try {
          context.run(
            () => _callbackGameplay(
              () => system.run(
                GameplaySystemContext(
                  this,
                  phase,
                  phase == GameplayPhase.fixedUpdate ? tick + 1 : tick,
                  phase.index < 2 ? fixedDt : _frameDt,
                ),
              ),
            ),
          );
        } on _GameplayCallbackEntrySkipped {
          break;
        } catch (error, stack) {
          _systemFailures.add(GameplaySystemFailure(id, error, stack));
          failed = true;
        } finally {
          _systemTimes[id] = GameplaySystemTime(
            id,
            phase,
            watch.elapsedMicroseconds,
            failed,
          );
        }
      }
    } finally {
      _runningSystems = false;
    }
  }
}
