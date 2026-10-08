/// Why a call into the engine failed. The same codes, in the same order,
/// as `FyStatus` in crates/fynkel_ffi/include/fynkel.h.
enum FynkelStatus {
  ok,

  /// No object has this id: it was despawned, or never spawned.
  noObject,

  /// The object doesn't have that component.
  noComponent,
  noMesh,
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
  noTexture,
}

/// A call into the engine failed. [message] is the engine's own
/// explanation, such as "object#5 does not exist (despawned, or never
/// spawned)".
final class FynkelException implements Exception {
  FynkelException(this.status, this.message);

  final FynkelStatus status;
  final String message;

  @override
  String toString() => 'FynkelException(${status.name}): $message';
}
