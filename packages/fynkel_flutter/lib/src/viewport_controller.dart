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
import 'package:fynkel/gameplay.dart';

import 'viewport_native.dart';
import 'viewport_overlay.dart';

export 'viewport_native.dart'
    show
        ViewportStatsSnapshot,
        renderBroken,
        renderNoWorld,
        renderAssetsUnavailable,
        renderDeviceLost,
        renderNoCamera,
        viewportAbiVersion;

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

/// A viewport's own camera ([FynkelViewportController.view]): where it
/// stands and the scene `Camera` component's projection. The editor's
/// Scene view looks at a running game through one, leaving the game's
/// cameras alone.
class ViewportView {
  const ViewportView({
    required this.world,
    this.orthographic = false,
    this.fovY = 1.0471975511965976,
    this.orthoHeight = 10,
    this.near = 0.1,
    this.far = 1000,
    this.manualExposure = false,
    this.exposure = 12,
    this.cullingMask = 0xFFFFFFFF,
  });

  /// The camera's world matrix, 16 floats, column-major.
  final List<double> world;
  final bool orthographic;

  /// Radians.
  final double fovY;
  final double orthoHeight, near, far;
  final bool manualExposure;

  /// EV100, when [manualExposure].
  final double exposure;
  final int cullingMask;
}

/// Operating-system thermal pressure, not a temperature measurement.
/// Unknown includes older hosts which do not report thermal state.
enum ViewportThermalState { unknown, nominal, fair, serious, critical }

ViewportThermalState _thermalState(Object? value) => switch (value) {
  0 => ViewportThermalState.nominal,
  1 => ViewportThermalState.fair,
  2 => ViewportThermalState.serious,
  3 => ViewportThermalState.critical,
  _ => ViewportThermalState.unknown,
};

/// Counters Swift keeps per texture (`copyPixelBuffer` traffic), reset on read.
class ViewportPlatformStats {
  const ViewportPlatformStats({
    this.copyCalls = 0,
    this.copyWithNewFrame = 0,
    this.copyNoFrame = 0,
    this.frameReadySignals = 0,
    this.copyQueued = 0,
    this.rebinds = 0,
    this.thermalState = ViewportThermalState.unknown,
  });
  final int copyCalls, copyWithNewFrame, copyNoFrame, frameReadySignals, copyQueued, rebinds;

  /// Current device-wide pressure at sampling time (not reset with counters).
  final ViewportThermalState thermalState;
}

/// The state of a [FynkelViewportController].
enum ViewportState { detached, creating, attached, resizing, failed }

class FynkelViewportController extends ChangeNotifier {
  FynkelViewportController({
    this.bufferCount = 4,
    this.syncMode = ViewportSyncMode.metalCompletion,
    this.acquireWaitUs = 3000,
    this.pipelined = false,
    this.frameQueue = 1,
    bool placeholders = true,
    String? assetCache,
    String? quality,
    ViewportCameraSpec? camera,
  }) // The setters below push changes to the live viewport; the constructor
    // only seeds the fields.
    // ignore: prefer_initializing_formals
    : _placeholders = placeholders,
       // ignore: prefer_initializing_formals
       _assetCache = assetCache,
       // ignore: prefer_initializing_formals
       _quality = quality,
       // ignore: prefer_initializing_formals
       _camera = camera;

  /// A game's viewport: the render worker ([pipelined]), a two-deep
  /// [frameQueue] (which also caps the frames in flight, so a GPU-bound
  /// phone drops frames before preparing them instead of queueing them) and
  /// five buffers — what `FynkelGameView` makes when it is given none — and
  /// no placeholder cubes, as the shipped game draws. An editor or tool
  /// that draws overlays between frames uses the plain constructor.
  FynkelViewportController.game({bool placeholders = false, String? assetCache, String? quality})
    : this(
        pipelined: true,
        frameQueue: 2,
        bufferCount: 5,
        placeholders: placeholders,
        assetCache: assetCache,
        quality: quality,
      );

  static const _channel = MethodChannel('fynkel_flutter/viewport');

  /// IOSurfaces in the ring (M0: 4 is the measured sweet spot).
  final int bufferCount;
  final ViewportSyncMode syncMode;

  /// Maximum time the presenter (raster thread) waits for a completion when
  /// no new image is ready. Completed images are acquired immediately (0 disables).
  final int acquireWaitUs;

  /// Draws on a native render worker: [render] extracts on the calling thread
  /// and answers the *previous* frame's result, while the frame's prepare,
  /// encode and submit overlap the next frame's simulation. For a game
  /// loop; an editor that draws overlays between frames keeps it off.
  final bool pipelined;

  /// Finished frames the compositor shows in order before it skips to the
  /// newest: 1 is a mailbox (lowest latency), 2 shows two frames that finish
  /// between rasters one after the other, so a host whose frames bunch (a
  /// long tick frame, then a short one) still shows every frame it makes.
  final int frameQueue;

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
  String? _quality;
  bool _qualityOk = true;
  ViewportCameraSpec? _camera;
  ViewportView? _view;
  bool _viewOk = true;
  Future<void>? _detaching;
  Future<void>? _creating;
  Future<void>? _resizing;

  ViewportState get state => _state;
  bool get isAttached => _state == ViewportState.attached || _state == ViewportState.resizing;

  /// Why [state] is [ViewportState.failed].
  Object? get error => _error;

  /// Flutter's id for the texture, once attached.
  int? get textureId => _textureId;

  /// Ring size in physical pixels.
  int get physicalWidth => _width;
  int get physicalHeight => _height;

  int _lightPool = 1024;
  int _lightsPerObject = 8;
  int get lightPool => _lightPool;
  int get lightsPerObject => _lightsPerObject;

  /// Sets the supported spatial forward-lighting budget. Overflow counters
  /// remain visible in frame stats. Invalid settings leave the old budget intact.
  void setLightBudget({required int pool, required int perObject}) {
    if (pool < 0 || pool > 16384 || perObject < 0 || perObject > 8 || perObject > pool) {
      throw ArgumentError('Lighting supports 0..16384 frame lights and 0..8 per object; perObject must fit pool.');
    }
    if (_viewport != nullptr && !_native!.setLightBudget(_viewport, pool, perObject)) {
      throw StateError('Native viewport refused the lighting budget.');
    }
    _lightPool = pool;
    _lightsPerObject = perObject;
  }

  /// Whether entities without a `MeshRenderer` draw as placeholder cubes.
  bool get placeholders => _placeholders;
  set placeholders(bool v) {
    _placeholders = v;
    if (_viewport != nullptr) {
      _native!.setPlaceholders(_viewport, v);
    }
  }

  /// A `fynkel cook` cache for the renderer to read meshes, textures and
  /// materials from when the world it draws did not load a scene from one.
  /// A world that did (`World.loadProject`/`loadScene`) brings its own
  /// asset store along, and the viewport draws from that — same residency,
  /// nothing to set here. Null renders placeholders only.
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

  /// The quality profile the renderer runs with, by name from the
  /// project's `quality.profiles` (shadows, shadow map size, MSAA, render
  /// scale, anisotropy). Null: the project's default (the engine defaults
  /// without a project). Switchable at any time; the next frame applies
  /// it.
  String? get quality => _quality;
  set quality(String? profile) {
    _quality = profile;
    if (_viewport != nullptr) {
      _qualityOk = _native!.setQuality(_viewport, profile);
    }
  }

  /// False when [quality] names no profile of the project being drawn (its
  /// default applies meanwhile).
  bool get qualityOk => _qualityOk;

  ViewportCameraSpec? get camera => _camera;
  set camera(ViewportCameraSpec? c) {
    _camera = c;
    if (c != null && _viewport != nullptr) {
      _applyCamera(c);
    }
  }

  /// This viewport's own camera, drawn instead of the world's active one
  /// for every world it renders (viewport ABI v7); null gives the picture
  /// back to the world. The world itself is not touched.
  ViewportView? get view => _view;
  set view(ViewportView? v) {
    _view = v;
    _viewCamera = null;
    if (_viewport != nullptr) _applyView();
  }

  /// False when the last [view] had values that cannot draw (the previous
  /// one stays).
  bool get viewOk => _viewOk;

  /// One of the rendered world's cameras this viewport looks through,
  /// instead of the one the frame picks by priority (viewport ABI 16): a
  /// game's second camera in a panel. Its pose is the frame's interpolated
  /// one. While it is not an active camera of the world, [render] answers
  /// [renderNoCamera] and the last picture stays. Setting it clears [view]
  /// and the other way round; null gives the picture back to the world.
  EntityRef? get viewCamera => _viewCamera;
  EntityRef? _viewCamera;
  set viewCamera(EntityRef? entity) {
    if (entity == _viewCamera) return;
    _viewCamera = entity;
    _view = null;
    if (_viewport != nullptr) _native!.setViewCamera(_viewport, entity);
  }

  void _applyView() {
    final v = _view;
    _viewOk = _native!.setView(
      _viewport,
      v?.world,
      orthographic: v?.orthographic ?? false,
      fovY: v?.fovY ?? 1.0471975511965976,
      orthoHeight: v?.orthoHeight ?? 10,
      near: v?.near ?? 0.1,
      far: v?.far ?? 1000,
      manualExposure: v?.manualExposure ?? false,
      exposure: v?.exposure ?? 12,
      cullingMask: v?.cullingMask ?? 0xFFFFFFFF,
    );
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
    if (_disposed || width <= 0 || height <= 0 || _detaching != null) return;
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

  Future<void> _create(int width, int height) {
    return _creating ??= _createNative(width, height).whenComplete(() => _creating = null);
  }

  Future<void> _createNative(int width, int height) async {
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
      if (pipelined) native.setPipelined(vp, true);
      if (frameQueue > 1) native.setFrameQueue(vp, frameQueue);
      native.setPlaceholders(vp, _placeholders);
      if (!native.setLightBudget(vp, _lightPool, _lightsPerObject)) {
        throw StateError('Native viewport refused the lighting budget.');
      }
      if (_assetCache != null) {
        _assetCacheOk = native.setAssetCache(vp, _assetCache);
      }
      if (_quality != null) {
        _qualityOk = native.setQuality(vp, _quality);
      }
      final cam = _camera;
      if (cam != null) {
        _applyCamera(cam);
      }
      if (_view != null) _applyView();
      if (_viewCamera != null) native.setViewCamera(vp, _viewCamera);
      final id = await _channel.invokeMethod<int>('registerTexture', {
        'ctx': vp.address,
        'acquireFn': native.acquireFnAddress,
        'setCallbackFn': native.setCallbackFnAddress,
        'iosurfaces': native.iosurfaceAddresses(vp),
      });
      if (id == null) throw StateError('Texture registration returned no texture id.');
      _textureId = id;
      // Teardown waits for this registration before unregistering/freeing.
      // Do not publish an attachment after detach/dispose has started.
      if (_state != ViewportState.creating || _disposed) return;
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

  Future<void> _drainResizes() {
    return _resizing ??= _resizeNative().whenComplete(() => _resizing = null);
  }

  Future<void> _resizeNative() async {
    _state = ViewportState.resizing;
    (int, int)? attempted;
    try {
      while (_pendingSize != null && _detaching == null) {
        final (w, h) = _pendingSize!;
        attempted = (w, h);
        _pendingSize = null;
        if (w == _width && h == _height) continue;
        if (!_native!.resize(_viewport, w, h)) {
          throw StateError(_native!.error(_viewport) ?? 'Viewport resize to ${w}x$h failed; retry the viewport.');
        }
        await _channel.invokeMethod('rebindTexture', {
          'textureId': _textureId,
          'iosurfaces': _native!.iosurfaceAddresses(_viewport),
        });
        _width = w;
        _height = h;
        _resizes++;
      }
    } catch (error) {
      _error = error;
      if (_state == ViewportState.resizing) {
        _pendingSize ??= attempted;
        _state = ViewportState.failed;
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
  /// Returns the slot index, -1 when the ring was full (frame dropped),
  /// [renderNoWorld], or [renderBroken] — a panic inside the viewport
  /// (viewport ABI 11): the controller is then [ViewportState.failed] with a
  /// [ViewportBrokenError] and draws nothing more until [detach]; null when
  /// the viewport is not attached or is mid-resize (the simulation still
  /// advanced, so no time is lost).
  int? render(World world) => renderHandle(world.native.handleAddress);

  /// [render] for a world the host holds only as its native `FynkelWorld*`
  /// address — the editor's edit-preview world
  /// (`ScenePreview.worldAddress`, authoring ABI v3), which no Dart [World]
  /// wraps because nothing steps it from Dart. 0 renders nothing
  /// ([renderNoWorld]).
  /// Export-only option: waits up to ten seconds for GPU readiness; -4 on failure.
  int? renderHandle(int worldAddress, {bool waitForAssets = false}) {
    if (_state != ViewportState.attached) return null;
    final slot = _native!.render(_viewport, worldAddress, waitForAssets: waitForAssets);
    if (slot == renderBroken || slot == renderDeviceLost) {
      _error = slot == renderDeviceLost
          ? StateError('GPU device lost. Recover the viewport to create a new device and texture.')
          : ViewportBrokenError(_native!.error(_viewport) ?? 'no message');
      _state = ViewportState.failed;
      _notify();
    }
    return slot;
  }

  /// Recreate a failed viewport after device loss or a renderer failure.
  /// Unregisters the old texture before freeing native resources. Camera,
  /// view, quality, cache and placeholder settings are reapplied; the world
  /// remains owned by its host and is never recreated here.
  Future<void> recover() async {
    if (_disposed || _state != ViewportState.failed) return;
    final width = _pendingSize?.$1 ?? _width;
    final height = _pendingSize?.$2 ?? _height;
    try {
      await detach();
    } catch (error) {
      _pendingSize = (width, height);
      _error = error;
      _state = ViewportState.failed;
      _notify();
      return;
    }
    if (_disposed) return;
    _error = null;
    await _create(width, height);
  }

  /// Whether another frame is needed to finish loading visible assets.
  bool get assetsPending => _state == ViewportState.attached && _native!.assetsPending(_viewport);

  /// Panics inside the native viewport, on purpose: the next [render]
  /// finds it broken. For tests of a host's failure path.
  void debugPanic() {
    if (_viewport != nullptr) _native!.debugPanic(_viewport);
  }

  /// Queues world-space line segments the next [render] draws on top of
  /// the scene — ten floats each, `ax ay az bx by bz r g b a` (metres;
  /// linear RGBA), flat in [lines] — as overlay lines 1.5 px wide that fade
  /// behind the scene ([overlay] sets widths, triangles and a grid). The
  /// queue empties with every render: queue, then render, every frame.
  /// Returns the segments queued, or null when the viewport is not
  /// attached.
  int? debugLines(Float32List lines) {
    if (_state != ViewportState.attached) return null;
    return _native!.debugLines(_viewport, lines);
  }

  /// Queues [overlay] for the next [render] (ADR-0019): its lines,
  /// triangles and ground grid draw after post, at the target's
  /// resolution, never exposed or tone mapped — the editor's grid, gizmos
  /// and handles — its selection outline (viewport ABI v8) under them, and
  /// its extra worlds (v9: `ViewportOverlay.extraWorld`) drawn into the
  /// frame with the rendered world, each read now — queue right before
  /// [render]. The queue empties with every render. Returns the lines +
  /// triangles queued plus the outline's entities plus the extra worlds
  /// (an address 0 is skipped), or null when the viewport is not attached.
  /// An outline with a non-finite colour or width, or extra worlds with a
  /// non-finite transform, are refused and add nothing.
  int? overlay(ViewportOverlay overlay) {
    if (_state != ViewportState.attached) return null;
    var queued = _native!.overlay(_viewport, overlay.lines, overlay.triangles, overlay.grid?.pack());
    final outline = overlay.selectionOutline;
    if (outline != null) {
      final n = _native!.outline(_viewport, outline.selected, outline.children, outline.packStyle());
      if (n > 0) queued += n;
    }
    final extras = overlay.extraWorlds;
    if (extras.isNotEmpty) {
      final n = _native!.extraWorlds(
        _viewport,
        [for (final w in extras) w.worldAddress],
        overlay.packExtraTransforms(),
        [for (final w in extras) w.flags],
      );
      if (n > 0) queued += n;
    }
    return queued;
  }

  /// The pixels of the frame a [render] returned as [slot]: tightly packed
  /// RGBA8, top row first, [physicalWidth] × [physicalHeight] × 4 bytes —
  /// the editor's Save Frame and its pixel gate against `fynkel render`.
  /// Blocks until the GPU is idle, so never per frame. Null when the
  /// viewport is not attached or the slot is not one of its ring's.
  Uint8List? readFrame(int slot) {
    if (_state != ViewportState.attached) return null;
    return _native!.readFrame(_viewport, slot, _width, _height);
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
      copyQueued: m['copyQueued'] as int? ?? 0,
      rebinds: m['rebinds'] as int? ?? 0,
      thermalState: _thermalState(m['thermalState']),
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
    final pending = _detaching;
    if (pending != null) return pending;
    // Publish the guard before notifying listeners: a listener may request
    // another size synchronously when it observes the detached state.
    final completion = Completer<void>();
    _detaching = completion.future;
    unawaited(
      _detach().then(
        (_) {
          _detaching = null;
          completion.complete();
        },
        onError: (Object error, StackTrace stack) {
          _detaching = null;
          completion.completeError(error, stack);
        },
      ),
    );
    return completion.future;
  }

  Future<void> _detach() async {
    _state = ViewportState.detached;
    _notify();
    // Registration/rebinding can still hand surfaces to the presenter.
    // Finish those operations before unregistering and freeing the ring.
    await _creating;
    await _resizing;
    _state = ViewportState.detached;
    _pendingSize = null;
    final id = _textureId;
    _textureId = null;
    _notify();
    if (_viewport != nullptr) {
      final vp = _viewport;
      _native!.clearFrameReadyCallback(vp);
      if (id != null) {
        try {
          await _channel.invokeMethod('unregisterTexture', {'textureId': id});
        } catch (error) {
          // The raster thread may still use this ring. Retain its ownership
          // and texture identity so recovery can retry the ordered teardown.
          _textureId = id;
          _error = error;
          _state = ViewportState.failed;
          _notify();
          rethrow;
        }
      }
      _viewport = nullptr;
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

/// [FynkelViewportController.error] after a panic inside the native
/// viewport (viewport ABI 11): the Rust side caught it at the boundary,
/// the app lives on, and this viewport draws nothing more. [detach] frees
/// it; the next size request makes a new one.
class ViewportBrokenError extends Error {
  ViewportBrokenError(this.panic);

  /// The panic's message.
  final String panic;

  @override
  String toString() => 'The viewport stopped: the renderer panicked.\n$panic';
}
