// The Inspector's 3D view (ADR-0030 §3): one asset alone on a preview
// stage — lit by the stage's sun, framed from the front-right as the
// Project's tiles show it — drawn through a viewport of its own on the
// shared GPU device. It draws on demand: when it opens, when the camera
// or the light moves, when the asset (or a material it shows) changes,
// and when its viewport attaches or resizes; never while it stands still.
//
// As Unity's preview: drag to orbit, the wheel (or a trackpad scroll) to
// zoom, the middle button or Shift-drag to pan, the right button to turn
// the light, F or a double-click to frame it again.
import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../inspector/asset_edits.dart';
import '../project/editor_project.dart';
import '../scene_view/scene_math.dart';
import '../theme/tokens.dart';
import 'clip_bar.dart';
import 'stage_backend.dart';

/// What the 3D previews draw with: the project's stages, the Inspector's
/// unsaved material edits, and whether there is a GPU to draw on (widget
/// tests have none).
class PreviewStageScope extends InheritedWidget {
  const PreviewStageScope({super.key, required this.project, this.edits, this.viewports = true, required super.child});

  final EditorProject project;
  final AssetEdits? edits;
  final bool viewports;

  static PreviewStageScope? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<PreviewStageScope>();

  @override
  bool updateShouldNotify(PreviewStageScope old) =>
      old.project != project || old.edits != edits || old.viewports != viewports;
}

/// The stats a stage's state reports, as the editor API gives them.
PreviewStageStats stageStats(StageState state) {
  final s = state.stats;
  final lo = s.boundsMin, hi = s.boundsMax;
  return PreviewStageStats(
    meshes: s.meshes,
    vertices: s.vertices,
    triangles: s.triangles,
    skinned: s.skinned,
    bones: s.bones,
    size: lo == null || hi == null || lo.length < 3 || hi.length < 3
        ? null
        : [for (var i = 0; i < 3; i++) hi[i] - lo[i]],
  );
}

/// The camera a framing starts from: an orbit about its target.
EditorCamera cameraOf(StageFraming f) {
  final d = f.eye.length == 3 && f.target.length == 3 ? vSub(f.target, f.eye) : <double>[-1, -0.8, -1];
  final distance = math.max(vLength(d), EditorCamera.minDistance);
  final dir = vNormalize(d);
  return EditorCamera(
    pivot: f.target.length == 3 ? List.of(f.target) : [0, 0, 0],
    distance: distance,
    yaw: math.atan2(-dir[0], -dir[2]),
    pitch: math.asin(dir[1].clamp(-1.0, 1.0)),
    fovY: f.fovY,
  );
}

class StageView extends StatefulWidget {
  const StageView({super.key, required this.asset, this.sub, this.shape, this.clip, this.onStats});

  final String asset;
  final String? sub;
  final Object? shape;

  /// Played on the model (a blend space's stage plays its own).
  final PreviewClip? clip;
  final void Function(PreviewStageStats stats)? onStats;

  @override
  State<StageView> createState() => StageViewState();
}

class StageViewState extends State<StageView> with SingleTickerProviderStateMixin {
  PreviewStageScope? _scope;
  StageBackend? _stage;
  String? _error;
  EditorCamera? _camera;
  FynkelViewportController? _controller;
  final _focus = FocusNode(debugLabel: 'preview.stage');
  int _revision = -1;
  bool _scheduled = false;
  bool _disposed = false;

  /// Frames drawn (tests).
  int frames = 0;

  // ── The clip's playback: the editor runs the clock, the stage poses ──
  StageClip? _clip;
  bool _playing = false;
  bool _loop = true;
  double _speed = 1;
  late final Ticker _ticker = createTicker(_tick);
  Duration _elapsed = Duration.zero;

  /// The clip posing the model, as the stage last reported it (tests).
  StageClip? get clip => _clip;
  bool get isPlaying => _playing;
  double get speed => _speed;
  bool get loop => _loop;

  /// The stage shown; null while it failed (tests).
  StageBackend? get stage => _stage;
  EditorCamera? get camera => _camera;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final scope = PreviewStageScope.maybeOf(context);
    if (identical(scope?.project, _scope?.project) && identical(scope?.edits, _scope?.edits) && _stage != null) {
      _scope = scope;
      return;
    }
    _detach();
    _scope = scope;
    _attach();
    _open();
  }

  @override
  void didUpdateWidget(covariant StageView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.asset != widget.asset || oldWidget.sub != widget.sub) {
      _open();
    } else if (oldWidget.clip != widget.clip) {
      _putClip();
    } else if (!_sameShape(oldWidget.shape, widget.shape) && widget.shape != null) {
      _setShape(widget.shape!);
    }
  }

  static bool _sameShape(Object? a, Object? b) {
    if (a is Map && b is Map) return a['asset'] == b['asset'] && a['sub'] == b['sub'];
    return a == b;
  }

  void _attach() {
    final scope = _scope;
    if (scope == null) return;
    scope.project.addListener(_onProject);
    scope.edits?.watchers.add(_onEdit);
    _revision = scope.project.assetsRevision;
    if (scope.viewports) {
      _controller = FynkelViewportController(placeholders: false)..addListener(_schedule);
    }
  }

  void _detach() {
    _scope?.project.removeListener(_onProject);
    _scope?.edits?.watchers.remove(_onEdit);
    _controller?.removeListener(_schedule);
    _controller?.dispose();
    _controller = null;
    _stage?.close();
    _stage = null;
  }

  void _open() {
    _stage?.close();
    _stage = null;
    final scope = _scope;
    if (scope == null) {
      _error = 'No project to stage it from.';
      return;
    }
    try {
      StageBackend stage;
      try {
        stage = scope.project.openStage(widget.asset, sub: widget.sub, shape: widget.shape);
      } on AuthoringException catch (e) {
        // A remembered mesh that no longer loads: the default shape.
        if (widget.shape is! Map) rethrow;
        stage = scope.project.openStage(widget.asset, sub: widget.sub);
        final why = e.diagnostics.firstOrNull?.message ?? '$e';
        SchedulerBinding.instance.addPostFrameCallback((_) {
          if (!_disposed) _toast(why);
        });
      }
      _stage = stage;
      _error = null;
      _camera = cameraOf(stage.state.framing);
      // An unsaved edit of the material shown (the store keeps it for
      // every world once set; this covers edits made with no view open).
      if (stage.state.subject == 'material' && widget.sub == null) {
        if (scope.edits?.drawn(widget.asset) case final fields?) stage.setMaterial(widget.asset, fields);
      }
      _stats(stage.state);
      _clip = stage.state.clip;
      if (widget.clip != null) {
        _putClip();
      } else if (_clip != null) {
        _play(true);
      }
    } on AuthoringException catch (e) {
      _error = e.diagnostics.firstOrNull?.message ?? '$e';
    } on UnsupportedError {
      _error = 'This project draws no 3D previews.';
    }
    _schedule();
  }

  /// The widget's clip on the stage, from its start, playing (as Unity's
  /// preview starts it); none takes it off.
  void _putClip() {
    final stage = _stage;
    if (stage == null) return;
    final c = widget.clip;
    try {
      _clip = stage.setClip(keep: false, clip: c == null ? null : {'asset': c.asset, 'sub': c.sub}, time: 0);
    } on AuthoringException catch (e) {
      _clip = null;
      _toast(e.diagnostics.firstOrNull?.message ?? '$e');
    }
    _play(_clip != null);
    _schedule();
  }

  void _play(bool on) {
    if (!mounted) return;
    setState(() => _playing = on);
    if (on && !_ticker.isActive) {
      _elapsed = Duration.zero;
      _ticker.start();
    } else if (!on && _ticker.isActive) {
      _ticker.stop();
    }
  }

  void _tick(Duration elapsed) {
    final clip = _clip;
    final dt = (elapsed - _elapsed).inMicroseconds / 1e6;
    _elapsed = elapsed;
    if (clip == null || !_playing) return;
    final d = clip.duration;
    var time = clip.time + dt * _speed;
    var ended = false;
    if (d <= 0) {
      time = 0;
    } else if (time >= d) {
      if (_loop) {
        time %= d;
      } else {
        time = d;
        ended = true;
      }
    }
    _seek(time);
    if (ended) _play(false);
  }

  /// The model posed at [time] seconds.
  void _seek(double time, {bool? inPlace, double? parameter}) {
    final stage = _stage;
    if (stage == null || stage.isClosed) return;
    try {
      final next = stage.setClip(time: time, inPlace: inPlace, parameter: parameter);
      if (mounted) setState(() => _clip = next);
    } on AuthoringException {
      _play(false);
      return;
    }
    _schedule();
  }

  /// The play button: from the start again once a clip that does not loop
  /// ran out.
  void togglePlay() {
    final clip = _clip;
    if (clip == null) return;
    if (!_playing && !_loop && clip.duration > 0 && clip.time >= clip.duration) _seek(0);
    _play(!_playing);
  }

  void _setShape(Object shape) {
    final stage = _stage;
    if (stage == null) return;
    try {
      _stats(stage.setShape(shape));
      _camera = cameraOf(stage.state.framing);
    } on AuthoringException catch (e) {
      _toast(e.diagnostics.firstOrNull?.message ?? '$e');
    }
    _schedule();
  }

  String? _notice;
  Timer? _noticeTimer;

  void _toast(String message) {
    setState(() => _notice = message);
    _noticeTimer?.cancel();
    _noticeTimer = Timer(const Duration(seconds: 4), () {
      if (mounted) setState(() => _notice = null);
    });
  }

  void _stats(StageState state) {
    final onStats = widget.onStats;
    if (onStats == null) return;
    final stats = stageStats(state);
    // Told after this frame: the area draws its info line from it.
    SchedulerBinding.instance.addPostFrameCallback((_) {
      if (!_disposed) onStats(stats);
    });
  }

  void _onProject() {
    final scope = _scope;
    final stage = _stage;
    if (scope == null || scope.project.assetsRevision == _revision) return;
    _revision = scope.project.assetsRevision;
    if (stage == null || stage.isClosed) return;
    try {
      _stats(stage.refresh());
    } on AuthoringException {
      // Gone with a rescan: the page closes it.
    }
    _schedule();
  }

  void _onEdit(String kind, String id, Map<String, Object?> fields) {
    final stage = _stage;
    if (kind != 'material' || stage == null || stage.isClosed) return;
    try {
      stage.setMaterial(id, fields);
    } on AuthoringException {
      return;
    }
    _schedule();
  }

  /// Draws after this frame.
  void _schedule() {
    if (_scheduled || _disposed) return;
    _scheduled = true;
    SchedulerBinding.instance.addPostFrameCallback((_) {
      _scheduled = false;
      if (!_disposed) render();
    });
    SchedulerBinding.instance.ensureVisualUpdate();
  }

  /// Draws now. Public for tests.
  void render() {
    final controller = _controller, stage = _stage, cam = _camera;
    if (controller == null || stage == null || cam == null || !controller.isAttached) return;
    final address = stage.worldAddress;
    if (address == 0) return;
    final radius = stage.state.framing.radius;
    controller.view = ViewportView(
      world: cam.world,
      fovY: cam.fovY,
      near: math.max(cam.distance * 0.01, 0.001),
      far: cam.distance + math.max(radius, 0.5) * 8,
    );
    if (controller.renderHandle(address) != null) frames++;
  }

  // ── Steering ─────────────────────────────────────────────────────────

  Offset? _last;
  int _buttons = 0;

  void _down(PointerDownEvent e) {
    _focus.requestFocus();
    _last = e.localPosition;
    _buttons = e.buttons;
  }

  void _move(PointerMoveEvent e) {
    final cam = _camera, last = _last;
    if (cam == null || last == null) return;
    final d = e.localPosition - last;
    _last = e.localPosition;
    final shift = HardwareKeyboard.instance.isShiftPressed;
    if (_buttons & kSecondaryMouseButton != 0) {
      try {
        _stage?.turnLight(d.dx * 0.01, d.dy * 0.01);
      } on AuthoringException {
        return;
      }
    } else if (_buttons & kMiddleMouseButton != 0 || shift) {
      cam.pan(d.dx, d.dy, context.size?.height ?? 200);
    } else {
      cam.orbit(d.dx, d.dy);
    }
    _schedule();
  }

  void _up(PointerEvent e) {
    _last = null;
    _buttons = 0;
  }

  void _signal(PointerSignalEvent e) {
    if (e is! PointerScrollEvent) return;
    GestureBinding.instance.pointerSignalResolver.register(e, (_) {
      _camera?.dolly(-e.scrollDelta.dy / 40);
      _schedule();
    });
  }

  Offset _panZoom = Offset.zero;

  void _panZoomUpdate(PointerPanZoomUpdateEvent e) {
    final d = e.localPan - _panZoom;
    _panZoom = e.localPan;
    _camera?.dolly(d.dy / 40);
    _schedule();
  }

  /// Back to the framing it opened with.
  void frame() {
    final stage = _stage;
    if (stage == null) return;
    _camera = cameraOf(stage.state.framing);
    _schedule();
  }

  KeyEventResult _key(FocusNode node, KeyEvent e) {
    if (e is KeyDownEvent && e.logicalKey == LogicalKeyboardKey.keyF) {
      frame();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  void dispose() {
    _disposed = true;
    _ticker.dispose();
    _noticeTimer?.cancel();
    _detach();
    _focus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final error = _error;
    final controller = _controller;
    final small = Theme.of(context).textTheme.bodySmall;
    final stageArea = Focus(
      focusNode: _focus,
      onKeyEvent: _key,
      child: Listener(
        onPointerDown: _down,
        onPointerMove: _move,
        onPointerUp: _up,
        onPointerCancel: _up,
        onPointerSignal: _signal,
        onPointerPanZoomStart: (_) => _panZoom = Offset.zero,
        onPointerPanZoomUpdate: _panZoomUpdate,
        child: GestureDetector(
          key: const Key('preview.stage'),
          behavior: HitTestBehavior.opaque,
          onDoubleTap: frame,
          child: Stack(
            fit: StackFit.expand,
            children: [
              if (controller != null && error == null)
                FynkelViewport(controller: controller, placeholder: const SizedBox.shrink()),
              if (error != null)
                Center(
                  child: Padding(
                    padding: const EdgeInsets.all(12),
                    child: Text(
                      error,
                      key: const Key('preview.stage.error'),
                      textAlign: TextAlign.center,
                      style: small?.copyWith(color: t.textSecondary),
                    ),
                  ),
                ),
              if (_notice case final notice?)
                Positioned(
                  left: 6,
                  right: 6,
                  bottom: _clip == null ? 4 : 30,
                  child: Text(
                    notice,
                    key: const Key('preview.stage.notice'),
                    textAlign: TextAlign.center,
                    style: small?.copyWith(color: t.warning),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
    // The play bar stands outside the steering: its drags scrub, never orbit.
    return Stack(
      fit: StackFit.expand,
      children: [
        stageArea,
        if (_clip case final clip? when error == null)
          Positioned(
            left: 0,
            right: 0,
            bottom: 0,
            child: ClipBar(
              clip: clip,
              playing: _playing,
              speed: _speed,
              loop: _loop,
              onPlay: togglePlay,
              onScrub: (t) {
                _play(false);
                _seek(t);
              },
              onSpeed: (s) => setState(() => _speed = s),
              onLoop: () => setState(() => _loop = !_loop),
              onInPlace: () => _seek(clip.time, inPlace: !clip.inPlace),
              onParameter: (v) => _seek(clip.time, parameter: v),
            ),
          ),
      ],
    );
  }
}
