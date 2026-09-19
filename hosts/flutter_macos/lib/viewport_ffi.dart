// dart:ffi bindings for the macOS viewport ABI (platform/macos/include/
// fynkel_macos.h). The engine ABI comes from package:fynkel; both live in
// the same dylib. This is host glue: when `fynkel_flutter` exists it owns
// this file (generated with ffigen like the engine bindings).

import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

const viewportAbiVersion = 1;

/// `fynkel_macos_viewport_render` result when the world pointer is null or
/// the world is poisoned.
const renderNoWorld = -2;

final class ViewportStats extends Struct {
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
  @Uint32()
  external int instancesLastFrame;
  @Uint32()
  external int frameReadySignals;
}

final class ViewportCamera extends Struct {
  @Array(3)
  external Array<Float> eye;
  @Array(3)
  external Array<Float> target;
  @Array(3)
  external Array<Float> up;
  @Float()
  external double fovYRadians;
  @Float()
  external double near;
  @Float()
  external double far;
}

typedef _AbiVersionC = Uint32 Function();
typedef _AbiVersionDart = int Function();
typedef _CreateC = Pointer<Void> Function(Uint32, Uint32, Uint32, Int32);
typedef _CreateDart = Pointer<Void> Function(int, int, int, int);
typedef _DestroyC = Void Function(Pointer<Void>);
typedef _DestroyDart = void Function(Pointer<Void>);
typedef _ResizeC = Int32 Function(Pointer<Void>, Uint32, Uint32);
typedef _ResizeDart = int Function(Pointer<Void>, int, int);
typedef _BufferCountC = Uint32 Function(Pointer<Void>);
typedef _BufferCountDart = int Function(Pointer<Void>);
typedef _IoSurfaceC = Pointer<Void> Function(Pointer<Void>, Uint32);
typedef _IoSurfaceDart = Pointer<Void> Function(Pointer<Void>, int);
typedef _RenderC = Int32 Function(Pointer<Void>, Pointer<Void>);
typedef _RenderDart = int Function(Pointer<Void>, Pointer<Void>);
typedef _SetCameraC = Void Function(Pointer<Void>, Pointer<ViewportCamera>);
typedef _SetCameraDart = void Function(Pointer<Void>, Pointer<ViewportCamera>);
typedef _SetAcquireWaitC = Void Function(Pointer<Void>, Uint32);
typedef _SetAcquireWaitDart = void Function(Pointer<Void>, int);
typedef _SetCallbackC = Void Function(Pointer<Void>, Pointer<Void>, Pointer<Void>);
typedef _SetCallbackDart = void Function(Pointer<Void>, Pointer<Void>, Pointer<Void>);
typedef _TakeStatsC = Void Function(Pointer<Void>, Pointer<ViewportStats>);
typedef _TakeStatsDart = void Function(Pointer<Void>, Pointer<ViewportStats>);

class ViewportNative {
  ViewportNative(this.libraryPath) : _lib = DynamicLibrary.open(libraryPath) {
    final v = _lib.lookupFunction<_AbiVersionC, _AbiVersionDart>('fynkel_macos_abi_version')();
    if (v != viewportAbiVersion) {
      throw StateError('viewport ABI $v, host expects $viewportAbiVersion');
    }
    _create = _lib.lookupFunction<_CreateC, _CreateDart>('fynkel_macos_viewport_create');
    _destroy = _lib.lookupFunction<_DestroyC, _DestroyDart>('fynkel_macos_viewport_destroy');
    _resize = _lib.lookupFunction<_ResizeC, _ResizeDart>('fynkel_macos_viewport_resize');
    _bufferCount = _lib.lookupFunction<_BufferCountC, _BufferCountDart>('fynkel_macos_viewport_buffer_count');
    _iosurface = _lib.lookupFunction<_IoSurfaceC, _IoSurfaceDart>('fynkel_macos_viewport_iosurface');
    _render = _lib.lookupFunction<_RenderC, _RenderDart>('fynkel_macos_viewport_render');
    _setCamera = _lib.lookupFunction<_SetCameraC, _SetCameraDart>('fynkel_macos_viewport_set_camera');
    _setAcquireWait =
        _lib.lookupFunction<_SetAcquireWaitC, _SetAcquireWaitDart>('fynkel_macos_viewport_set_acquire_wait_us');
    _setCallback =
        _lib.lookupFunction<_SetCallbackC, _SetCallbackDart>('fynkel_macos_viewport_set_frame_ready_callback');
    _takeStats = _lib.lookupFunction<_TakeStatsC, _TakeStatsDart>('fynkel_macos_viewport_take_stats');
    acquireFnAddress = _lib.lookup<Void>('fynkel_macos_viewport_acquire_presented').address;
    setCallbackFnAddress = _lib.lookup<Void>('fynkel_macos_viewport_set_frame_ready_callback').address;
  }

  final String libraryPath;
  final DynamicLibrary _lib;
  late final _CreateDart _create;
  late final _DestroyDart _destroy;
  late final _ResizeDart _resize;
  late final _BufferCountDart _bufferCount;
  late final _IoSurfaceDart _iosurface;
  late final _RenderDart _render;
  late final _SetCameraDart _setCamera;
  late final _SetAcquireWaitDart _setAcquireWait;
  late final _SetCallbackDart _setCallback;
  late final _TakeStatsDart _takeStats;

  /// Raw symbol addresses handed to Swift so it can call Rust directly from
  /// Flutter's raster thread without dlopen-ing the library a second time.
  late final int acquireFnAddress;
  late final int setCallbackFnAddress;

  /// [syncMode]: 0 = wgpu on_submitted_work_done, 1 = blocking, 2 = Metal
  /// completion handler (production).
  Pointer<Void> create({required int width, required int height, int bufferCount = 4, int syncMode = 2}) =>
      _create(width, height, bufferCount, syncMode);

  void destroy(Pointer<Void> vp) => _destroy(vp);

  /// Rebuilds the IOSurface ring; the host must re-wrap the surfaces afterwards.
  bool resize(Pointer<Void> vp, int width, int height) => _resize(vp, width, height) == 0;

  int bufferCount(Pointer<Void> vp) => _bufferCount(vp);

  List<int> iosurfaceAddresses(Pointer<Void> vp) =>
      List.generate(bufferCount(vp), (i) => _iosurface(vp, i).address);

  /// Renders the world's current extract. Returns the slot index, -1 if the
  /// frame was dropped (ring full) or [renderNoWorld].
  int render(Pointer<Void> vp, int worldHandleAddress) => _render(vp, Pointer.fromAddress(worldHandleAddress));

  void setCamera(Pointer<Void> vp,
      {required List<double> eye, required List<double> target, double fovYDegrees = 60, double near = 0.1, double far = 100}) {
    final c = calloc<ViewportCamera>();
    try {
      for (var i = 0; i < 3; i++) {
        c.ref.eye[i] = eye[i];
        c.ref.target[i] = target[i];
        c.ref.up[i] = i == 1 ? 1 : 0;
      }
      c.ref.fovYRadians = fovYDegrees * 3.141592653589793 / 180;
      c.ref.near = near;
      c.ref.far = far;
      _setCamera(vp, c);
    } finally {
      calloc.free(c);
    }
  }

  /// Bounded wait the presenter (raster thread) may spend on the newest
  /// in-flight frame so it composites this tick's frame (0 disables).
  void setAcquireWaitUs(Pointer<Void> vp, int us) => _setAcquireWait(vp, us);

  /// Clears the Rust -> host frame-ready callback (synchronously; waits for
  /// an in-progress callback). Call before the callback's target goes away.
  void clearFrameReadyCallback(Pointer<Void> vp) => _setCallback(vp, nullptr, nullptr);

  ViewportStatsSnapshot takeStats(Pointer<Void> vp) {
    final out = calloc<ViewportStats>();
    try {
      _takeStats(vp, out);
      final s = out.ref;
      return ViewportStatsSnapshot(
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
        instances: s.instancesLastFrame,
      );
    } finally {
      calloc.free(out);
    }
  }

  /// The one dylib exporting the engine and viewport ABIs.
  static String defaultLibraryPath() {
    const fromDefine = String.fromEnvironment('FYNKEL_NATIVE_LIB');
    if (fromDefine.isNotEmpty) return fromDefine;
    final env = Platform.environment['FYNKEL_NATIVE_LIB'];
    if (env != null && env.isNotEmpty) return env;
    throw StateError('Set --dart-define=FYNKEL_NATIVE_LIB=/abs/path/to/libfynkel_platform_macos.dylib');
  }
}

class ViewportStatsSnapshot {
  const ViewportStatsSnapshot({
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
    required this.instances,
  });
  final double acquireWaitP50Ms, acquireWaitP95Ms, acquireWaitMaxMs;
  final int framesRendered, framesDropped, framesPresented, presentsWithoutNewFrame, resizes, instances;
  final int presentLagP50, presentLagP95, presentLagMax;
  final double cpuP50Ms, cpuP95Ms, cpuP99Ms, gpuP50Ms, gpuP95Ms, gpuP99Ms, resizeP50Ms, resizeMaxMs;
}
