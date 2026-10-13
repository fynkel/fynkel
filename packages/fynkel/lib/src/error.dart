/// Why a call into the engine failed. The same codes, in the same order,
/// as `FyStatus` in crates/fynkel_ffi/include/fynkel.h.
enum FynkelStatus {
  /// It worked. A [FynkelException] never has it.
  ok,

  /// No object has this id: it was destroyed, or never added.
  noObject,

  /// The object doesn't have that component.
  noComponent,

  /// The world has no such mesh: it was disposed, or never added.
  noMesh,

  /// The world has no such material: it was disposed, or never added.
  noMaterial,

  /// A mesh or material can't be removed while an object uses it, or a
  /// texture while a material does.
  inUse,

  /// A value was out of range, not finite, or not one of the choices.
  invalidValue,

  /// A pointer was null. package:fynkel never passes one, so this is a bug
  /// in it.
  nullPointer,

  /// No GPU could be had.
  noGpu,

  /// A frame didn't finish in time.
  timeout,

  /// The render worker stopped after a panic.
  workerFailed,

  /// A buffer had the wrong size.
  wrongSize,

  /// The engine panicked. Nothing was left half-done in Dart; the message
  /// says what happened.
  panic,

  /// An earlier panic happened while the world was being changed, so it
  /// can't be trusted any more.
  poisoned,

  /// Not available on this platform, or not for this kind of view.
  unsupported,

  /// The world has no such texture: it was disposed, or never added.
  noTexture,

  /// The view's GPU device was lost, or the GPU reported an error, and the
  /// view stopped drawing. A new view gets a new device and uploads
  /// everything again; `FynkelView` makes one by itself.
  deviceLost,
}

/// The engine itself failed: no GPU, a frame that didn't finish, a lost
/// device, a panic. A game's own mistakes throw Dart's errors instead: an
/// [ArgumentError] for a bad value, a [StateError] for a call at the wrong
/// time or on something gone. [message] is the engine's own explanation.
final class FynkelException implements Exception {
  /// The failure [status], with the engine's [message].
  FynkelException(this.status, this.message);

  /// What kind of failure it was.
  final FynkelStatus status;

  /// The engine's own explanation of what happened.
  final String message;

  @override
  String toString() => 'FynkelException(${status.name}): $message';
}
