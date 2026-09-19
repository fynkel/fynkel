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

  /// Raw symbol addresses handed to Swift so it can call Rust directly from
  /// Flutter's raster thread without dlopen-ing the library a second time.
  late final int acquireFnAddress;
  late final int setCallbackFnAddress;

  Pointer<Void> create({required int width, required int height, int bufferCount = 4, bool blocking = false}) =>
      _create(width, height, bufferCount, blocking ? 1 : 0);

  void destroy(Pointer<Void> ctx) => _destroy(ctx);

  /// Rebuilds the IOSurface ring; the host must re-wrap the surfaces afterwards.
  bool resize(Pointer<Void> ctx, int width, int height) => _resize(ctx, width, height) == 0;

  /// Clears the Rust -> host frame-ready callback (synchronously; waits for an
  /// in-progress callback). Call before the callback's target goes away.
  void clearFrameReadyCallback(Pointer<Void> ctx) => _setCallback(ctx, nullptr, nullptr);

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
  });
  final int framesRendered, framesDropped, framesPresented, presentsWithoutNewFrame, resizes;
  final double cpuP50Ms, cpuP95Ms, cpuP99Ms, gpuP50Ms, gpuP95Ms, gpuP99Ms, resizeP50Ms, resizeMaxMs;
}
