// dart:ffi bindings for the Apple viewport ABI (platform/apple/include/
// fynkel_apple.h). The engine ABI comes from package:fynkel; both live in
// the same dylib, provided by package:fynkel's build hook (ADR-0015), so the
// symbols below are `@Native` externals on that code asset — nothing is
// dlopen-ed. Package-private: `FynkelViewportController` is the API.

import 'dart:convert';
import 'dart:ffi';

import 'package:ffi/ffi.dart';

import 'percentiles.dart';

const viewportAbiVersion = 3;

/// `fynkel_apple_viewport_render` result when the world pointer is null or
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
  @Uint32()
  external int meshesLastFrame;
  @Uint32()
  external int placeholdersLastFrame;
  @Uint32()
  external int missingLastFrame;
  @Uint32()
  external int assetErrors;
  @Uint32()
  external int sceneCamera;
  @Uint32()
  external int transparentLastFrame;
  @Uint32()
  external int lights;
  @Uint32()
  external int lightsDropped;
  @Uint32()
  external int shadowCasters;
  @Uint32()
  external int shadowMap;
  @Uint32()
  external int msaa;
  @Uint32()
  external int renderWidth;
  @Uint32()
  external int renderHeight;
  @Uint32()
  external int culledLastFrame;
  @Uint32()
  external int drawsLastFrame;
  @Uint32()
  external int shadowDrawsLastFrame;
  @Uint32()
  external int extractP50Us;
  @Uint32()
  external int extractP95Us;
  @Uint32()
  external int extractP99Us;
  @Uint32()
  external int prepareP50Us;
  @Uint32()
  external int prepareP95Us;
  @Uint32()
  external int prepareP99Us;
  @Uint32()
  external int encodeP50Us;
  @Uint32()
  external int encodeP95Us;
  @Uint32()
  external int encodeP99Us;
  @Uint32()
  external int gpuTimestamps;
  @Uint32()
  external int gpuFramesTimed;
  @Uint32()
  external int gpuFramesUntimed;
  @Uint32()
  external int gpuShadowP50Us;
  @Uint32()
  external int gpuShadowP95Us;
  @Uint32()
  external int gpuShadowP99Us;
  @Uint32()
  external int gpuSceneP50Us;
  @Uint32()
  external int gpuSceneP95Us;
  @Uint32()
  external int gpuSceneP99Us;
  @Uint32()
  external int gpuPostP50Us;
  @Uint32()
  external int gpuPostP95Us;
  @Uint32()
  external int gpuPostP99Us;
  @Uint32()
  external int gpuFrameP50Us;
  @Uint32()
  external int gpuFrameP95Us;
  @Uint32()
  external int gpuFrameP99Us;
  @Uint64()
  external int gpuBytes;
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
// exports both ABIs (platform/apple), so nothing is dlopen-ed here.
const _asset = 'package:fynkel/fynkel_native';

@Native<Uint32 Function()>(symbol: 'fynkel_apple_abi_version', assetId: _asset)
external int _abiVersion();
@Native<Pointer<Void> Function(Uint32, Uint32, Uint32, Int32)>(symbol: 'fynkel_apple_viewport_create', assetId: _asset)
external Pointer<Void> _create(int width, int height, int bufferCount, int syncMode);
@Native<Void Function(Pointer<Void>)>(symbol: 'fynkel_apple_viewport_destroy', assetId: _asset)
external void _destroy(Pointer<Void> vp);
@Native<Int32 Function(Pointer<Void>, Uint32, Uint32)>(symbol: 'fynkel_apple_viewport_resize', assetId: _asset)
external int _resize(Pointer<Void> vp, int width, int height);
@Native<Uint32 Function(Pointer<Void>)>(symbol: 'fynkel_apple_viewport_buffer_count', assetId: _asset)
external int _bufferCount(Pointer<Void> vp);
@Native<Pointer<Void> Function(Pointer<Void>, Uint32)>(symbol: 'fynkel_apple_viewport_iosurface', assetId: _asset)
external Pointer<Void> _iosurface(Pointer<Void> vp, int index);
@Native<Int32 Function(Pointer<Void>, Pointer<Void>)>(symbol: 'fynkel_apple_viewport_render', assetId: _asset)
external int _render(Pointer<Void> vp, Pointer<Void> world);
@Native<Void Function(Pointer<Void>, Pointer<ViewportCamera>)>(
  symbol: 'fynkel_apple_viewport_set_camera',
  assetId: _asset,
)
external void _setCamera(Pointer<Void> vp, Pointer<ViewportCamera> camera);
@Native<Void Function(Pointer<Void>, Uint32)>(symbol: 'fynkel_apple_viewport_set_acquire_wait_us', assetId: _asset)
external void _setAcquireWait(Pointer<Void> vp, int us);
@Native<Void Function(Pointer<Void>, Pointer<Void>, Pointer<Void>)>(
  symbol: 'fynkel_apple_viewport_set_frame_ready_callback',
  assetId: _asset,
)
external void _setCallback(Pointer<Void> vp, Pointer<Void> fn, Pointer<Void> userData);
@Native<Int32 Function(Pointer<Void>)>(symbol: 'fynkel_apple_viewport_acquire_presented', assetId: _asset)
external int _acquirePresented(Pointer<Void> vp);
@Native<Void Function(Pointer<Void>, Pointer<ViewportStats>)>(
  symbol: 'fynkel_apple_viewport_take_stats',
  assetId: _asset,
)
external void _takeStats(Pointer<Void> vp, Pointer<ViewportStats> out);
@Native<Int32 Function(Pointer<Void>, Pointer<Uint8>, Uint32)>(
  symbol: 'fynkel_apple_viewport_set_asset_cache',
  assetId: _asset,
)
external int _setAssetCache(Pointer<Void> vp, Pointer<Uint8> dir, int len);
@Native<Void Function(Pointer<Void>, Int32)>(symbol: 'fynkel_apple_viewport_set_placeholders', assetId: _asset)
external void _setPlaceholders(Pointer<Void> vp, int enabled);
@Native<Int32 Function(Pointer<Void>, Pointer<Uint8>, Uint32)>(
  symbol: 'fynkel_apple_viewport_set_quality',
  assetId: _asset,
)
external int _setQuality(Pointer<Void> vp, Pointer<Uint8> name, int len);

class ViewportNative {
  ViewportNative() {
    final v = _abiVersion();
    if (v != viewportAbiVersion) {
      throw StateError('viewport ABI $v, host expects $viewportAbiVersion');
    }
  }

  /// Raw symbol addresses handed to Swift so it can call Rust directly from
  /// Flutter's raster thread without dlopen-ing the library a second time.
  final int acquireFnAddress = Native.addressOf<NativeFunction<Int32 Function(Pointer<Void>)>>(_acquirePresented)
      .address;
  final int setCallbackFnAddress =
      Native.addressOf<NativeFunction<Void Function(Pointer<Void>, Pointer<Void>, Pointer<Void>)>>(_setCallback)
          .address;

  /// [syncMode]: 0 = wgpu on_submitted_work_done, 1 = blocking, 2 = Metal
  /// completion handler (production).
  Pointer<Void> create({required int width, required int height, int bufferCount = 4, int syncMode = 2}) =>
      _create(width, height, bufferCount, syncMode);

  void destroy(Pointer<Void> vp) => _destroy(vp);

  /// Rebuilds the IOSurface ring; the host must re-wrap the surfaces afterwards.
  bool resize(Pointer<Void> vp, int width, int height) => _resize(vp, width, height) == 0;

  int bufferCount(Pointer<Void> vp) => _bufferCount(vp);

  List<int> iosurfaceAddresses(Pointer<Void> vp) => List.generate(bufferCount(vp), (i) => _iosurface(vp, i).address);

  /// Renders the world's current extract. Returns the slot index, -1 if the
  /// frame was dropped (ring full) or [renderNoWorld].
  int render(Pointer<Void> vp, int worldHandleAddress) => _render(vp, Pointer.fromAddress(worldHandleAddress));

  void setCamera(
    Pointer<Void> vp, {
    required List<double> eye,
    required List<double> target,
    double fovYDegrees = 60,
    double near = 0.1,
    double far = 100,
  }) {
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

  /// Points the renderer at a `fynkel cook` cache directory (null clears).
  /// False when the directory is not a readable cache.
  bool setAssetCache(Pointer<Void> vp, String? dir) {
    if (dir == null || dir.isEmpty) return _setAssetCache(vp, nullptr, 0) == 0;
    final bytes = utf8.encode(dir);
    final p = calloc<Uint8>(bytes.length + 1);
    try {
      p.asTypedList(bytes.length).setAll(0, bytes);
      return _setAssetCache(vp, p, bytes.length) == 0;
    } finally {
      calloc.free(p);
    }
  }

  /// Whether entities without a MeshRenderer draw as placeholder cubes.
  void setPlaceholders(Pointer<Void> vp, bool enabled) => _setPlaceholders(vp, enabled ? 1 : 0);

  /// Selects the project's quality profile by name (null: the project's
  /// default). False when the store being drawn from has profiles and none
  /// has that name.
  bool setQuality(Pointer<Void> vp, String? profile) {
    if (profile == null || profile.isEmpty) return _setQuality(vp, nullptr, 0) == 0;
    final bytes = utf8.encode(profile);
    final p = calloc<Uint8>(bytes.length + 1);
    try {
      p.asTypedList(bytes.length).setAll(0, bytes);
      return _setQuality(vp, p, bytes.length) == 0;
    } finally {
      calloc.free(p);
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
        meshes: s.meshesLastFrame,
        placeholders: s.placeholdersLastFrame,
        missing: s.missingLastFrame,
        assetErrors: s.assetErrors,
        sceneCamera: s.sceneCamera != 0,
        transparent: s.transparentLastFrame,
        lights: s.lights,
        lightsDropped: s.lightsDropped,
        shadowCasters: s.shadowCasters,
        shadowMap: s.shadowMap,
        msaa: s.msaa,
        renderWidth: s.renderWidth,
        renderHeight: s.renderHeight,
        culled: s.culledLastFrame,
        draws: s.drawsLastFrame,
        shadowDraws: s.shadowDrawsLastFrame,
        extract: MsPercentiles(s.extractP50Us / 1000, s.extractP95Us / 1000, s.extractP99Us / 1000, s.framesRendered),
        prepare: MsPercentiles(s.prepareP50Us / 1000, s.prepareP95Us / 1000, s.prepareP99Us / 1000, s.framesRendered),
        encode: MsPercentiles(s.encodeP50Us / 1000, s.encodeP95Us / 1000, s.encodeP99Us / 1000, s.framesRendered),
        gpuTimestamps: s.gpuTimestamps != 0,
        gpuFramesUntimed: s.gpuFramesUntimed,
        gpuShadow: MsPercentiles(s.gpuShadowP50Us / 1000, s.gpuShadowP95Us / 1000, s.gpuShadowP99Us / 1000, s.gpuFramesTimed),
        gpuScene: MsPercentiles(s.gpuSceneP50Us / 1000, s.gpuSceneP95Us / 1000, s.gpuSceneP99Us / 1000, s.gpuFramesTimed),
        gpuPost: MsPercentiles(s.gpuPostP50Us / 1000, s.gpuPostP95Us / 1000, s.gpuPostP99Us / 1000, s.gpuFramesTimed),
        gpuFrame: MsPercentiles(s.gpuFrameP50Us / 1000, s.gpuFrameP95Us / 1000, s.gpuFrameP99Us / 1000, s.gpuFramesTimed),
        gpuBytes: s.gpuBytes,
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
    this.meshes = 0,
    this.placeholders = 0,
    this.missing = 0,
    this.assetErrors = 0,
    this.sceneCamera = false,
    this.transparent = 0,
    this.lights = 0,
    this.lightsDropped = 0,
    this.shadowCasters = 0,
    this.shadowMap = 0,
    this.msaa = 1,
    this.renderWidth = 0,
    this.renderHeight = 0,
    this.culled = 0,
    this.draws = 0,
    this.shadowDraws = 0,
    this.extract = MsPercentiles.empty,
    this.prepare = MsPercentiles.empty,
    this.encode = MsPercentiles.empty,
    this.gpuTimestamps = false,
    this.gpuFramesUntimed = 0,
    this.gpuShadow = MsPercentiles.empty,
    this.gpuScene = MsPercentiles.empty,
    this.gpuPost = MsPercentiles.empty,
    this.gpuFrame = MsPercentiles.empty,
    this.gpuBytes = 0,
  });
  final int meshes, placeholders, missing, assetErrors;
  final bool sceneCamera;

  /// Last frame: blend instances drawn in the transparent pass; point/spot
  /// lights kept and dropped over the per-draw budget; the sun's shadow
  /// casters and the shadow map side in texels (0 when no shadow was
  /// drawn); samples per pixel; the internal render size (the target ×
  /// the quality profile's render scale).
  final int transparent, lights, lightsDropped, shadowCasters, shadowMap, msaa, renderWidth, renderHeight;

  /// Last frame: instances outside the camera frustum (not drawn — they
  /// are not in [instances]); mesh draw calls in the scene pass (one per
  /// opaque `(mesh, material)` batch in view plus one per transparent
  /// instance); draw calls in the shadow pass.
  final int culled, draws, shadowDraws;
  final double acquireWaitP50Ms, acquireWaitP95Ms, acquireWaitMaxMs;
  final int framesRendered, framesDropped, framesPresented, presentsWithoutNewFrame, resizes, instances;
  final int presentLagP50, presentLagP95, presentLagMax;
  final double cpuP50Ms, cpuP95Ms, cpuP99Ms, gpuP50Ms, gpuP95Ms, gpuP99Ms, resizeP50Ms, resizeMaxMs;

  /// The render call's stages over the window (M3-16, blueprint §15.4):
  /// the world's extract, the renderer's prepare (upload + cull) and
  /// encode (pass recording); `cpuP*Ms` is encode + submit.
  final MsPercentiles extract, prepare, encode;

  /// GPU pass times from Metal timestamps ([gpuTimestamps]; a device
  /// without them reports zeros): the shadow pass, the scene pass, post
  /// (+ present) and the frame's span from the first pass's begin to the
  /// last pass's end — the passes overlap on Apple GPUs, so the sum
  /// exceeds the span. The values trail the draw by three or four frames
  /// and their `count` is the frames read back this window;
  /// [gpuFramesUntimed] counts the frames (since the viewport was created)
  /// whose stamps could not be taken because the GPU was too far behind.
  final bool gpuTimestamps;
  final int gpuFramesUntimed;
  final MsPercentiles gpuShadow, gpuScene, gpuPost, gpuFrame;

  /// The renderer's estimate of the GPU memory it holds: resident meshes
  /// and textures, the attachments, the shadow map, its buffers.
  final int gpuBytes;
}
