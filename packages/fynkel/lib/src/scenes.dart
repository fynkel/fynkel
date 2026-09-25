// Scene flow and zone events (M3-12, docs/scenes.md): `SceneManager` over
// `World.loadScene`/`unloadScene` with a deferred `transition`, and the
// `ZoneEvents` hub for `fynkel.TriggerZone` events by name.

part of 'world.dart';

/// How [World.loadScene] treats what the world already holds.
enum LoadMode {
  /// Unload every loaded scene and destroy every runtime spawn first.
  replace,

  /// Keep the world; the scene joins it (its id must not be loaded yet).
  additive,
}

/// Where the scene flow is ([SceneManager.state]).
enum SceneState {
  /// Nothing loaded through the manager yet.
  empty,

  /// A transition is requested and runs at the start of the next frame.
  pending,

  /// The last load succeeded; [SceneManager.current] names it.
  ready,

  /// The last transition failed; [SceneManager.error] has the report.
  failed,
}

/// Named zone events (M3-12): callbacks by the `fynkel.TriggerZone`'s
/// event name, run right after the tick whose step produced them (the
/// same point as the contact callbacks), with the world's context current.
class ZoneEvents {
  final Map<String, List<void Function(ZoneEntered)>> _enter = {};
  final Map<String, List<void Function(ZoneExited)>> _exit = {};

  /// Runs [callback] whenever something enters a zone named [event].
  void on(String event, void Function(ZoneEntered e) callback) => (_enter[event] ??= []).add(callback);

  /// Runs [callback] whenever something leaves a zone named [event].
  void onExit(String event, void Function(ZoneExited e) callback) => (_exit[event] ??= []).add(callback);

  /// Removes every callback of [event] (both directions).
  void off(String event) {
    _enter.remove(event);
    _exit.remove(event);
  }

  void clear() {
    _enter.clear();
    _exit.clear();
  }

  void _dispatch(ZoneEvent z) {
    switch (z) {
      case ZoneEntered():
        final cbs = _enter[z.event];
        if (cbs != null) {
          for (final cb in cbs.toList(growable: false)) {
            cb(z);
          }
        }
      case ZoneExited():
        final cbs = _exit[z.event];
        if (cbs != null) {
          for (final cb in cbs.toList(growable: false)) {
            cb(z);
          }
        }
    }
  }
}

/// The scene flow of one world (M3-12): [load] and [unload] now, or
/// [transition] to a scene at the start of the next [World.frame] — the
/// one point where no behaviour callback is running, so a behaviour can
/// ask for the next level from `fixedUpdate` or a zone callback without
/// pulling its own object from under itself. [state] is observable
/// ([addListener]); [current] is the scene of the last replace load.
class SceneManager {
  SceneManager._(this.world);

  final World world;

  SceneState _state = SceneState.empty;
  SceneState get state => _state;

  /// The asset id of the scene the last replace load brought in (the
  /// startup scene after `loadProject`); null before any.
  String? get current => _current;
  String? _current;

  /// The report of the last successful load through the manager.
  LoadReport? lastReport;

  /// The report of the last failed transition ([SceneState.failed]).
  FynkelLoadError? error;

  /// The scene a pending [transition] targets.
  String? get pending => _pendingId;
  String? _pendingId;
  LoadMode _pendingMode = LoadMode.replace;
  final List<Completer<LoadReport>> _pendingCompleters = [];

  final List<void Function(SceneState state)> _listeners = [];

  /// The scenes the world holds ([World.loadedScenes]).
  List<LoadedScene> get loaded => world.loadedScenes;

  bool isLoaded(Object sceneId) {
    final id = World._assetId(sceneId, 'sceneId');
    return world.loadedScenes.any((s) => s.id == id);
  }

  void addListener(void Function(SceneState state) listener) => _listeners.add(listener);
  void removeListener(void Function(SceneState state) listener) => _listeners.remove(listener);

  void _set(SceneState s) {
    _state = s;
    for (final l in _listeners.toList(growable: false)) {
      l(s);
    }
  }

  /// Loads [sceneId] now ([World.loadScene]); a replace load makes it
  /// [current]. Not from inside a behaviour callback — use [transition].
  LoadReport load(Object sceneId, {LoadMode mode = LoadMode.replace, String? cacheDir}) {
    try {
      return world.loadScene(sceneId, mode: mode, cacheDir: cacheDir);
    } on FynkelLoadError catch (e) {
      error = e;
      _set(SceneState.failed);
      rethrow;
    }
  }

  /// Unloads [sceneId] now ([World.unloadScene]).
  UnloadReport unload(Object sceneId) => world.unloadScene(sceneId);

  /// Requests a load of [to] at the start of the next [World.frame]:
  /// [state] is [SceneState.pending] until then, [SceneState.ready] (or
  /// [SceneState.failed], with [error]) after. The returned future
  /// completes when the load ran (with its report, or the
  /// [FynkelLoadError]). A second request before the frame replaces the
  /// first (its future completes with the same outcome).
  Future<LoadReport> transition(Object to, {LoadMode mode = LoadMode.replace}) {
    _pendingId = World._assetId(to, 'to');
    _pendingMode = mode;
    final c = Completer<LoadReport>();
    _pendingCompleters.add(c);
    _set(SceneState.pending);
    return c.future;
  }

  /// Drops a pending [transition] (its futures complete with a
  /// [StateError]).
  void cancel() {
    if (_pendingId == null) return;
    _pendingId = null;
    final waiting = _pendingCompleters.toList(growable: false);
    _pendingCompleters.clear();
    _set(_current == null ? SceneState.empty : SceneState.ready);
    for (final c in waiting) {
      c.completeError(StateError('scene transition cancelled'));
    }
  }

  /// Called by [World.frame] before the frame begins.
  void _perform() {
    final id = _pendingId;
    if (id == null) return;
    _pendingId = null;
    final mode = _pendingMode;
    final waiting = _pendingCompleters.toList(growable: false);
    _pendingCompleters.clear();
    try {
      final report = world.loadScene(id, mode: mode);
      for (final c in waiting) {
        c.complete(report);
      }
    } on FynkelLoadError catch (e, st) {
      error = e;
      _set(SceneState.failed);
      for (final c in waiting) {
        c.completeError(e, st);
      }
    }
  }

  /// Called by [World.loadScene] on success.
  void _loaded(LoadReport report, LoadMode mode) {
    lastReport = report;
    error = null;
    if (mode == LoadMode.replace) _current = report.sceneId;
    _set(SceneState.ready);
  }

  /// Called by [World.unloadScene] on success.
  void _unloaded(UnloadReport report) {
    if (report.sceneId == _current) _current = null;
    if (world.loadedScenes.isEmpty && _pendingId == null) _set(SceneState.empty);
  }

  /// Records a `loadProject` as the manager's current scene; a project
  /// without a startup scene loaded none, and the manager stays [SceneState.empty].
  void _projectLoaded(LoadReport report) {
    if (report.sceneId != null) return _loaded(report, LoadMode.replace);
    lastReport = report;
    error = null;
    _current = null;
  }
}
