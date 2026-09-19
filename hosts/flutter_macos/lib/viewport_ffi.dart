// dart:ffi bindings for the macOS viewport ABI (platform/macos/include/
// fynkel_macos.h). The engine ABI comes from package:fynkel; both live in
// the same dylib, provided by package:fynkel's build hook (ADR-0015). This
// is host glue: when `fynkel_flutter` exists it owns this file (generated
// with ffigen like the engine bindings).

import 'dart:ffi';

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

// The viewport symbols are `@Native` externals on the same code asset as
// the engine bindings: package:fynkel's build hook ships one dylib that
// exports both ABIs (platform/macos), so nothing is dlopen-ed here.
const _asset = 'package:fynkel/fynkel_native';

@Native<Uint32 Function()>(symbol: 'fynkel_macos_abi_version', assetId: _asset)
external int _abiVersion();
@Native<Pointer<Void> Function(Uint32, Uint32, Uint32, Int32)>(symbol: 'fynkel_macos_viewport_create', assetId: _asset)
external Pointer<Void> _create(int width, int height, int bufferCount, int syncMode);
@Native<Void Function(Pointer<Void>)>(symbol: 'fynkel_macos_viewport_destroy', assetId: _asset)
external void _destroy(Pointer<Void> vp);
@Native<Int32 Function(Pointer<Void>, Uint32, Uint32)>(symbol: 'fynkel_macos_viewport_resize', assetId: _asset)
external int _resize(Pointer<Void> vp, int width, int height);
@Native<Uint32 Function(Pointer<Void>)>(symbol: 'fynkel_macos_viewport_buffer_count', assetId: _asset)
external int _bufferCount(Pointer<Void> vp);
@Native<Pointer<Void> Function(Pointer<Void>, Uint32)>(symbol: 'fynkel_macos_viewport_iosurface', assetId: _asset)
external Pointer<Void> _iosurface(Pointer<Void> vp, int index);
@Native<Int32 Function(Pointer<Void>, Pointer<Void>)>(symbol: 'fynkel_macos_viewport_render', assetId: _asset)
external int _render(Pointer<Void> vp, Pointer<Void> world);
@Native<Void Function(Pointer<Void>, Pointer<ViewportCamera>)>(symbol: 'fynkel_macos_viewport_set_camera', assetId: _asset)
external void _setCamera(Pointer<Void> vp, Pointer<ViewportCamera> camera);
@Native<Void Function(Pointer<Void>, Uint32)>(symbol: 'fynkel_macos_viewport_set_acquire_wait_us', assetId: _asset)
external void _setAcquireWait(Pointer<Void> vp, int us);
@Native<Void Function(Pointer<Void>, Pointer<Void>, Pointer<Void>)>(
    symbol: 'fynkel_macos_viewport_set_frame_ready_callback', assetId: _asset)
external void _setCallback(Pointer<Void> vp, Pointer<Void> fn, Pointer<Void> userData);
@Native<Int32 Function(Pointer<Void>)>(symbol: 'fynkel_macos_viewport_acquire_presented', assetId: _asset)
external int _acquirePresented(Pointer<Void> vp);
@Native<Void Function(Pointer<Void>, Pointer<ViewportStats>)>(symbol: 'fynkel_macos_viewport_take_stats', assetId: _asset)
external void _takeStats(Pointer<Void> vp, Pointer<ViewportStats> out);

class ViewportNative {
  ViewportNative() {
    final v = _abiVersion();
    if (v != viewportAbiVersion) {
      throw StateError('viewport ABI $v, host expects $viewportAbiVersion');
    }
  }

  /// Raw symbol addresses handed to Swift so it can call Rust directly from
  /// Flutter's raster thread without dlopen-ing the library a second time.
  final int acquireFnAddress = Native.addressOf<NativeFunction<Int32 Function(Pointer<Void>)>>(_acquirePresented).address;
  final int setCallbackFnAddress =
      Native.addressOf<NativeFunction<Void Function(Pointer<Void>, Pointer<Void>, Pointer<Void>)>>(_setCallback).address;

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
