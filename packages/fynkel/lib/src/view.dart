import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'native.dart';
import 'png.dart';
import 'world.dart';

/// What a view did since it was made (`fy_view_stats`).
final class ViewStats {
  /// All zeros unless given: a start to subtract from, and for tests.
  const ViewStats({
    this.posted = 0,
    this.rendered = 0,
    this.coalesced = 0,
    this.skipped = 0,
    this.presented = 0,
    this.dropped = 0,
    this.textureBytes = 0,
    this.meshBytes = 0,
    this.skyBytes = 0,
    this.busy = Duration.zero,
    this.gpu = const GpuTimes(),
  });

  /// Scenes handed to the render worker by [WorldView.frame].
  final int posted;

  /// Scenes drawn.
  final int rendered;

  /// Scenes replaced by a newer one before they were drawn.
  final int coalesced;

  /// Scenes not drawn because the target had nowhere to draw.
  final int skipped;

  /// On screen: new frames taken by Flutter. 0 for a [HeadlessView].
  final int presented;

  /// On screen: frames finished but never shown, because newer ones were
  /// waiting.
  final int dropped;

  /// GPU memory the world's textures hold, in bytes: every mipmap level, as
  /// stored. Shared by all the world's views, so not added up across them.
  /// As of the last frame drawn.
  final int textureBytes;

  /// GPU memory the world's meshes hold, in bytes. Shared by all the
  /// world's views, like [textureBytes]. As of the last frame drawn.
  final int meshBytes;

  /// GPU memory the sky's light holds for this view, in bytes: 0 without
  /// a sky or with a [ColorSky]. As of the last frame drawn.
  final int skyBytes;

  /// Time the render thread spent on scenes, from taking one to being done
  /// with it; the rest of the time it waited for the next. On Android this
  /// includes waiting for the GPU to finish each frame.
  final Duration busy;

  /// The GPU's time for each part of the frames timed while
  /// [WorldView.timeGpu] was on.
  final GpuTimes gpu;

  /// The counts since [earlier]. The memory is the newer one's.
  ViewStats operator -(ViewStats earlier) => ViewStats(
    posted: posted - earlier.posted,
    rendered: rendered - earlier.rendered,
    coalesced: coalesced - earlier.coalesced,
    skipped: skipped - earlier.skipped,
    presented: presented - earlier.presented,
    dropped: dropped - earlier.dropped,
    textureBytes: textureBytes,
    meshBytes: meshBytes,
    skyBytes: skyBytes,
    busy: busy - earlier.busy,
    gpu: gpu - earlier.gpu,
  );

  @override
  String toString() =>
      'posted=$posted rendered=$rendered coalesced=$coalesced skipped=$skipped presented=$presented dropped=$dropped '
      'textureBytes=$textureBytes meshBytes=$meshBytes skyBytes=$skyBytes busyMicros=${busy.inMicroseconds} gpu=($gpu)';
}

/// The GPU's time for each part of a view's frames, added up over
/// [frames] frames timed while [WorldView.timeGpu] was on: for finding
/// what a frame's GPU time goes on. Part of [ViewStats]; subtract an
/// earlier one for a stretch, and divide by [frames] for a frame's.
///
/// On a phone's GPU each part's work may start before the one before it
/// ends, so the parts can add up to a little more than the frame took.
final class GpuTimes {
  /// None unless given: a start to subtract from, and for tests.
  const GpuTimes({
    this.frames = 0,
    this.shadows = Duration.zero,
    this.skyLight = Duration.zero,
    this.ambientOcclusion = Duration.zero,
    this.scene = Duration.zero,
    this.bloom = Duration.zero,
    this.finish = Duration.zero,
    this.effects = Duration.zero,
  });

  /// The frames timed.
  final int frames;

  /// The sun's and the spot lights' shadow maps.
  final Duration shadows;

  /// A new sky's light, worked out when the sky changes.
  final Duration skyLight;

  /// [AmbientOcclusion]: the solid scene's depth, the occlusion and its
  /// blur.
  final Duration ambientOcclusion;

  /// The scene itself: solid, sky, see-through, particles and lines.
  final Duration scene;

  /// [Bloom]'s chain of smaller and larger images.
  final Duration bloom;

  /// The scene image onto the view: tone mapping, the glow added. Not
  /// drawn at [Quality.low], which draws straight onto the view.
  final Duration finish;

  /// The screen effects.
  final Duration effects;

  /// Every part's time added up.
  Duration get total => shadows + skyLight + ambientOcclusion + scene + bloom + finish + effects;

  /// These and [more] together: for adding up stretches.
  GpuTimes operator +(GpuTimes more) => GpuTimes(
    frames: frames + more.frames,
    shadows: shadows + more.shadows,
    skyLight: skyLight + more.skyLight,
    ambientOcclusion: ambientOcclusion + more.ambientOcclusion,
    scene: scene + more.scene,
    bloom: bloom + more.bloom,
    finish: finish + more.finish,
    effects: effects + more.effects,
  );

  /// The frames and times since [earlier].
  GpuTimes operator -(GpuTimes earlier) => GpuTimes(
    frames: frames - earlier.frames,
    shadows: shadows - earlier.shadows,
    skyLight: skyLight - earlier.skyLight,
    ambientOcclusion: ambientOcclusion - earlier.ambientOcclusion,
    scene: scene - earlier.scene,
    bloom: bloom - earlier.bloom,
    finish: finish - earlier.finish,
    effects: effects - earlier.effects,
  );

  @override
  bool operator ==(Object other) =>
      other is GpuTimes &&
      other.frames == frames &&
      other.shadows == shadows &&
      other.skyLight == skyLight &&
      other.ambientOcclusion == ambientOcclusion &&
      other.scene == scene &&
      other.bloom == bloom &&
      other.finish == finish &&
      other.effects == effects;

  @override
  int get hashCode => Object.hash(frames, shadows, skyLight, ambientOcclusion, scene, bloom, finish, effects);

  @override
  String toString() =>
      'frames=$frames shadows=${shadows.inMicroseconds} skyLight=${skyLight.inMicroseconds} '
      'ambientOcclusion=${ambientOcclusion.inMicroseconds} scene=${scene.inMicroseconds} '
      'bloom=${bloom.inMicroseconds} finish=${finish.inMicroseconds} effects=${effects.inMicroseconds}';
}

/// A view of a [World]: a render worker on its own thread that draws the
/// world each time [frame] is called. Every view shares one GPU device, and
/// a world's views share its meshes and textures, so a second view of a
/// world costs only its own frames. [HeadlessView] reads
/// the pixels back; [AppleView] and [AndroidSurfaceView] draw for Flutter to
/// show.
abstract base class WorldView implements Finalizable {
  WorldView._(this._world, this._width, this._height, Pointer<FyView> pointer, {required bool finalize})
    : _pointer = pointer {
    if (finalize) _finalizer.attach(this, pointer.cast(), detach: this);
    _world.viewStarted(onScreen: this is! HeadlessView);
  }

  static final _finalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FyView>)>>(fy_view_free).cast(),
  );

  static Pointer<FyView> _create(int Function(Pointer<Pointer<FyView>>) create) {
    final out = malloc<Pointer<FyView>>();
    try {
      check(create(out));
      return out.value;
    } finally {
      malloc.free(out);
    }
  }

  final World _world;
  int _width, _height;
  Pointer<FyView> _pointer;

  /// The width the frames from the next [frame] on are drawn at, in pixels.
  int get width => _width;

  /// The height the frames from the next [frame] on are drawn at, in
  /// pixels.
  int get height => _height;

  /// Whether [dispose] was called; then [frame], [resize], [stats] and
  /// the rest of its calls throw a StateError.
  bool get isDisposed => _pointer == nullptr;

  /// The native view, for the platform plugins' calls. A plugin must stop
  /// using it before [dispose].
  int get address => _live.address;

  Pointer<FyView> get _live {
    if (_pointer == nullptr) throw StateError('this $runtimeType was disposed');
    return _pointer;
  }

  /// The camera the view draws from: its own, or the world's main one
  /// ([World.camera]) when null. Read at each [frame]. While its object
  /// isn't a live, shown object of the world (not added yet, destroyed,
  /// inactive), the view draws black, not the main camera's picture.
  Camera? camera;
  int _sentCamera = 0;

  /// Copies the world and hands it to the render worker, without waiting
  /// for the GPU. Returns the frame's number (1, 2, …).
  int frame() {
    final live = _live;
    final own = this.camera;
    final camera = own == null ? 0 : cameraObjectId(own, _world);
    if (camera != _sentCamera) {
      check(fy_view_camera_set(live, camera));
      _sentCamera = camera;
    }
    _world.sendTransforms();
    check(fy_view_frame(live, scratch.id));
    return scratch.id.value;
  }

  /// Draws the frames from the next [frame] on at [width] × [height]
  /// pixels. Frames already on their way finish at the old size and are
  /// never shown.
  void resize(int width, int height) {
    if (width == _width && height == _height) return;
    check(fy_view_resize(_live, width, height));
    _width = width;
    _height = height;
  }

  /// Whether every view's frames are timed on the GPU, each part of a frame
  /// on its own, into [ViewStats.gpu]: for measuring, off by default.
  /// Timing costs a little GPU time, and on Apple GPUs keeps some driver
  /// memory in use. Where the GPU can't time its work nothing is timed.
  static bool get timeGpu => _timeGpu;
  static set timeGpu(bool on) {
    _timeGpu = on;
    fy_gpu_timing_set(on);
  }

  static var _timeGpu = false;

  /// What the view did since it was made, and the GPU memory it holds
  /// now. Subtract an earlier one for the counts in between.
  ViewStats get stats {
    final s = scratch.viewStats;
    check(fy_view_stats(_live, s));
    final r = s.ref;
    return ViewStats(
      posted: r.posted,
      rendered: r.rendered,
      coalesced: r.coalesced,
      skipped: r.skipped,
      presented: r.presented,
      dropped: r.dropped,
      textureBytes: r.texture_bytes,
      meshBytes: r.mesh_bytes,
      skyBytes: r.sky_bytes,
      busy: Duration(microseconds: r.busy_micros),
      gpu: r.gpu_frames == 0
          ? const GpuTimes()
          : GpuTimes(
              frames: r.gpu_frames,
              shadows: Duration(microseconds: r.gpu_shadows_micros),
              skyLight: Duration(microseconds: r.gpu_sky_light_micros),
              ambientOcclusion: Duration(microseconds: r.gpu_ambient_occlusion_micros),
              scene: Duration(microseconds: r.gpu_scene_micros),
              bloom: Duration(microseconds: r.gpu_bloom_micros),
              finish: Duration(microseconds: r.gpu_finish_micros),
              effects: Duration(microseconds: r.gpu_effects_micros),
            ),
    );
  }

  /// Stops the render worker (after the frame it's drawing) and frees the
  /// view. Calling it again does nothing.
  void dispose() {
    if (_pointer == nullptr) return;
    _finalizer.detach(this);
    fy_view_free(_pointer);
    _pointer = nullptr;
  }
}

/// A view that draws a [World] offscreen and reads the pixels back: for
/// tests and tools, not for showing on screen (package:fynkel_flutter's
/// `FynkelView` does that).
final class HeadlessView extends WorldView {
  /// A view of [world] drawing [width] × [height] pixel frames, with its
  /// own render worker. Throws an [ArgumentError] for a
  /// size of 0, and a [FynkelException] when there's no GPU or the worker
  /// won't start. Dispose of it when done.
  HeadlessView(World world, {required int width, required int height})
    : super._(
        world,
        width,
        height,
        WorldView._create((out) => fy_view_new_headless(world.pointer, width, height, out)),
        finalize: true,
      ) {
    _allocatePixels();
  }

  static final _pixelsFinalizer = NativeFinalizer(malloc.nativeFree);

  Pointer<Uint8> _pixels = nullptr;

  int get _length => width * height * 4;

  void _allocatePixels() {
    _pixels = malloc<Uint8>(_length);
    _pixelsFinalizer.attach(this, _pixels.cast(), detach: this);
  }

  void _freePixels() {
    _pixelsFinalizer.detach(this);
    malloc.free(_pixels);
    _pixels = nullptr;
  }

  /// Waits up to [timeout] for frame [number] (or a later one) to finish
  /// and returns its number and pixels: RGBA8 in sRGB, rows top to bottom,
  /// at the size the frame was drawn.
  ({int number, Uint8List rgba}) read(int number, {Duration timeout = const Duration(seconds: 10)}) {
    check(fy_view_read(_live, number, timeout.inMilliseconds, _pixels, _length, scratch.id));
    return (number: scratch.id.value, rgba: Uint8List.fromList(_pixels.asTypedList(_length)));
  }

  /// Draws the world as it is now and gives the picture as a PNG file's
  /// bytes, to look at what a test sees:
  /// `File('shot.png').writeAsBytesSync(view.png())`.
  Uint8List png({Duration timeout = const Duration(seconds: 10)}) =>
      encodePng(width, height, read(frame(), timeout: timeout).rgba);

  @override
  void resize(int width, int height) {
    if (width == this.width && height == this.height) return;
    super.resize(width, height);
    _freePixels();
    _allocatePixels();
  }

  @override
  void dispose() {
    if (isDisposed) return;
    super.dispose();
    _freePixels();
  }
}

/// A view that draws a [World] into IOSurfaces for Flutter to show on
/// macOS and iOS. package:fynkel_flutter's `FynkelView` makes one and
/// hands [address] and the function addresses to its Swift plugin; a
/// game never uses it directly. It must be disposed: an Apple view left
/// for the garbage collector stays allocated, with its render worker.
final class AppleView extends WorldView {
  /// Throws a [FynkelException] with [FynkelStatus.unsupported] off Apple.
  AppleView(World world, {required int width, required int height})
    : super._(
        world,
        width,
        height,
        WorldView._create((out) => fy_view_new_apple(world.pointer, width, height, out)),
        // Flutter's raster thread may be using it, so it's never freed by
        // the garbage collector, only by [dispose] after the plugin let go.
        finalize: false,
      );

  /// `fy_apple_acquire`, for Flutter's raster thread.
  static int get acquireAddress =>
      Native.addressOf<NativeFunction<Uint32 Function(Pointer<FyView>, Pointer<Pointer<Void>>)>>(fy_apple_acquire)
          .address;

  /// `fy_view_free`, for the plugin to free the views of an isolate that
  /// is gone (a hot restart disposes nothing).
  static int get freeAddress => Native.addressOf<NativeFunction<Void Function(Pointer<FyView>)>>(fy_view_free).address;

  /// `fy_apple_set_frame_ready`, for the plugin to learn of new frames.
  static int get setFrameReadyAddress =>
      Native.addressOf<
            NativeFunction<
              Int32 Function(Pointer<FyView>, Pointer<NativeFunction<Void Function(Pointer<Void>)>>, Pointer<Void>)
            >
          >(fy_apple_set_frame_ready)
          .address;
}

/// A view that draws a [World] straight to a `CAMetalLayer` the Flutter
/// plugin puts under Flutter's view (iOS). package:fynkel_flutter's
/// `FynkelView` makes one and hands [address] to its Swift plugin, which
/// gives it a layer (and with it the size) through [attachAddress]; [resize]
/// changes nothing here. A game never uses it directly. It must be
/// disposed: it's never freed by the garbage collector.
final class AppleLayerView extends WorldView {
  /// Throws a [FynkelException] with [FynkelStatus.unsupported] off Apple.
  AppleLayerView(World world, {required int width, required int height})
    : super._(
        world,
        width,
        height,
        WorldView._create((out) => fy_view_new_apple_layer(world.pointer, width, height, out)),
        finalize: false,
      );

  /// `fy_apple_layer_attach`, for the plugin to give the view its layer.
  static int get attachAddress =>
      Native.addressOf<NativeFunction<Int32 Function(Pointer<FyView>, Pointer<Void>, Uint32, Uint32)>>(
        fy_apple_layer_attach,
      ).address;

  /// `fy_apple_layer_detach`, for the plugin to take it away.
  static int get detachAddress =>
      Native.addressOf<NativeFunction<Int32 Function(Pointer<FyView>)>>(fy_apple_layer_detach).address;
}

/// A view that draws a [World] into an Android `Surface` for Flutter to
/// show. package:fynkel_flutter's `FynkelView` makes one and hands
/// [address] to its Kotlin plugin, which gives it a surface (and with it
/// the size) through JNI; [resize] changes nothing here. A game never uses
/// it directly. It must be disposed: an Android view left for the garbage
/// collector stays allocated, with its render worker.
final class AndroidSurfaceView extends WorldView {
  /// Throws a [FynkelException] with [FynkelStatus.unsupported] off
  /// Android.
  AndroidSurfaceView(World world, {required int width, required int height})
    : super._(
        world,
        width,
        height,
        WorldView._create((out) => fy_view_new_android(world.pointer, width, height, out)),
        // The plugin may be handing it a surface, so it's never freed by
        // the garbage collector, only by [dispose] after the plugin let go.
        finalize: false,
      );
}
