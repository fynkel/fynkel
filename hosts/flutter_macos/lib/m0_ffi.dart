// Hand-written dart:ffi bindings for the throwaway M0 C ABI in
// crates/fynkel_m0. The real fynkel_native ABI is generated with ffigen from
// a C header (ADR-0004); this file exists only for the M0 experiment.

import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

final class M0Stats extends Struct {
  @Uint64()
  external int framesRendered;
  @Uint64()
  external int framesDroppedNoFreeBuffer;
  @Uint64()
  external int framesPresented;
  @Uint64()
  external int presentsWithoutNewFrame;
  @Uint32()
  external int cpuP50Us;
  @Uint32()
  external int cpuP95Us;
  @Uint32()
  external int cpuP99Us;
  @Uint32()
  external int gpuCompleteP50Us;
  @Uint32()
  external int gpuCompleteP95Us;
  @Uint32()
  external int gpuCompleteP99Us;
  @Uint64()
  external int resizes;
  @Uint32()
  external int resizeP50Us;
  @Uint32()
  external int resizeMaxUs;
  @Uint32()
  external int presentLagP50;
  @Uint32()
  external int presentLagP95;
  @Uint32()
  external int presentLagMax;
  @Uint32()
  external int acquireWaitP50Us;
  @Uint32()
  external int acquireWaitP95Us;
  @Uint32()
  external int acquireWaitMaxUs;
}

typedef _CreateC = Pointer<Void> Function(Uint32, Uint32, Uint32, Int32);
typedef _CreateDart = Pointer<Void> Function(int, int, int, int);
typedef _DestroyC = Void Function(Pointer<Void>);
typedef _DestroyDart = void Function(Pointer<Void>);
typedef _IoSurfaceC = Pointer<Void> Function(Pointer<Void>, Uint32);
typedef _IoSurfaceDart = Pointer<Void> Function(Pointer<Void>, int);
typedef _RenderC = Int32 Function(Pointer<Void>, Float);
typedef _RenderDart = int Function(Pointer<Void>, double);
typedef _TakeStatsC = Void Function(Pointer<Void>, Pointer<M0Stats>);
typedef _TakeStatsDart = void Function(Pointer<Void>, Pointer<M0Stats>);
typedef _ResizeC = Int32 Function(Pointer<Void>, Uint32, Uint32);
typedef _ResizeDart = int Function(Pointer<Void>, int, int);
typedef _SetCallbackC = Void Function(Pointer<Void>, Pointer<Void>, Pointer<Void>);
typedef _SetAcquireWaitC = Void Function(Pointer<Void>, Uint32);
typedef _SetAcquireWaitDart = void Function(Pointer<Void>, int);
typedef _SetCallbackDart = void Function(Pointer<Void>, Pointer<Void>, Pointer<Void>);

class M0Native {
  M0Native(this.libraryPath) : _lib = DynamicLibrary.open(libraryPath) {
    _create = _lib.lookupFunction<_CreateC, _CreateDart>('fynkel_m0_create');
    _destroy = _lib.lookupFunction<_DestroyC, _DestroyDart>('fynkel_m0_destroy');
    _iosurface = _lib.lookupFunction<_IoSurfaceC, _IoSurfaceDart>('fynkel_m0_iosurface');
    _render = _lib.lookupFunction<_RenderC, _RenderDart>('fynkel_m0_render');
    _takeStats = _lib.lookupFunction<_TakeStatsC, _TakeStatsDart>('fynkel_m0_take_stats');
    _resize = _lib.lookupFunction<_ResizeC, _ResizeDart>('fynkel_m0_resize');
    _setCallback = _lib.lookupFunction<_SetCallbackC, _SetCallbackDart>('fynkel_m0_set_frame_ready_callback');
    _setAcquireWait = _lib.lookupFunction<_SetAcquireWaitC, _SetAcquireWaitDart>('fynkel_m0_set_acquire_wait_us');
    acquireFnAddress = _lib.lookup<Void>('fynkel_m0_acquire_presented').address;
    setCallbackFnAddress = _lib.lookup<Void>('fynkel_m0_set_frame_ready_callback').address;
  }

  final String libraryPath;
  final DynamicLibrary _lib;
  late final _CreateDart _create;
  late final _DestroyDart _destroy;
  late final _IoSurfaceDart _iosurface;
  late final _RenderDart _render;
  late final _TakeStatsDart _takeStats;
  late final _ResizeDart _resize;
  late final _SetCallbackDart _setCallback;
  late final _SetAcquireWaitDart _setAcquireWait;

  /// Raw symbol addresses handed to Swift so it can call Rust directly from
  /// Flutter's raster thread without dlopen-ing the library a second time.
  late final int acquireFnAddress;
  late final int setCallbackFnAddress;

  /// [syncMode]: 0 = wgpu on_submitted_work_done (polled), 1 = blocking,
  /// 2 = Metal completion handler (see fynkel_m0 `SyncMode`).
  Pointer<Void> create({required int width, required int height, int bufferCount = 4, int syncMode = 2}) =>
      _create(width, height, bufferCount, syncMode);

  void destroy(Pointer<Void> ctx) => _destroy(ctx);

  /// Rebuilds the IOSurface ring; the host must re-wrap the surfaces afterwards.
  bool resize(Pointer<Void> ctx, int width, int height) => _resize(ctx, width, height) == 0;

  /// Clears the Rust -> host frame-ready callback (synchronously; waits for an
  /// in-progress callback). Call before the callback's target goes away.
  void clearFrameReadyCallback(Pointer<Void> ctx) => _setCallback(ctx, nullptr, nullptr);

  /// Bounded wait the presenter (raster thread) may spend on the newest
  /// in-flight frame so it composites this tick's frame (0 disables).
  void setAcquireWaitUs(Pointer<Void> ctx, int us) => _setAcquireWait(ctx, us);

  List<int> iosurfaceAddresses(Pointer<Void> ctx, int count) =>
      List.generate(count, (i) => _iosurface(ctx, i).address);

  /// Returns the buffer index rendered, or -1 if the frame was dropped.
  int render(Pointer<Void> ctx, double timeSecs) => _render(ctx, timeSecs);

  M0StatsSnapshot takeStats(Pointer<Void> ctx) {
    final out = calloc<M0Stats>();
    try {
      _takeStats(ctx, out);
      final s = out.ref;
      return M0StatsSnapshot(
        framesRendered: s.framesRendered,
        framesDropped: s.framesDroppedNoFreeBuffer,
        framesPresented: s.framesPresented,
        presentsWithoutNewFrame: s.presentsWithoutNewFrame,
        cpuP50Ms: s.cpuP50Us / 1000,
        cpuP95Ms: s.cpuP95Us / 1000,
        cpuP99Ms: s.cpuP99Us / 1000,
        gpuP50Ms: s.gpuCompleteP50Us / 1000,
        gpuP95Ms: s.gpuCompleteP95Us / 1000,
        gpuP99Ms: s.gpuCompleteP99Us / 1000,
        resizes: s.resizes,
        resizeP50Ms: s.resizeP50Us / 1000,
        resizeMaxMs: s.resizeMaxUs / 1000,
        presentLagP50: s.presentLagP50,
        presentLagP95: s.presentLagP95,
        presentLagMax: s.presentLagMax,
        acquireWaitP50Ms: s.acquireWaitP50Us / 1000,
        acquireWaitP95Ms: s.acquireWaitP95Us / 1000,
        acquireWaitMaxMs: s.acquireWaitMaxUs / 1000,
      );
    } finally {
      calloc.free(out);
    }
  }

  static String defaultLibraryPath() {
    const fromDefine = String.fromEnvironment('FYNKEL_M0_DYLIB');
    if (fromDefine.isNotEmpty) return fromDefine;
    final env = Platform.environment['FYNKEL_M0_DYLIB'];
    if (env != null && env.isNotEmpty) return env;
    throw StateError('Set --dart-define=FYNKEL_M0_DYLIB=/abs/path/to/libfynkel_m0.dylib');
  }
}

class M0StatsSnapshot {
  const M0StatsSnapshot({
    required this.framesRendered,
    required this.framesDropped,
    required this.framesPresented,
    required this.presentsWithoutNewFrame,
    required this.cpuP50Ms,
    required this.cpuP95Ms,
    required this.cpuP99Ms,
    required this.gpuP50Ms,
    required this.gpuP95Ms,
    required this.gpuP99Ms,
    required this.resizes,
    required this.resizeP50Ms,
    required this.resizeMaxMs,
    required this.presentLagP50,
    required this.presentLagP95,
    required this.presentLagMax,
    required this.acquireWaitP50Ms,
    required this.acquireWaitP95Ms,
    required this.acquireWaitMaxMs,
  });
  final double acquireWaitP50Ms, acquireWaitP95Ms, acquireWaitMaxMs;
  final int framesRendered, framesDropped, framesPresented, presentsWithoutNewFrame, resizes;
  final int presentLagP50, presentLagP95, presentLagMax;
  final double cpuP50Ms, cpuP95Ms, cpuP99Ms, gpuP50Ms, gpuP95Ms, gpuP99Ms, resizeP50Ms, resizeMaxMs;
}

// ---- Path C: engine-owned CAMetalLayer surface (fynkel_m0::surface) --------

final class M0SurfaceStats extends Struct {
  @Uint64()
  external int frames;
  @Uint64()
  external int skipped;
  @Uint64()
  external int resizes;
  @Uint32()
  external int intervalP50Us;
  @Uint32()
  external int intervalP95Us;
  @Uint32()
  external int intervalP99Us;
  @Uint32()
  external int acquireP50Us;
  @Uint32()
  external int acquireP95Us;
  @Uint32()
  external int acquireP99Us;
  @Uint32()
  external int cpuP50Us;
  @Uint32()
  external int cpuP95Us;
  @Uint32()
  external int cpuP99Us;
  @Uint32()
  external int resizeP50Us;
  @Uint32()
  external int resizeMaxUs;
}

typedef _SurfaceCreateC = Pointer<Void> Function(Pointer<Void>, Uint32, Uint32);
typedef _SurfaceCreateDart = Pointer<Void> Function(Pointer<Void>, int, int);
typedef _SurfaceResizeC = Void Function(Pointer<Void>, Uint32, Uint32);
typedef _SurfaceResizeDart = void Function(Pointer<Void>, int, int);
typedef _SurfacePauseC = Void Function(Pointer<Void>, Int32);
typedef _SurfacePauseDart = void Function(Pointer<Void>, int);
typedef _SurfaceStatsC = Void Function(Pointer<Void>, Pointer<M0SurfaceStats>);
typedef _SurfaceStatsDart = void Function(Pointer<Void>, Pointer<M0SurfaceStats>);

class M0SurfaceNative {
  M0SurfaceNative(String libraryPath) : _lib = DynamicLibrary.open(libraryPath) {
    _create = _lib.lookupFunction<_SurfaceCreateC, _SurfaceCreateDart>('fynkel_m0_surface_create');
    _destroy = _lib.lookupFunction<_DestroyC, _DestroyDart>('fynkel_m0_surface_destroy');
    _resize = _lib.lookupFunction<_SurfaceResizeC, _SurfaceResizeDart>('fynkel_m0_surface_resize');
    _setPaused = _lib.lookupFunction<_SurfacePauseC, _SurfacePauseDart>('fynkel_m0_surface_set_paused');
    _takeStats = _lib.lookupFunction<_SurfaceStatsC, _SurfaceStatsDart>('fynkel_m0_surface_take_stats');
  }

  final DynamicLibrary _lib;
  late final _SurfaceCreateDart _create;
  late final _DestroyDart _destroy;
  late final _SurfaceResizeDart _resize;
  late final _SurfacePauseDart _setPaused;
  late final _SurfaceStatsDart _takeStats;

  /// [layerAddress] is a CAMetalLayer the host keeps alive; starts the engine's
  /// render thread immediately.
  Pointer<Void> create({required int layerAddress, required int width, required int height}) =>
      _create(Pointer.fromAddress(layerAddress), width, height);
  void destroy(Pointer<Void> ctx) => _destroy(ctx);
  void resize(Pointer<Void> ctx, int width, int height) => _resize(ctx, width, height);
  void setPaused(Pointer<Void> ctx, bool paused) => _setPaused(ctx, paused ? 1 : 0);

  M0SurfaceStatsSnapshot takeStats(Pointer<Void> ctx) {
    final out = calloc<M0SurfaceStats>();
    try {
      _takeStats(ctx, out);
      final s = out.ref;
      return M0SurfaceStatsSnapshot(
        frames: s.frames,
        skipped: s.skipped,
        resizes: s.resizes,
        intervalMs: [s.intervalP50Us / 1000, s.intervalP95Us / 1000, s.intervalP99Us / 1000],
        acquireMs: [s.acquireP50Us / 1000, s.acquireP95Us / 1000, s.acquireP99Us / 1000],
        cpuMs: [s.cpuP50Us / 1000, s.cpuP95Us / 1000, s.cpuP99Us / 1000],
        resizeP50Ms: s.resizeP50Us / 1000,
        resizeMaxMs: s.resizeMaxUs / 1000,
      );
    } finally {
      calloc.free(out);
    }
  }
}

class M0SurfaceStatsSnapshot {
  const M0SurfaceStatsSnapshot({
    required this.frames,
    required this.skipped,
    required this.resizes,
    required this.intervalMs,
    required this.acquireMs,
    required this.cpuMs,
    required this.resizeP50Ms,
    required this.resizeMaxMs,
  });
  final int frames, skipped, resizes;
  /// p50, p95, p99.
  final List<double> intervalMs, acquireMs, cpuMs;
  final double resizeP50Ms, resizeMaxMs;
}
