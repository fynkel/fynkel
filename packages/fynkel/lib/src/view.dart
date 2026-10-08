import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'native.dart';
import 'world.dart';

/// What a view did since it was made (`fy_view_stats`).
final class ViewStats {
  const ViewStats({
    this.posted = 0,
    this.rendered = 0,
    this.coalesced = 0,
    this.skipped = 0,
    this.presented = 0,
    this.dropped = 0,
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

  /// The counts since [earlier].
  ViewStats operator -(ViewStats earlier) => ViewStats(
    posted: posted - earlier.posted,
    rendered: rendered - earlier.rendered,
    coalesced: coalesced - earlier.coalesced,
    skipped: skipped - earlier.skipped,
    presented: presented - earlier.presented,
    dropped: dropped - earlier.dropped,
  );

  @override
  String toString() =>
      'posted=$posted rendered=$rendered coalesced=$coalesced skipped=$skipped presented=$presented dropped=$dropped';
}

/// A view of a [World]: a render worker on its own thread and GPU device
/// that draws the world each time [frame] is called. [HeadlessView] reads
/// the pixels back; [AppleView] and [AndroidSurfaceView] draw for Flutter to
/// show.
abstract base class WorldView implements Finalizable {
  WorldView._(this._width, this._height, Pointer<FyView> pointer, {required bool finalize}) : _pointer = pointer {
    if (finalize) _finalizer.attach(this, pointer.cast(), detach: this);
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

  int _width, _height;
  Pointer<FyView> _pointer;

  /// The size the frames from the next [frame] on are drawn at, in pixels.
  int get width => _width;
  int get height => _height;

  bool get isDisposed => _pointer == nullptr;

  /// The native view, for the platform plugins' calls. A plugin must stop
  /// using it before [dispose].
  int get address => _live.address;

  Pointer<FyView> get _live {
    if (_pointer == nullptr) throw StateError('this $runtimeType was disposed');
    return _pointer;
  }

  /// Copies the world and hands it to the render worker, without waiting
  /// for the GPU. Returns the frame's number (1, 2, …).
  int frame() {
    check(fy_view_frame(_live, scratch.id));
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
  HeadlessView(World world, {required int width, required int height})
    : super._(
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
        width,
        height,
        WorldView._create((out) => fy_view_new_android(world.pointer, width, height, out)),
        // The plugin may be handing it a surface, so it's never freed by
        // the garbage collector, only by [dispose] after the plugin let go.
        finalize: false,
      );
}
