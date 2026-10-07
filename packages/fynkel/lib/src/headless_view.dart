import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'native.dart';
import 'world.dart';

/// A view that draws a [World] offscreen and reads the pixels back: for
/// tests and tools, not for showing on screen (package:fynkel_flutter's
/// `FynkelView` does that). It has its own render worker and GPU device.
final class HeadlessView implements Finalizable {
  HeadlessView(World world, {required this.width, required this.height}) {
    final out = malloc<Pointer<FyView>>();
    try {
      check(fy_view_new_headless(world.pointer, width, height, out));
      _pointer = out.value;
    } finally {
      malloc.free(out);
    }
    _viewFinalizer.attach(this, _pointer.cast(), detach: this);
    _pixels = malloc<Uint8>(_length);
    _pixelsFinalizer.attach(this, _pixels.cast(), detach: this);
  }

  static final _viewFinalizer = NativeFinalizer(
    Native.addressOf<NativeFunction<Void Function(Pointer<FyView>)>>(fy_view_free).cast(),
  );
  static final _pixelsFinalizer = NativeFinalizer(malloc.nativeFree);

  final int width;
  final int height;
  Pointer<FyView> _pointer = nullptr;
  Pointer<Uint8> _pixels = nullptr;

  int get _length => width * height * 4;

  Pointer<FyView> get _live {
    if (_pointer == nullptr) throw StateError('this HeadlessView was disposed');
    return _pointer;
  }

  /// Copies the world and hands it to the render worker, without waiting
  /// for the GPU. Returns the frame's number (1, 2, …).
  int frame() {
    check(fy_view_frame(_live, scratch.id));
    return scratch.id.value;
  }

  /// Waits up to [timeout] for frame [number] (or a later one) to finish
  /// and returns its number and pixels: RGBA8 in sRGB, rows top to bottom.
  ({int number, Uint8List rgba}) read(int number, {Duration timeout = const Duration(seconds: 10)}) {
    check(fy_view_read(_live, number, timeout.inMilliseconds, _pixels, _length, scratch.id));
    return (number: scratch.id.value, rgba: Uint8List.fromList(_pixels.asTypedList(_length)));
  }

  /// Stops the render worker (after the frame it's drawing) and frees the
  /// view.
  void dispose() {
    if (_pointer == nullptr) return;
    _viewFinalizer.detach(this);
    _pixelsFinalizer.detach(this);
    fy_view_free(_pointer);
    malloc.free(_pixels);
    _pointer = nullptr;
    _pixels = nullptr;
  }
}
