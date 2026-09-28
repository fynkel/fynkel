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

  /// A [SceneManager.loadAsync] is not finished yet.
  loading,

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
    _set(_resting);
    for (final c in waiting) {
      c.completeError(StateError('scene transition cancelled'));
    }
  }

  /// Starts loading [sceneId] in the background (Unity's
  /// `LoadSceneAsync`, ADR-0038): the scene is read and every asset it uses
  /// loaded on another thread while the game runs on — the old level
  /// keeps playing, a loading screen animates. Once that is done
  /// ([SceneLoad.isReady]) the load activates at the start of the next
  /// [World.frame]: its objects are made and, for [LoadMode.replace], the
  /// world is taken down then, not before. [state] is
  /// [SceneState.loading] while any load is unfinished.
  ///
  /// Loads activate in the order they were asked for. One held with
  /// [SceneLoad.allowActivation] false holds only itself; one still
  /// loading holds the loads after it. A scene missing from the cache fails
  /// at activation ([SceneLoad.done] completes with the
  /// [FynkelLoadError], [error] has it). Throws [FynkelLoadError] at once
  /// when the cache does not open; [StateError] without a cache.
  SceneLoad loadAsync(Object sceneId, {LoadMode mode = LoadMode.replace, String? cacheDir}) {
    final id = World._assetId(sceneId, 'sceneId');
    final cache = cacheDir ?? world._cacheDir;
    if (cache == null) {
      throw StateError('loadAsync($id): no cache directory — pass cacheDir, or loadProject first');
    }
    final (status, ticket, json) = world._native.loadSceneAsync(cache, id);
    if (status != FynkelStatus.FYNKEL_STATUS_OK) {
      try {
        world._load((status, json), 'loadSceneAsync');
      } on FynkelLoadError catch (e) {
        error = e;
        _set(SceneState.failed);
        rethrow;
      }
    }
    final load = SceneLoad._(this, id, mode, cache, ticket);
    _loads.add(load);
    _set(SceneState.loading);
    return load;
  }

  /// The [loadAsync]s not finished, in the order they were asked for.
  List<SceneLoad> get loading => List.unmodifiable(_loads);
  final List<SceneLoad> _loads = [];

  /// Where the flow rests with nothing failing: a transition pending,
  /// a load unfinished, a scene loaded, or nothing.
  SceneState get _resting => _pendingId != null
      ? SceneState.pending
      : _loads.isNotEmpty
          ? SceneState.loading
          : _current == null && world.loadedScenes.isEmpty
              ? SceneState.empty
              : SceneState.ready;

  /// Called by [World.frame] before the frame begins: the pending
  /// [transition], then the [loadAsync]s ready to activate.
  void _perform() {
    _performTransition();
    for (final load in _loads.toList(growable: false)) {
      if (!load.allowActivation) continue; // holds only itself
      if (!load.isReady) break; // keeps the request order
      _loads.remove(load);
      load._activate();
    }
  }

  void _performTransition() {
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
    _set(_loads.isEmpty ? SceneState.ready : SceneState.loading);
  }

  /// Called by [World.unloadScene] on success.
  void _unloaded(UnloadReport report) {
    if (report.sceneId == _current) _current = null;
    if (world.loadedScenes.isEmpty && _pendingId == null && _loads.isEmpty) _set(SceneState.empty);
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

/// A scene loading in the background ([SceneManager.loadAsync], Unity's
/// `AsyncOperation`, ADR-0038). Show [progress] on a loading bar, `await`
/// [done], hold the switch with [allowActivation] (a "press any key"
/// screen), or [cancel] it.
class SceneLoad {
  SceneLoad._(this._manager, this.sceneId, this.mode, this._cache, this._ticket);

  final SceneManager _manager;
  final String _cache;
  final int _ticket;

  /// The scene's asset id.
  final String sceneId;
  final LoadMode mode;

  /// When false, a ready load waits — the old level plays on — until the
  /// game sets it back to true. Unlike Unity, a held load does not hold
  /// the loads asked for after it.
  bool allowActivation = true;

  final Completer<LoadReport> _done = Completer<LoadReport>()..future.ignore();

  /// Completes when the load activated, with the same report as
  /// [World.loadScene]; with the [FynkelLoadError] when it failed, a
  /// [StateError] when it was cancelled.
  Future<LoadReport> get done => _done.future;

  /// Activated, failed or cancelled.
  bool get isDone => _done.isCompleted;

  int _state = NativeWorld.loadLoading;
  double _progress = 0;

  void _poll() {
    if (isDone || _state != NativeWorld.loadLoading) return;
    final p = _manager.world._native.loadProgress(_ticket);
    if (p == null) return;
    _state = p.$1;
    _progress = p.$2;
  }

  /// The share of the scene's assets loaded, 0 to 1. It reaches 1 when
  /// the load is ready, not at 0.9 as in Unity.
  double get progress {
    if (isDone) return 1;
    _poll();
    return _progress;
  }

  /// The background work is done: the load activates at the next frame
  /// (or waits, when [allowActivation] is false).
  bool get isReady {
    if (isDone) return false;
    _poll();
    return _state != NativeWorld.loadLoading;
  }

  /// Drops the load if it has not activated: its work stops, [done]
  /// completes with a [StateError]. Unity cannot do this.
  void cancel() {
    if (isDone) return;
    _manager.world._native.loadCancel(_ticket);
    _manager._loads.remove(this);
    _manager._set(_manager._resting);
    _done.completeError(StateError('scene load cancelled: $sceneId'));
  }

  void _activate() {
    final world = _manager.world;
    final flags = mode == LoadMode.additive ? NativeWorld.loadAdditive : 0;
    try {
      final report = world._load(world._native.loadActivate(_ticket, flags), 'loadSceneAsync');
      world._cacheDir = _cache;
      _manager._loaded(report, mode);
      _done.complete(report);
    } on FynkelLoadError catch (e, st) {
      _manager.error = e;
      _manager._set(_manager._loads.isEmpty ? SceneState.failed : SceneState.loading);
      _done.completeError(e, st);
    }
  }
}
