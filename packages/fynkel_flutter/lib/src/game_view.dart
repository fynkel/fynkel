// `FynkelGameView`: the Ticker-driven host loop (ADR-0008) around a
// `FynkelViewport`. One Flutter frame = `World.frame` (fixed ticks with the
// §5.1 behaviour points, then update/lateUpdate) followed by one viewport
// render of the resulting extract. Optional per-second stats windows feed
// a HUD or a benchmark capture.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:fynkel/fynkel.dart';

import 'viewport.dart';
import 'viewport_controller.dart';

/// Percentiles of a sample window, in milliseconds.
class MsPercentiles {
  const MsPercentiles(this.p50, this.p95, this.p99, this.count);
  final double p50, p95, p99;
  final int count;

  static const empty = MsPercentiles(0, 0, 0, 0);

  factory MsPercentiles.of(List<double> samples) {
    if (samples.isEmpty) return empty;
    final s = List<double>.from(samples)..sort();
    double at(double q) => s[((s.length - 1) * q).round()];
    return MsPercentiles(at(.5), at(.95), at(.99), s.length);
  }

  String format([int digits = 2]) =>
      'p50=${p50.toStringAsFixed(digits)} p95=${p95.toStringAsFixed(digits)} p99=${p99.toStringAsFixed(digits)}';
}

/// One reporting window (about a second) of the game view's loop.
class FynkelFrameStats {
  const FynkelFrameStats({
    required this.tickInterval,
    required this.simulate,
    required this.renderCall,
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

  /// Time between consecutive Ticker callbacks.
  final MsPercentiles tickInterval;

  /// `World.frame` wall time (gameplay + submit + step + snapshot).
  final MsPercentiles simulate;

  /// `render()` wall time (extract + encode + submit).
  final MsPercentiles renderCall;
  final int fixedTicks, missedDeadlines;

  /// Frames the ring could not take (full) this window.
  final int dropped;
  final int skippedWhileResizing;
  final BridgeStats world;
  final ViewportStatsSnapshot? viewport;
  final ViewportPlatformStats? platform;
  final int physicalWidth, physicalHeight;
  final bool muted;
}

class FynkelGameView extends StatefulWidget {
  const FynkelGameView({
    super.key,
    required this.world,
    this.controller,
    this.fixedUpdate,
    this.onFrame,
    this.onStats,
    this.statsInterval = const Duration(seconds: 1),
    this.overlay,
    this.pauseWhenHidden = true,
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

  /// Clamp for one frame's dt (a debugger pause must not become a burst).
  final double maxFrameDt;
  final bool autofocus;
  final FocusNode? focusNode;
  final FocusOnKeyEventCallback? onKeyEvent;
  final ViewportPointerCallback? onPointerEvent;
  final Widget? placeholder;

  @override
  State<FynkelGameView> createState() => FynkelGameViewState();
}

class FynkelGameViewState extends State<FynkelGameView> with SingleTickerProviderStateMixin, WidgetsBindingObserver {
  FynkelViewportController? _owned;
  FynkelViewportController get controller => widget.controller ?? (_owned ??= FynkelViewportController());
  late final Ticker _ticker;
  Timer? _statsTimer;
  Duration? _lastTick;
  bool _primed = false;

  final _intervalsMs = <double>[];
  final _simMs = <double>[];
  final _renderCallMs = <double>[];
  int _ticks = 0, _missed = 0, _dropped = 0, _skipped = 0;

  /// Whether the loop is currently muted (hidden, or [setPaused]).
  bool get paused => _ticker.muted;
  bool _pausedByUser = false;
  bool _hidden = false;

  /// Pause/resume the loop; the clock is re-primed on resume.
  void setPaused(bool v) {
    _pausedByUser = v;
    _applyMute();
  }

  void _applyMute() {
    final mute = _pausedByUser || (widget.pauseWhenHidden && _hidden);
    if (_ticker.muted == mute) return;
    _ticker.muted = mute;
    if (!mute) {
      _primed = false;
    }
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
    _ticker = createTicker(_onTick)..start();
    if (widget.onStats != null) {
      _statsTimer = Timer.periodic(widget.statsInterval, (_) => _report());
    }
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
    if ((oldWidget.onStats == null) != (widget.onStats == null) || oldWidget.statsInterval != widget.statsInterval) {
      _statsTimer?.cancel();
      _statsTimer = widget.onStats == null ? null : Timer.periodic(widget.statsInterval, (_) => _report());
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
    _ticker.dispose();
    _statsTimer?.cancel();
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

    final sim = Stopwatch()..start();
    final plan = world.frame(dt, fixedUpdate: widget.fixedUpdate);
    sim.stop();
    _simMs.add(sim.elapsedMicroseconds / 1000);
    _ticks += plan.steps;
    if (plan.missedDeadline) {
      _missed++;
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
    final stats = FynkelFrameStats(
      tickInterval: MsPercentiles.of(_intervalsMs),
      simulate: MsPercentiles.of(_simMs),
      renderCall: MsPercentiles.of(_renderCallMs),
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
    _renderCallMs.clear();
    _ticks = _missed = _dropped = _skipped = 0;
    widget.onStats?.call(stats);
  }

  @override
  Widget build(BuildContext context) {
    final viewport = FynkelViewport(
      controller: controller,
      focusNode: widget.focusNode,
      autofocus: widget.autofocus,
      onKeyEvent: widget.onKeyEvent,
      onPointerEvent: widget.onPointerEvent,
      placeholder: widget.placeholder,
    );
    final overlay = widget.overlay;
    if (overlay == null) return viewport;
    return Stack(fit: StackFit.expand, children: [viewport, overlay]);
  }
}
