// `FynkelGameView`: the Ticker-driven host loop (ADR-0008) around a
// `FynkelViewport`. One Flutter frame = `World.frame` (fixed ticks with the
// §5.1 behaviour points, then update/lateUpdate) followed by one viewport
// render of the resulting extract. Optional per-second stats windows feed
// a HUD or a benchmark capture.

import 'dart:async';
import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:fynkel/gameplay.dart';

import 'application.dart';
import 'error_report.dart';
import 'heap_probe.dart';
import 'input_capture.dart';
import 'percentiles.dart';
import 'viewport.dart';
import 'viewport_controller.dart';
import 'viewport_overlay.dart';

export 'percentiles.dart' show MsPercentiles;

/// One reporting window (about a second) of the game view's loop.
class FynkelFrameStats {
  const FynkelFrameStats({
    required this.frames,
    required this.fps,
    this.windowSeconds = 0,
    required this.tickInterval,
    required this.simulate,
    required this.renderCall,
    this.behaviours = MsPercentiles.empty,
    this.nativeStep = MsPercentiles.empty,
    this.flutterBuild = MsPercentiles.empty,
    this.flutterRaster = MsPercentiles.empty,
    this.systems = const [],
    required this.fixedTicks,
    required this.missedDeadlines,
    required this.dropped,
    required this.skippedWhileResizing,
    required this.world,
    required this.viewport,
    required this.platform,
    required this.physicalWidth,
    required this.physicalHeight,
    required this.muted,
    this.dartHeap,
  });

  /// Game-loop callbacks this window and their rate over wall time.
  /// This is not the rate of distinct textures acquired by the compositor
  /// or proof of evenly spaced frames on the display.
  final int frames;
  final double fps;

  /// Wall time covered by this report. Zero for legacy/manual snapshots.
  final double windowSeconds;

  /// New textures acquired by the compositor per second, not display scanouts.
  double? get textureAcquisitionFps =>
      windowSeconds > 0 && viewport != null ? viewport!.framesPresented / windowSeconds : null;

  /// Time between consecutive Ticker callbacks.
  final MsPercentiles tickInterval;

  /// `World.frame` wall time (gameplay + submit + step + snapshot).
  final MsPercentiles simulate;

  /// `render()` wall time (extract + encode + submit).
  final MsPercentiles renderCall;

  /// The split of [simulate] (M3-16, `World.frameTimes`): the Dart
  /// behaviours (every `fixedUpdate`/`afterPhysics`/`update`/`lateUpdate`
  /// and the host's `fixedUpdate` callback) and the native engine (the
  /// ticks with their command flush, snapshot and event drains).
  final MsPercentiles behaviours, nativeStep;

  /// Flutter's own frame timings this window (`SchedulerBinding
  /// .addTimingsCallback`): the UI thread's build and the raster thread's
  /// work per Flutter frame — the compositor cost around the game loop.
  final MsPercentiles flutterBuild, flutterRaster;

  /// The engine's per-system times of the window's last tick
  /// (`World.systemTimes`; one query per window).
  final List<SystemTime> systems;
  final int fixedTicks, missedDeadlines;

  /// Frames the ring could not take (full) this window.
  final int dropped;
  final int skippedWhileResizing;
  final BridgeStats world;
  final ViewportStatsSnapshot? viewport;
  final ViewportPlatformStats? platform;
  final int physicalWidth, physicalHeight;
  final bool muted;

  /// The Dart heap this window (blueprint C3): present when the view was
  /// given `profileDartHeap: true` and the app carries a VM service (debug
  /// and profile builds; a release build has none, so this stays null).
  final DartHeapStats? dartHeap;

  /// The frame's total CPU work on the game path — [simulate] plus
  /// [renderCall] at the same percentile (an upper bound on the sum's
  /// percentile) — the number the blueprint's 12 ms p95 target is about.
  MsPercentiles get totalWork => MsPercentiles(
    simulate.p50 + renderCall.p50,
    simulate.p95 + renderCall.p95,
    simulate.p99 + renderCall.p99,
    frames,
  );

  /// The window on one line of `key=value` pairs for a capture to parse
  /// (`benchmarks/m3/capture.py` reads `[stats] …` lines from an app's
  /// stdout): counts, every timer as `p50/p95/p99` ms, the memory in
  /// bytes, the Dart heap when [dartHeap] is present. Keys are stable.
  String summary() {
    String pct(MsPercentiles p) =>
        '${p.p50.toStringAsFixed(3)}/${p.p95.toStringAsFixed(3)}/${p.p99.toStringAsFixed(3)}';
    final v = viewport;
    final h = dartHeap;
    return [
      'frames=$frames',
      'ticks=$fixedTicks',
      'missed=$missedDeadlines',
      'dropped=$dropped',
      'entities=${world.liveEntities}',
      'interval=${pct(tickInterval)}',
      'simulate=${pct(simulate)}',
      'render=${pct(renderCall)}',
      'behaviours=${pct(behaviours)}',
      'native=${pct(nativeStep)}',
      'work=${pct(totalWork)}',
      'build=${pct(flutterBuild)}',
      'raster=${pct(flutterRaster)}',
      'snapshot=${world.snapshotBytes}',
      'snapshot_peak=${world.snapshotPeakBytes}',
      'mem_native=${world.nativeBytes}',
      'mem_resident=${world.residentBytes}',
      'mem_assets=${world.residencyBytes}',
      if (v != null) ...[
        'instances=${v.instances}',
        'draws=${v.draws}',
        'culled=${v.culled}',
        'shadow=${v.shadowCasters}/${v.shadowDraws}',
        'rendered=${v.framesRendered}',
        'acquired=${v.framesPresented}',
        'repeated=${v.presentsWithoutNewFrame}',
        'present_lag=${v.presentLagP50}/${v.presentLagP95}/${v.presentLagMax}',
        'acquire_wait=${v.acquireWaitP50Ms.toStringAsFixed(3)}/${v.acquireWaitP95Ms.toStringAsFixed(3)}/${v.acquireWaitMaxMs.toStringAsFixed(3)}',
        'extract=${pct(v.extract)}',
        'prepare=${pct(v.prepare)}',
        'encode=${pct(v.encode)}',
        'cpu=${v.cpuP50Ms.toStringAsFixed(3)}/${v.cpuP95Ms.toStringAsFixed(3)}/${v.cpuP99Ms.toStringAsFixed(3)}',
        'gpu_complete=${v.gpuP50Ms.toStringAsFixed(3)}/${v.gpuP95Ms.toStringAsFixed(3)}/${v.gpuP99Ms.toStringAsFixed(3)}',
        'gpu_frame=${pct(v.gpuFrame)}',
        'gpu_shadow=${pct(v.gpuShadow)}',
        'gpu_ao=${pct(v.gpuAo)}',
        'gpu_scene=${pct(v.gpuScene)}',
        'gpu_post=${pct(v.gpuPost)}',
        'gpu_n=${v.gpuFrame.count}',
        'gpu_untimed=${v.gpuFramesUntimed}',
        'gpu_timestamps=${v.gpuTimestamps}',
        'mem_gpu=${v.gpuBytes}',
      ],
      if (platform case final p?) ...[
        'texture_copies=${p.copyCalls}',
        'texture_new=${p.copyWithNewFrame}',
        'texture_empty=${p.copyNoFrame}',
        'texture_ready=${p.frameReadySignals}',
        'texture_more=${p.copyQueued}',
        'thermal=${p.thermalState.name}',
      ],
      if (h != null) ...[
        'heap=${h.heapUsage}',
        'heap_capacity=${h.heapCapacity}',
        'heap_external=${h.externalUsage}',
        'heap_alloc=${h.allocatedBytes}',
        'gc=${h.gcCollections}',
        'gc_new=${h.newSpaceCollections}',
        'gc_old=${h.oldSpaceCollections}',
        'gc_pause=${h.gcPauseMs.toStringAsFixed(3)}',
      ],
    ].join(' ');
  }

  /// This window as the editor channel's `stats` record (blueprint
  /// §19.2e item 8): the numbers a Play session shows live, plus
  /// [summary] for the row the Console prints.
  Map<String, Object?> channelRecord() => {
    'fps': double.parse(fps.toStringAsFixed(2)),
    'frames': frames,
    'ticks': fixedTicks,
    'missed': missedDeadlines,
    'dropped': dropped,
    'entities': world.liveEntities,
    'workP50': double.parse(totalWork.p50.toStringAsFixed(3)),
    'workP95': double.parse(totalWork.p95.toStringAsFixed(3)),
    'workP99': double.parse(totalWork.p99.toStringAsFixed(3)),
    'draws': viewport?.draws,
    'memNative': world.nativeBytes,
    'memResident': world.residentBytes,
    'summary': summary(),
  };
}

/// Opt-in host frame trace. Times are epoch microseconds for correlation with
/// native diagnostics; wall-clock changes invalidate cross-process alignment.
/// A completed host frame is not proof of physical display presentation.
class FynkelFrameTrace {
  const FynkelFrameTrace({required this.startUs, required this.simulationEndUs,
    required this.renderStartUs, required this.endUs, required this.steps,
    required this.tick, required this.slot});
  final int startUs, simulationEndUs, renderStartUs, endUs, steps, tick;
  final int? slot;
}

class FynkelGameView extends StatefulWidget {
  const FynkelGameView({
    super.key,
    required this.world,
    this.controller,
    this.input,
    this.fixedUpdate,
    this.onFrame,
    this.onFrameTrace,
    this.onStats,
    this.statsInterval = const Duration(seconds: 1),
    this.overlay,
    this.pauseWhenHidden = true,
    this.audio = false,
    this.autofocus = true,
    this.focusNode,
    this.onKeyEvent,
    this.onPointerEvent,
    this.placeholder,
    this.resolutionScale = 1.0,
    this.profileDartHeap = false,
    this.gizmos,
  });

  /// The world this view simulates and draws. The view does not own it.
  final World world;

  /// Viewport settings; the view creates (and disposes) a default one when
  /// null. Reads `assetCache`/`placeholders`/`camera` from it as given.
  final FynkelViewportController? controller;

  /// Where the frame's raw control events collect (keys and mouse from the
  /// viewport, gamepad and captured-mouse deltas from the platform, a
  /// [TouchOverlay]'s controls); `World.frame` takes them every tick. The
  /// view creates one when null; pass your own to share it with an overlay
  /// or to record (`FynkelInputCapture(recorder: ...)`).
  final FynkelInputCapture? input;

  /// Host-side code for every fixed tick, after the behaviours' `fixedUpdate`
  /// (see `World.frame`).
  final void Function(int tick)? fixedUpdate;

  /// Called once per Flutter frame after `World.frame`, before the render.
  final void Function(double dtSeconds, FramePlan plan)? onFrame;

  /// Optional diagnostic callback after simulation and render return. Timestamp
  /// reads and trace objects are created only while a callback is installed.
  final void Function(FynkelFrameTrace trace)? onFrameTrace;

  /// Per-window loop statistics (HUD, benchmark captures).
  final void Function(FynkelFrameStats stats)? onStats;
  final Duration statsInterval;

  /// Sample the Dart heap (usage, allocation, GC) for [onStats] through
  /// the app's VM service (blueprint C3; `DartHeapProbe`). Debug and
  /// profile builds only — a release build has no service and the stats
  /// stay null. The sample is a few JSON-RPC round trips per window,
  /// never on the frame.
  final bool profileDartHeap;

  /// Draw the world's [Gizmos] over the picture (ADR-0019 §3): true or
  /// false; null follows the environment — on when `FYNKEL_GIZMOS=1`, what
  /// the editor's Play passes when its Gizmos toggle is on. Debug and
  /// profile builds only: in release [Gizmos] compiles to nothing.
  final bool? gizmos;

  /// Widgets drawn over the viewport (the HUD).
  final Widget? overlay;

  /// Mute the ticker while the app is hidden/paused (the clock restarts on
  /// resume so no catch-up burst of fixed ticks follows). Independently,
  /// a [FynkelPlayScope] above the view (the editor's Play, ADR-0021)
  /// pauses it — loop and audio — and steps it one fixed step at a time.
  final bool pauseWhenHidden;

  /// Open the world's output device (`world.audio.open()`, M3-11) when
  /// the view starts, suspend it while the app is hidden/paused and close
  /// it when the view goes; false leaves the world on its null backend
  /// (tests, a view that only draws). A platform without a device is an
  /// `audio_device` diagnostic, not an error.
  final bool audio;

  final bool autofocus;
  final FocusNode? focusNode;

  /// Called first for every key/pointer event; a key the callback marks
  /// handled does not reach the input capture.
  final FocusOnKeyEventCallback? onKeyEvent;
  final ViewportPointerCallback? onPointerEvent;
  final Widget? placeholder;

  /// Fraction of native resolution to render at (see [FynkelViewport.resolutionScale]).
  final double resolutionScale;

  @override
  State<FynkelGameView> createState() => FynkelGameViewState();
}

class FynkelGameViewState extends State<FynkelGameView> with SingleTickerProviderStateMixin, WidgetsBindingObserver {
  FynkelViewportController? _owned;
  // A game's own viewport draws on the render worker and shows every frame
  // it makes at the display's rate (docs/foundation-f8-throughput-observations.md):
  // five buffers hold the two queued frames, the one shown and the one retiring.
  FynkelViewportController get controller =>
      widget.controller ?? (_owned ??= FynkelViewportController(pipelined: true, frameQueue: 2, bufferCount: 5));
  FynkelInputCapture? _ownedInput;
  FynkelInputCapture get input => widget.input ?? (_ownedInput ??= FynkelInputCapture());
  bool _hasFocus = false;
  bool _pointerCaptured = false;

  /// Whether the viewport has keyboard focus (keys reach the game).
  bool get hasFocus => _hasFocus;

  /// Whether the mouse is captured ([capturePointer]).
  bool get pointerCaptured => _pointerCaptured;

  /// Hides the cursor and routes its movement to `mouse:delta` (macOS;
  /// elsewhere the request is refused and false comes back). Released by
  /// a focus loss, a pause and [dispose].
  Future<bool> capturePointer(bool capture) async {
    if (capture == _pointerCaptured) return _pointerCaptured;
    final ok = await FynkelInputChannel.capturePointer(capture);
    _pointerCaptured = capture && ok;
    input.pointerCaptured = _pointerCaptured;
    return _pointerCaptured;
  }

  late final Ticker _ticker;
  Timer? _statsTimer;
  Duration? _lastTick;
  bool _primed = false;

  final _intervalsMs = <double>[];
  final _simMs = <double>[];
  final _behavioursMs = <double>[];
  final _nativeMs = <double>[];
  final _renderCallMs = <double>[];
  final _buildMs = <double>[];
  final _rasterMs = <double>[];
  int _ticks = 0, _missed = 0, _dropped = 0, _skipped = 0, _frames = 0;
  final _window = Stopwatch()..start();
  // Reused per tick (a frame allocates no timers).
  final _simWatch = Stopwatch(), _renderWatch = Stopwatch();

  /// Whether the loop is currently muted (hidden, [setPaused], or the
  /// editor's Pause).
  bool get paused => _ticker.muted;
  bool _pausedByUser = false;
  bool _hidden = false;

  /// The editor's Pause ([FynkelPlayScope]): the loop and the audio stop.
  bool get hostPaused => _hostPaused;
  bool _hostPaused = false;
  FynkelPlayControl? _play;

  // ------------------------------------------------- screen ↔ world (ADR-0062)

  /// The view's size in logical pixels; null before its first layout.
  Size? get viewSize {
    final box = context.findRenderObject();
    return box is RenderBox && box.hasSize ? box.size : null;
  }

  Size _sizeFor(String what) =>
      viewSize ?? (throw StateError('$what: the game view has not been laid out yet'));

  /// The ray through [position] — in this view's logical pixels from its
  /// top left, as a gesture's `localPosition` over the view is — from the
  /// frame's camera: Unity's `Camera.ScreenPointToRay`. Null without a
  /// camera.
  ScreenRay? screenRay(Offset position) {
    final size = _sizeFor('screenRay');
    return widget.world.projection.screenRay(position.dx, position.dy, width: size.width, height: size.height);
  }

  /// The nearest collider under [position] (a tap on an object, the
  /// cursor over the ground): [screenRay] into `World.raycast`, with its
  /// options. Null without a camera or a hit.
  PhysicsHit? pick(Offset position, {double maxDistance = 1000, int mask = 0xffffffff, bool hitSensors = true}) {
    final size = _sizeFor('pick');
    return widget.world.projection.pick(
      position.dx,
      position.dy,
      width: size.width,
      height: size.height,
      maxDistance: maxDistance,
      mask: mask,
      hitSensors: hitSensors,
    );
  }

  /// Where the world [point] is drawn in this view, now (`.offset` is the
  /// position; check `onScreen`). For a widget that follows an object every
  /// frame use a `ProjectedAnchor` instead. Null without a camera.
  ScreenPoint? worldToScreen(Vec3 point, {int occlusionMask = 0}) {
    final size = _sizeFor('worldToScreen');
    return widget.world.projection.worldToScreen(
      point,
      width: size.width,
      height: size.height,
      occlusionMask: occlusionMask,
    );
  }

  /// Pause/resume the loop; the clock is re-primed on resume.
  void setPaused(bool v) {
    _pausedByUser = v;
    _applyMute();
  }

  bool _openedAudio = false;

  void _applyMute() {
    if (widget.audio) widget.world.audio.suspend(_hidden || _hostPaused);
    final mute = _pausedByUser || _hostPaused || (widget.pauseWhenHidden && _hidden);
    if (_ticker.muted == mute) return;
    _ticker.muted = mute;
    if (mute) {
      // Nothing is held across a pause: the next frame captures a reset.
      input.reset();
      unawaited(capturePointer(false));
    } else {
      _primed = false;
    }
  }

  /// Around the overlay: never focused itself, it sees the keys a focused
  /// HUD widget lets through (foundation 4.7).
  final FocusNode _overlayFocus = FocusNode(
    debugLabel: 'FynkelGameView overlay',
    canRequestFocus: false,
    skipTraversal: true,
  );

  bool _viewportFocused = false;

  void _onFocusChange(bool focused) {
    _viewportFocused = focused;
    _focusMoved();
  }

  /// Keys reach the game while the viewport or a widget of the overlay has
  /// focus — a HUD button does not take them, as in Unity — but not while
  /// a text field there does. Held keys are let go when they stop
  /// reaching it (their key-ups would go elsewhere).
  void _focusMoved() {
    final reach = _viewportFocused || (_overlayFocus.hasFocus && !_typing());
    if (reach == _hasFocus) return;
    _hasFocus = reach;
    if (!reach) {
      input.reset();
      unawaited(capturePointer(false));
    }
  }

  /// Whether the focused widget is a text field (it keeps the keys).
  bool _typing() => FocusManager.instance.primaryFocus?.context?.findAncestorWidgetOfExactType<EditableText>() != null;

  KeyEventResult _onOverlayKey(FocusNode node, KeyEvent event) =>
      _typing() ? KeyEventResult.ignored : _onKey(node, event);

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    final user = widget.onKeyEvent?.call(node, event);
    if (user == KeyEventResult.handled) return user!;
    return input.handleKey(event) ? KeyEventResult.handled : (user ?? KeyEventResult.ignored);
  }

  void _onPointer(PointerEvent event, Offset physical) {
    widget.onPointerEvent?.call(event, physical);
    input.handlePointer(event, MediaQuery.devicePixelRatioOf(context));
  }

  ViewportState? _seenState;

  /// Window creation and the first texture registration stall the frame
  /// pipeline for 100+ ms; re-prime the clock when the viewport attaches so
  /// that stall is not fed to the world as one big dt (catch-up burst).
  void _onViewportChanged() {
    final state = controller.state;
    if (state == ViewportState.attached && _seenState != ViewportState.resizing) {
      _primed = false;
    }
    _seenState = state;
  }

  @override
  void initState() {
    super.initState();
    ChannelErrorReporting.install();
    WidgetsBinding.instance.addObserver(this);
    controller.addListener(_onViewportChanged);
    // Any move of the focus: into the overlay, or within it to a text field.
    FocusManager.instance.addListener(_focusMoved);
    FynkelInputChannel.attach(input);
    if (widget.audio && !widget.world.audio.isOpen) _openedAudio = widget.world.audio.open();
    _applyGizmos();
    _ticker = createTicker(_onTick)..start();
    if (_wantsStats) {
      _statsTimer = Timer.periodic(widget.statsInterval, (_) => _report());
      SchedulerBinding.instance.addTimingsCallback(_onTimings);
      _timingsInstalled = true;
      if (widget.profileDartHeap) unawaited(_connectHeapProbe());
    }
  }

  /// Whether the window is worth measuring: the host asked for it, or the
  /// editor channel is listening (ADR-0016 — Play's stats records).
  bool get _wantsStats => widget.onStats != null || EditorChannel.enabled;

  bool _timingsInstalled = false;
  DartHeapProbe? _heapProbe;
  bool _heapProbeRequested = false;

  Future<void> _connectHeapProbe() async {
    if (_heapProbeRequested) return;
    _heapProbeRequested = true;
    final probe = await DartHeapProbe.connect();
    if (!mounted) {
      await probe?.close();
      return;
    }
    _heapProbe = probe;
  }

  void _onTimings(List<FrameTiming> timings) {
    for (final t in timings) {
      _buildMs.add(t.buildDuration.inMicroseconds / 1000);
      _rasterMs.add(t.rasterDuration.inMicroseconds / 1000);
    }
  }

  void _installTimings(bool wanted) {
    if (wanted == _timingsInstalled) return;
    if (wanted) {
      SchedulerBinding.instance.addTimingsCallback(_onTimings);
    } else {
      SchedulerBinding.instance.removeTimingsCallback(_onTimings);
    }
    _timingsInstalled = wanted;
  }

  /// Whether this view draws gizmos ([FynkelGameView.gizmos]).
  bool get gizmosOn => Gizmos.compiled && (widget.gizmos ?? Platform.environment['FYNKEL_GIZMOS'] == '1');

  void _applyGizmos() => widget.world.showGizmos = gizmosOn;

  final _gizmoOverlay = ViewportOverlay();

  /// The gizmo lines the last frame handed the viewport (tests).
  @visibleForTesting
  int? gizmoLinesQueued;
  double _pixelRatio = 1;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _pixelRatio = (MediaQuery.maybeDevicePixelRatioOf(context) ?? 1) * widget.resolutionScale;
    // The editor's Pause and Step: this runs again whenever they change.
    final play = FynkelPlayScope.maybeOf(context);
    if (!identical(play, _play)) {
      // The editor's live Play mode finds the world through the scope.
      _play?.detach(widget.world);
      play?.attach(widget.world);
      _play = play;
    }
    final hostPaused = play?.paused ?? false;
    if (hostPaused != _hostPaused) {
      _hostPaused = hostPaused;
      _applyMute();
    }
    if (play != null && _hostPaused && play.takeStep()) _scheduleStep();
    if (play != null && play.redraws != _redraws) {
      _redraws = play.redraws;
      if (_hostPaused) _scheduleRedraw();
    }
  }

  /// The [FynkelPlayControl.redraws] this view drew last.
  int _redraws = 0;

  /// Pictures drawn for [FynkelPlayControl.redraw] (without a step).
  @visibleForTesting
  int redrawn = 0;

  /// The world drawn again on the next Flutter frame, not stepped (a paused
  /// world the editor changed).
  void _scheduleRedraw() {
    SchedulerBinding.instance.scheduleFrameCallback((_) {
      if (!mounted || !_hostPaused) return;
      redrawn++;
      _render(widget.world);
    });
    SchedulerBinding.instance.scheduleFrame();
  }

  /// One frame of the world's fixed step on the next Flutter frame (Step
  /// while paused); further steps queued meanwhile follow one per frame.
  void _scheduleStep() {
    SchedulerBinding.instance.scheduleFrameCallback((_) {
      if (!mounted || !_hostPaused) return;
      _frame(0, singleStep: true);
      if (_play?.takeStep() ?? false) _scheduleStep();
    });
    SchedulerBinding.instance.scheduleFrame();
  }

  @override
  void didUpdateWidget(FynkelGameView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.gizmos != widget.gizmos || oldWidget.world != widget.world) _applyGizmos();
    if (oldWidget.world != widget.world) {
      _play?.detach(oldWidget.world);
      _play?.attach(widget.world);
    }
    if (oldWidget.controller != widget.controller) {
      (oldWidget.controller ?? _owned)?.removeListener(_onViewportChanged);
      if (widget.controller != null) {
        _owned?.dispose();
        _owned = null;
      }
      controller.addListener(_onViewportChanged);
    }
    if (oldWidget.input != widget.input) {
      FynkelInputChannel.detach(oldWidget.input ?? _ownedInput ?? input);
      if (widget.input != null) _ownedInput = null;
      FynkelInputChannel.attach(input);
    }
    if ((oldWidget.onStats == null) != (widget.onStats == null) || oldWidget.statsInterval != widget.statsInterval) {
      _statsTimer?.cancel();
      _statsTimer = _wantsStats ? Timer.periodic(widget.statsInterval, (_) => _report()) : null;
      _installTimings(_wantsStats);
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _hidden = switch (state) {
      AppLifecycleState.resumed || AppLifecycleState.inactive => false,
      AppLifecycleState.hidden || AppLifecycleState.paused || AppLifecycleState.detached => true,
    };
    _applyMute();
  }

  @override
  void dispose() {
    FocusManager.instance.removeListener(_focusMoved);
    _overlayFocus.dispose();
    WidgetsBinding.instance.removeObserver(this);
    _play?.detach(widget.world);
    if (_openedAudio) widget.world.audio.close();
    _ticker.dispose();
    _statsTimer?.cancel();
    _installTimings(false);
    unawaited(_heapProbe?.close());
    FynkelInputChannel.detach(input);
    if (_pointerCaptured) unawaited(FynkelInputChannel.capturePointer(false));
    controller.removeListener(_onViewportChanged);
    // Detach first so no render lands on a controller the owner disposes next.
    unawaited(controller.detach());
    _owned?.dispose();
    super.dispose();
  }

  void _onTick(Duration elapsed) {
    var dt = 0.0;
    if (_primed && _lastTick != null) {
      dt = (elapsed - _lastTick!).inMicroseconds / 1e6;
      _intervalsMs.add(dt * 1000);
    }
    _lastTick = elapsed;
    _primed = true;
    _frame(dt);
  }

  /// One frame of [dt] seconds: the world's frame, then the render.
  void _frame(double dt, {bool singleStep = false}) {
    if (_panic != null) return; // stopped for good (a Step, a resume)
    final trace = widget.onFrameTrace;
    final startUs = trace == null ? 0 : DateTime.now().microsecondsSinceEpoch;
    final world = widget.world;
    _frames++;

    final sim = _simWatch
      ..reset()
      ..start();
    final FramePlan plan;
    try {
      plan = singleStep
          ? world.stepFrame(input: input.take(0), fixedUpdate: widget.fixedUpdate)
          : world.frame(dt, input: input.take(dt), fixedUpdate: widget.fixedUpdate);
    } on FynkelPanicError catch (e, stack) {
      sim.stop();
      _stopped(e, stack);
      return;
    }
    sim.stop();
    final simulationEndUs = trace == null ? 0 : DateTime.now().microsecondsSinceEpoch;
    _simMs.add(sim.elapsedMicroseconds / 1000);
    final ft = world.frameTimes;
    _behavioursMs.add(ft.behavioursUs / 1000);
    _nativeMs.add(ft.nativeUs / 1000);
    _ticks += plan.steps;
    if (plan.missedDeadline) {
      _missed++;
    }
    // World labels: one projection call for the frame's anchors, after the
    // frame so they see the poses the render interpolates (docs/hud.md).
    if (!world.projection.isEmpty) {
      world.projection.update();
    }
    widget.onFrame?.call(dt, plan);
    final renderStartUs = trace == null ? 0 : DateTime.now().microsecondsSinceEpoch;
    final slot = _render(world);
    if (trace != null) {
      trace(FynkelFrameTrace(startUs: startUs, simulationEndUs: simulationEndUs,
        renderStartUs: renderStartUs, endUs: DateTime.now().microsecondsSinceEpoch,
        steps: plan.steps, tick: world.tick, slot: slot));
    }
  }

  /// The engine panicked (foundation 4.1): the world is poisoned for good,
  /// so the frames stop, the error is reported once and the view says
  /// why over the last picture.
  void _stopped(FynkelPanicError error, StackTrace stack) {
    _ticker.stop();
    FlutterError.reportError(
      FlutterErrorDetails(
        exception: error,
        stack: stack,
        library: 'fynkel_flutter',
        context: ErrorDescription('while running the game; the engine stopped the world'),
      ),
    );
    if (mounted) setState(() => _panic = error);
  }

  FynkelPanicError? _panic;

  /// The world's picture as it is now, with its gizmo lines.
  int? _render(World world) {
    final c = controller;
    if (c.state == ViewportState.resizing) {
      _skipped++;
      return null;
    }
    if (c.state != ViewportState.attached) return null;
    final gizmos = world.gizmos;
    if (gizmos != null && !gizmos.isEmpty) {
      // One point wide, faded behind geometry: the editor's gizmo lines.
      gizmoLinesQueued = c.overlay(
        _gizmoOverlay
          ..clear()
          ..segments(gizmos.lines, width: _pixelRatio),
      );
    }
    final sw = _renderWatch
      ..reset()
      ..start();
    final slot = c.render(world);
    sw.stop();
    _renderCallMs.add(sw.elapsedMicroseconds / 1000);
    if (slot == -1) {
      _dropped++;
    }
    if (slot == renderNoWorld) {
      debugPrint('[fynkel_flutter] render: no world (poisoned?)');
    }
    if (slot == renderBroken) {
      // Once: the controller is failed now and renders no more; the
      // viewport widget shows the message.
      FlutterError.reportError(
        FlutterErrorDetails(
          exception: c.error!,
          library: 'fynkel_flutter',
          context: ErrorDescription('while drawing the game; the viewport stopped'),
        ),
      );
    }
    return slot;
  }

  Future<void> _report() async {
    final c = controller;
    final viewport = c.takeStats();
    final platform = await c.platformStats();
    final dartHeap = await _heapProbe?.sample();
    if (!mounted) return;
    final seconds = _window.elapsedMicroseconds / 1e6;
    final stats = FynkelFrameStats(
      frames: _frames,
      fps: seconds > 0 ? _frames / seconds : 0,
      windowSeconds: seconds,
      tickInterval: MsPercentiles.of(_intervalsMs),
      simulate: MsPercentiles.of(_simMs),
      renderCall: MsPercentiles.of(_renderCallMs),
      behaviours: MsPercentiles.of(_behavioursMs),
      nativeStep: MsPercentiles.of(_nativeMs),
      flutterBuild: MsPercentiles.of(_buildMs),
      flutterRaster: MsPercentiles.of(_rasterMs),
      systems: _ticks > 0 ? widget.world.systemTimes() : const [],
      fixedTicks: _ticks,
      missedDeadlines: _missed,
      dropped: _dropped,
      skippedWhileResizing: _skipped,
      world: widget.world.stats(),
      viewport: viewport,
      platform: platform,
      physicalWidth: c.physicalWidth,
      physicalHeight: c.physicalHeight,
      muted: _ticker.muted,
      dartHeap: dartHeap,
    );
    _intervalsMs.clear();
    _simMs.clear();
    _behavioursMs.clear();
    _nativeMs.clear();
    _renderCallMs.clear();
    _buildMs.clear();
    _rasterMs.clear();
    _ticks = _missed = _dropped = _skipped = _frames = 0;
    _window.reset();
    if (EditorChannel.enabled) EditorChannel.stats(stats.channelRecord(), tick: widget.world.tick);
    widget.onStats?.call(stats);
  }

  @override
  Widget build(BuildContext context) {
    final viewport = FynkelViewport(
      controller: controller,
      focusNode: widget.focusNode,
      autofocus: widget.autofocus,
      onKeyEvent: _onKey,
      onPointerEvent: _onPointer,
      onFocusChange: _onFocusChange,
      placeholder: widget.placeholder,
      resolutionScale: widget.resolutionScale,
    );
    final hud = widget.overlay;
    final overlay = hud == null ? null : Focus(focusNode: _overlayFocus, onKeyEvent: _onOverlayKey, child: hud);
    final panic = _panic;
    if (panic != null) {
      return Stack(fit: StackFit.expand, children: [viewport, _Stopped(panic)]);
    }
    if (overlay == null) return viewport;
    return Stack(fit: StackFit.expand, children: [viewport, overlay]);
  }
}

/// Over a game the engine stopped (foundation 4.1): what happened, in
/// place of the game's own overlay.
class _Stopped extends StatelessWidget {
  const _Stopped(this.error);

  final FynkelPanicError error;

  @override
  Widget build(BuildContext context) => ColoredBox(
    color: const Color(0xCC000000),
    child: Center(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Text(
          'The game stopped: the engine panicked.\n${error.panic}',
          textAlign: TextAlign.center,
          textDirection: TextDirection.ltr,
          style: const TextStyle(color: Color(0xFFFFFFFF), fontSize: 14),
        ),
      ),
    ),
  );
}

/// A [ScreenPoint] as a Flutter position.
extension ScreenPointOffset on ScreenPoint {
  /// [x], [y] as an [Offset] in the viewport's pixels.
  Offset get offset => Offset(x, y);
}
