// The project's cook on the session's thread, one at a time (ADR-0040 §4):
// what the "Importing Assets" window shows and what Play, Rebuild Cache
// and the Scene view's automatic rebuild wait for.
import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../documents/editor_document.dart';

/// Runs cooks through a [PreviewBackend], one at a time. A request while
/// one runs waits for it, then every request that arrived meanwhile shares
/// one more cook (a live one when any of them asked for live), so a burst
/// of file changes cooks twice at most.
final class CookCoordinator extends ChangeNotifier {
  CookCoordinator({
    this.showAfter = const Duration(milliseconds: 300),
    this.pollEvery = const Duration(milliseconds: 16),
  });

  /// A cook shorter than this shows no window (the editor still takes no
  /// edits while it runs).
  final Duration showAfter;

  /// How often the running cook is asked how far it got.
  final Duration pollEvery;

  CookRun? _run;
  Completer<CookReport?>? _current;
  _Request? _next;
  Timer? _timer;
  Timer? _showTimer;
  bool _shown = false;

  /// Why the last cook could not start, or null.
  String? error;

  /// A cook runs: the editor takes no edits.
  bool get isRunning => _run != null;

  /// The window is up: a cook has run for longer than [showAfter].
  bool get isShowing => _shown;

  int get done => _run?.done ?? 0;
  int get total => _run?.total ?? 0;

  /// The asset the cook last started, relative to the assets folder.
  String get current => _run?.current ?? '';

  /// The share finished, 0 to 1.
  double get progress => total == 0 ? 0 : done / total;

  /// Cooks the project through [backend]; every preview reloads over the
  /// result. Completes with the report ([CookReport.cancelled] after
  /// [cancel]), or null when the cook could not start ([error] says why)
  /// or a waiting request was cancelled.
  Future<CookReport?> cook(PreviewBackend backend, {bool live = false}) {
    if (_run == null) return _start(backend, live);
    final next = _next ??= _Request(backend, live);
    next
      ..backend = backend
      ..live = next.live || live;
    return next.done.future;
  }

  /// Stops the running cook at its next asset, and drops the one waiting.
  void cancel() {
    _run?.cancel();
    final next = _next;
    _next = null;
    next?.done.complete(null);
    notifyListeners();
  }

  Future<CookReport?> _start(PreviewBackend backend, bool live) {
    final CookRun run;
    try {
      error = null;
      run = backend.startCook(live: live);
    } on AuthoringException catch (e) {
      error = '$e';
      return Future.value(null);
    } on StateError catch (e) {
      error = '$e';
      return Future.value(null);
    }
    _run = run;
    final done = _current = Completer<CookReport?>();
    _timer = Timer.periodic(pollEvery, (_) => _tick());
    _showTimer = Timer(showAfter, () {
      _shown = true;
      notifyListeners();
    });
    notifyListeners();
    return done.future;
  }

  void _tick() {
    final run = _run;
    if (run == null) return;
    bool ended;
    try {
      ended = run.poll();
    } on Object catch (e) {
      // The preview closed under the cook (its tab or the project went).
      error = '$e';
      _end(null);
      return;
    }
    if (!ended) {
      notifyListeners();
      return;
    }
    CookReport? report;
    try {
      report = run.finish();
    } on Object catch (e) {
      error = '$e';
    }
    _end(report);
  }

  void _end(CookReport? report) {
    _timer?.cancel();
    _timer = null;
    _showTimer?.cancel();
    _showTimer = null;
    _run = null;
    _shown = false;
    final done = _current;
    _current = null;
    final next = _next;
    _next = null;
    if (next != null) {
      // The waiting requests' cook; its window shows on its own clock.
      unawaited(_start(next.backend, next.live).then(next.done.complete));
    }
    notifyListeners();
    done?.complete(report);
  }

  @override
  void dispose() {
    _timer?.cancel();
    _showTimer?.cancel();
    _run?.cancel();
    super.dispose();
  }
}

final class _Request {
  _Request(this.backend, this.live);

  PreviewBackend backend;
  bool live;
  final done = Completer<CookReport?>();
}
