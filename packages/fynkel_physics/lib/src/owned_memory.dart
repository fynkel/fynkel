import 'dart:ffi';

import 'package:ffi/ffi.dart';

/// Native memory that belongs to [owner]: freed by [free] (a buffer that
/// grows), or else when the owner is garbage collected or its isolate
/// ends, as on a hot restart, which disposes nothing.
///
/// A disposed owner keeps its scratch until then, so a call made after
/// `dispose` writes into memory it still owns before it finds out and
/// throws.
final class OwnedMemory implements Allocator {
  OwnedMemory(this.owner);

  static final _finalizer = NativeFinalizer(malloc.nativeFree);

  final Finalizable owner;

  // The detach token of each allocation, by address.
  final _tokens = <int, Object>{};

  @override
  Pointer<T> allocate<T extends NativeType>(int byteCount, {int? alignment}) {
    final pointer = malloc.allocate<T>(byteCount, alignment: alignment);
    final token = Object();
    _tokens[pointer.address] = token;
    _finalizer.attach(owner, pointer.cast(), detach: token, externalSize: byteCount);
    return pointer;
  }

  @override
  void free(Pointer<NativeType> pointer) {
    final token = _tokens.remove(pointer.address);
    if (token != null) _finalizer.detach(token);
    malloc.free(pointer);
  }
}
