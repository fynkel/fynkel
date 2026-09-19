// One native viewport and the Flutter texture that presents it.
//
// The controller owns the Rust side (`fynkel_apple_viewport_*`: an IOSurface
// ring the renderer draws the world's extract into) and the Swift side (a
// `FlutterTexture` registered through the plugin's method channel that
// hands Flutter the newest GPU-complete surface). `FynkelViewport` sizes it
// from layout; `FynkelGameView` (or any host loop) calls [render] once per
// frame after the simulation. Teardown is ordered (see [detach]) so the
// raster thread never calls into freed memory.

import 'dart:async';
import 'dart:ffi';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart';

import 'viewport_native.dart';

export 'viewport_native.dart' show ViewportStatsSnapshot, renderNoWorld, viewportAbiVersion;

/// How the renderer learns that a frame's GPU work is complete.
enum ViewportSyncMode {
  /// wgpu `on_submitted_work_done` polling.
  asyncPoll,

  /// Block until the GPU is done (measurement only).
  blocking,

  /// Metal command-buffer completion handler (production).
  metalCompletion,
}

/// A camera set from Dart when the scene has no `Camera` component (the
/// scene's camera wins whenever one exists).
class ViewportCameraSpec {
  const ViewportCameraSpec({
    required this.eye,
    required this.target,
    this.fovYDegrees = 60,
    this.near = 0.1,
    this.far = 100,
  });
  final Vec3 eye, target;
  final double fovYDegrees, near, far;
}

/// Counters Swift keeps per texture (`copyPixelBuffer` traffic), reset on read.
class ViewportPlatformStats {
  const ViewportPlatformStats({
    this.copyCalls = 0,
    this.copyWithNewFrame = 0,
    this.copyNoFrame = 0,
    this.frameReadySignals = 0,
    this.rebinds = 0,
  });
  final int copyCalls, copyWithNewFrame, copyNoFrame, frameReadySignals, rebinds;
}

/// The state of a [FynkelViewportController].
enum ViewportState { detached, creating, attached, resizing, failed }

class FynkelViewportController extends ChangeNotifier {
  FynkelViewportController({
    this.bufferCount = 4,
    this.syncMode = ViewportSyncMode.metalCompletion,
    this.acquireWaitUs = 3000,
    bool placeholders = true,
    String? assetCache,
    ViewportCameraSpec? camera,
  }) // The setters below push changes to the live viewport; the constructor
    // only seeds the fields.
    // ignore: prefer_initializing_formals
    : _placeholders = placeholders,
       // ignore: prefer_initializing_formals
       _assetCache = assetCache,
       // ignore: prefer_initializing_formals
       _camera = camera;

  static const _channel = MethodChannel('fynkel_flutter/viewport');

  /// IOSurfaces in the ring (M0: 4 is the measured sweet spot).
  final int bufferCount;
  final ViewportSyncMode syncMode;

  /// Bounded wait the presenter (raster thread) may spend on the newest
  /// in-flight frame so it composites this tick's frame (0 disables).
  final int acquireWaitUs;

  ViewportNative? _native;
  Pointer<Void> _viewport = nullptr;
  int? _textureId;
  int _width = 0;
  int _height = 0;
  (int, int)? _pendingSize;
  ViewportState _state = ViewportState.detached;
  Object? _error;
  bool _placeholders;
  String? _assetCache;
  bool _assetCacheOk = true;
  ViewportCameraSpec? _camera;
  Future<void>? _detaching;

  ViewportState get state => _state;
  bool get isAttached => _state == ViewportState.attached || _state == ViewportState.resizing;

  /// Why [state] is [ViewportState.failed].
  Object? get error => _error;

  /// Flutter's id for the texture, once attached.
  int? get textureId => _textureId;

  /// Ring size in physical pixels.
  int get physicalWidth => _width;
  int get physicalHeight => _height;

  /// Whether entities without a `MeshRenderer` draw as placeholder cubes.
  bool get placeholders => _placeholders;
  set placeholders(bool v) {
    _placeholders = v;
    if (_viewport != nullptr) {
      _native!.setPlaceholders(_viewport, v);
    }
  }

  /// The `fynkel cook` cache the renderer reads meshes, textures and
  /// materials from (`LoadReport.cacheDir`); null renders placeholders only.
  String? get assetCache => _assetCache;
  set assetCache(String? dir) {
    _assetCache = dir;
    if (_viewport != nullptr) {
      _assetCacheOk = _native!.setAssetCache(_viewport, dir);
    }
  }

  /// False when [assetCache] is set but is not a readable cache (the world
  /// then renders magenta meshes and `assetErrors` in the stats climb).
  bool get assetCacheOk => _assetCacheOk;

  ViewportCameraSpec? get camera => _camera;
  set camera(ViewportCameraSpec? c) {
    _camera = c;
    if (c != null && _viewport != nullptr) {
      _applyCamera(c);
    }
  }

  void _applyCamera(ViewportCameraSpec c) => _native!.setCamera(
    _viewport,
    eye: [c.eye.x, c.eye.y, c.eye.z],
    target: [c.target.x, c.target.y, c.target.z],
    fovYDegrees: c.fovYDegrees,
    near: c.near,
    far: c.far,
  );

  /// Sizes (or resizes) the viewport in physical pixels; the widget calls
  /// this from layout. The first call creates the ring and registers the
  /// texture; later calls are coalesced resizes (stop rendering → Rust
  /// rebuilds the ring → Swift re-wraps the surfaces → resume).
  void requestSize(int width, int height) {
    if (width <= 0 || height <= 0 || _detaching != null) return;
    switch (_state) {
      case ViewportState.detached:
        unawaited(_create(width, height));
      case ViewportState.creating:
      case ViewportState.failed:
        return;
      case ViewportState.attached:
      case ViewportState.resizing:
        if (width == _width && height == _height) {
          _pendingSize = null;
          return;
        }
        _pendingSize = (width, height);
        if (_state == ViewportState.attached) {
          unawaited(_drainResizes());
        }
    }
  }

  Future<void> _create(int width, int height) async {
    _state = ViewportState.creating;
    _width = width;
    _height = height;
    try {
      final native = _native ??= ViewportNative();
      final vp = native.create(width: width, height: height, bufferCount: bufferCount, syncMode: syncMode.index);
      if (vp == nullptr) {
        throw StateError('fynkel_apple_viewport_create returned null (see the native log)');
      }
      _viewport = vp;
      native.setAcquireWaitUs(vp, acquireWaitUs);
      native.setPlaceholders(vp, _placeholders);
      if (_assetCache != null) {
        _assetCacheOk = native.setAssetCache(vp, _assetCache);
      }
      final cam = _camera;
      if (cam != null) {
        _applyCamera(cam);
      }
      final id = await _channel.invokeMethod<int>('registerTexture', {
        'ctx': vp.address,
        'acquireFn': native.acquireFnAddress,
        'setCallbackFn': native.setCallbackFnAddress,
        'iosurfaces': native.iosurfaceAddresses(vp),
      });
      if (_detaching != null) {
        // Detached while the channel call was in flight: finish the teardown
        // the detach could not do yet.
        if (id != null) {
          await _channel.invokeMethod('unregisterTexture', {'textureId': id});
        }
        native.destroy(vp);
        _viewport = nullptr;
        return;
      }
      _textureId = id;
      _state = ViewportState.attached;
      _notify();
      if (_pendingSize != null) {
        unawaited(_drainResizes());
      }
    } catch (e) {
      _error = e;
      _state = ViewportState.failed;
      if (_viewport != nullptr) {
        _native!.destroy(_viewport);
        _viewport = nullptr;
      }
      _notify();
    }
  }

  Future<void> _drainResizes() async {
    _state = ViewportState.resizing;
    try {
      while (_pendingSize != null && _detaching == null) {
        final (w, h) = _pendingSize!;
        _pendingSize = null;
        if (w == _width && h == _height) continue;
        if (!_native!.resize(_viewport, w, h)) {
          debugPrint('[fynkel_flutter] viewport resize to ${w}x$h failed');
          continue;
        }
        await _channel.invokeMethod('rebindTexture', {
          'textureId': _textureId,
          'iosurfaces': _native!.iosurfaceAddresses(_viewport),
        });
        _width = w;
        _height = h;
        _resizes++;
      }
    } finally {
      if (_state == ViewportState.resizing) {
        _state = ViewportState.attached;
      }
      _notify();
    }
  }

  int _resizes = 0;

  /// Resizes completed since attach.
  int get resizes => _resizes;

  /// Renders [world]'s current extract into the next free ring slot.
  /// Returns the slot index, -1 when the ring was full (frame dropped) or
  /// [renderNoWorld]; null when the viewport is not attached or is mid-resize
  /// (the simulation still advanced, so no time is lost).
  int? render(World world) {
    if (_state != ViewportState.attached) return null;
    return _native!.render(_viewport, world.native.handleAddress);
  }

  /// Rust-side counters since the last call (encode/GPU/present timings).
  ViewportStatsSnapshot? takeStats() => isAttached ? _native!.takeStats(_viewport) : null;

  /// Swift-side counters since the last call.
  Future<ViewportPlatformStats?> platformStats() async {
    final id = _textureId;
    if (id == null) return null;
    final m = await _channel.invokeMapMethod<String, dynamic>('stats', {'textureId': id});
    if (m == null) return null;
    return ViewportPlatformStats(
      copyCalls: m['copyCalls'] as int? ?? 0,
      copyWithNewFrame: m['copyWithNewFrame'] as int? ?? 0,
      copyNoFrame: m['copyNoFrame'] as int? ?? 0,
      frameReadySignals: m['frameReadySignals'] as int? ?? 0,
      rebinds: m['rebinds'] as int? ?? 0,
    );
  }

  /// Ordered teardown of the native side; the controller is reusable
  /// afterwards (the next [requestSize] creates a new ring). Safe to call
  /// more than once. Order: 1. stop rendering (callers hold no more
  /// [render]s: state leaves `attached`); 2. clear the Rust→Swift callback
  /// (synchronous, waits for an in-progress one); 3. Swift detaches and
  /// unregisters the texture (the raster thread will not call into Rust
  /// afterwards); 4. free the viewport.
  Future<void> detach() {
    return _detaching ??= _detach().whenComplete(() => _detaching = null);
  }

  Future<void> _detach() async {
    if (_state == ViewportState.creating) {
      // `_create` finishes the job when its channel call returns.
      _state = ViewportState.detached;
      _textureId = null;
      _notify();
      return;
    }
    _state = ViewportState.detached;
    _pendingSize = null;
    final id = _textureId;
    _textureId = null;
    _notify();
    if (_viewport != nullptr) {
      final vp = _viewport;
      _viewport = nullptr;
      _native!.clearFrameReadyCallback(vp);
      if (id != null) {
        await _channel.invokeMethod('unregisterTexture', {'textureId': id});
      }
      _native!.destroy(vp);
    }
  }

  bool _disposed = false;

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    unawaited(detach());
    super.dispose();
  }
}
