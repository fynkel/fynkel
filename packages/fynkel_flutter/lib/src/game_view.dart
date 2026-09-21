// `FynkelGameView`: the Ticker-driven host loop (ADR-0008) around a
// `FynkelViewport`. One Flutter frame = `World.frame` (fixed ticks with the
// §5.1 behaviour points, then update/lateUpdate) followed by one viewport
// render of the resulting extract. Optional per-second stats windows feed
// a HUD or a benchmark capture.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:fynkel/fynkel.dart';

import 'input_capture.dart';
import 'percentiles.dart';
import 'viewport.dart';
import 'viewport_controller.dart';

export 'percentiles.dart' show MsPercentiles;

/// One reporting window (about a second) of the game view's loop.
class FynkelFrameStats {
  const FynkelFrameStats({
    required this.frames,
    required this.fps,
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
  });

  /// Ticker callbacks (frames simulated and rendered) this window, and
  /// that count over the window's wall time — the rate the game runs at,
  /// which is the display's refresh rate when nothing is missed.
  final int frames;
  final double fps;

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

  /// The frame's total CPU work on the game path — [simulate] plus
  /// [renderCall] at the same percentile (an upper bound on the sum's
  /// percentile) — the number the blueprint's 12 ms p95 target is about.
  MsPercentiles get totalWork => MsPercentiles(
        simulate.p50 + renderCall.p50,
        simulate.p95 + renderCall.p95,
        simulate.p99 + renderCall.p99,
        frames,
      );
}

class FynkelGameView extends StatefulWidget {
  const FynkelGameView({
    super.key,
    required this.world,
    this.controller,
    this.input,
    this.fixedUpdate,
    this.onFrame,
    this.onStats,
    this.statsInterval = const Duration(seconds: 1),
    this.overlay,
    this.pauseWhenHidden = true,
    this.audio = false,
    this.maxFrameDt = 0.25,
    this.autofocus = true,
    this.focusNode,
    this.onKeyEvent,
    this.onPointerEvent,
    this.placeholder,
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

  /// Per-window loop statistics (HUD, benchmark captures).
  final void Function(FynkelFrameStats stats)? onStats;
  final Duration statsInterval;

  /// Widgets drawn over the viewport (the HUD).
  final Widget? overlay;

  /// Mute the ticker while the app is hidden/paused (the clock restarts on
  /// resume so no catch-up burst of fixed ticks follows).
  final bool pauseWhenHidden;

  /// Open the world's output device (`world.audio.open()`, M3-11) when
  /// the view starts, suspend it while the app is hidden/paused and close
  /// it when the view goes; false leaves the world on its null backend
  /// (tests, a view that only draws). A platform without a device is an
  /// `audio_device` diagnostic, not an error.
  final bool audio;

  /// Clamp for one frame's dt (a debugger pause must not become a burst).
  final double maxFrameDt;
  final bool autofocus;
  final FocusNode? focusNode;

  /// Called first for every key/pointer event; a key the callback marks
  /// handled does not reach the input capture.
  final FocusOnKeyEventCallback? onKeyEvent;
  final ViewportPointerCallback? onPointerEvent;
  final Widget? placeholder;

  @override
  State<FynkelGameView> createState() => FynkelGameViewState();
}

class FynkelGameViewState extends State<FynkelGameView> with SingleTickerProviderStateMixin, WidgetsBindingObserver {
  FynkelViewportController? _owned;
  FynkelViewportController get controller => widget.controller ?? (_owned ??= FynkelViewportController());
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

  /// Whether the loop is currently muted (hidden, or [setPaused]).
  bool get paused => _ticker.muted;
  bool _pausedByUser = false;
  bool _hidden = false;

  /// Pause/resume the loop; the clock is re-primed on resume.
  void setPaused(bool v) {
    _pausedByUser = v;
    _applyMute();
  }

  bool _openedAudio = false;

  void _applyMute() {
    if (widget.audio) widget.world.audio.suspend(_hidden);
    final mute = _pausedByUser || (widget.pauseWhenHidden && _hidden);
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

  void _onFocusChange(bool focused) {
    _hasFocus = focused;
    if (!focused) {
      input.reset();
      unawaited(capturePointer(false));
    }
  }

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
    WidgetsBinding.instance.addObserver(this);
    controller.addListener(_onViewportChanged);
    FynkelInputChannel.attach(input);
    if (widget.audio && !widget.world.audio.isOpen) _openedAudio = widget.world.audio.open();
    _ticker = createTicker(_onTick)..start();
    if (widget.onStats != null) {
      _statsTimer = Timer.periodic(widget.statsInterval, (_) => _report());
      SchedulerBinding.instance.addTimingsCallback(_onTimings);
      _timingsInstalled = true;
    }
  }

  bool _timingsInstalled = false;

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

  @override
  void didUpdateWidget(FynkelGameView oldWidget) {
    super.didUpdateWidget(oldWidget);
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
      _statsTimer = widget.onStats == null ? null : Timer.periodic(widget.statsInterval, (_) => _report());
      _installTimings(widget.onStats != null);
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
    WidgetsBinding.instance.removeObserver(this);
    if (_openedAudio) widget.world.audio.close();
    _ticker.dispose();
    _statsTimer?.cancel();
    _installTimings(false);
    FynkelInputChannel.detach(input);
    if (_pointerCaptured) unawaited(FynkelInputChannel.capturePointer(false));
    controller.removeListener(_onViewportChanged);
    // Detach first so no render lands on a controller the owner disposes next.
    unawaited(controller.detach());
    _owned?.dispose();
    super.dispose();
  }

  void _onTick(Duration elapsed) {
    final world = widget.world;
    var dt = 0.0;
    if (_primed && _lastTick != null) {
      dt = (elapsed - _lastTick!).inMicroseconds / 1e6;
      if (dt > widget.maxFrameDt) {
        dt = widget.maxFrameDt;
      }
      _intervalsMs.add(dt * 1000);
    }
    _lastTick = elapsed;
    _primed = true;
    _frames++;

    final sim = Stopwatch()..start();
    final plan = world.frame(dt, input: input.take(dt), fixedUpdate: widget.fixedUpdate);
    sim.stop();
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

    final c = controller;
    if (c.state == ViewportState.resizing) {
      _skipped++;
      return;
    }
    if (c.state != ViewportState.attached) return;
    final sw = Stopwatch()..start();
    final slot = c.render(world);
    sw.stop();
    _renderCallMs.add(sw.elapsedMicroseconds / 1000);
    if (slot == -1) {
      _dropped++;
    }
    if (slot == renderNoWorld) {
      debugPrint('[fynkel_flutter] render: no world (poisoned?)');
    }
  }

  Future<void> _report() async {
    final c = controller;
    final viewport = c.takeStats();
    final platform = await c.platformStats();
    if (!mounted) return;
    final seconds = _window.elapsedMicroseconds / 1e6;
    final stats = FynkelFrameStats(
      frames: _frames,
      fps: seconds > 0 ? _frames / seconds : 0,
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
    );
    final overlay = widget.overlay;
    if (overlay == null) return viewport;
    return Stack(fit: StackFit.expand, children: [viewport, overlay]);
  }
}
