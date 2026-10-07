#if os(iOS)
import Flutter
#else
import FlutterMacOS
#endif
import CoreVideo
import Foundation
import IOSurface

/// The C functions of the engine the texture calls. Dart passes their
/// addresses (`AppleView.acquireAddress`, `setFrameReadyAddress`), so this
/// plugin doesn't link the engine library itself.
typealias FyAcquire = @convention(c) (UnsafeRawPointer?, UnsafeMutablePointer<UnsafeMutableRawPointer?>?) -> UInt32
typealias FyFrameReady = @convention(c) (UnsafeMutableRawPointer?) -> Void
typealias FySetFrameReady = @convention(c) (UnsafeRawPointer?, FyFrameReady?, UnsafeMutableRawPointer?) -> Int32

/// `fy_apple_acquire`'s flags.
private let shownFlag: UInt32 = 1
private let moreFlag: UInt32 = 2

/// One engine view on screen: a `FlutterTexture` whose pixel buffers are
/// the IOSurfaces the engine draws into. Nothing is copied: Flutter samples
/// the surface the GPU finished.
final class FynkelTexture: NSObject, FlutterTexture {
  /// The engine view (`FyView*`); valid until `detach()` returns.
  private let view: UnsafeRawPointer
  private let acquire: FyAcquire
  private weak var registry: FlutterTextureRegistry?
  var textureId: Int64 = 0

  /// Guards everything below. The raster thread holds it for the whole
  /// `acquire` call, so once `detach()` has it, the engine view is never
  /// touched again and Dart may free it.
  private let lock = NSLock()
  private var detached = false

  /// A pixel buffer for each surface seen so far. The engine has 5
  /// surfaces and replaces all of them on a resize, so a surface missing
  /// from a full cache means the old ones are gone: start again. Each
  /// entry keeps its surface alive, so a cached address can't be reused
  /// by a new surface.
  private var buffers: [(surface: UnsafeMutableRawPointer, buffer: CVPixelBuffer)] = []
  private static let slots = 5

  init(view: UnsafeRawPointer, acquire: FyAcquire, registry: FlutterTextureRegistry) {
    self.view = view
    self.acquire = acquire
    self.registry = registry
    buffers.reserveCapacity(Self.slots)
  }

  /// The engine calls this on a Metal thread each time a frame finishes.
  /// `textureFrameAvailable` is safe to call from any thread.
  static let frameReady: FyFrameReady = { userdata in
    guard let userdata else { return }
    let texture = Unmanaged<FynkelTexture>.fromOpaque(userdata).takeUnretainedValue()
    texture.registry?.textureFrameAvailable(texture.textureId)
  }

  /// Stops all calls into the engine view. Waits for one in progress.
  func detach() {
    lock.lock()
    detached = true
    buffers.removeAll()
    lock.unlock()
  }

  // Flutter's raster thread, once per frame that shows this texture.
  func copyPixelBuffer() -> Unmanaged<CVPixelBuffer>? {
    lock.lock()
    defer { lock.unlock() }
    if detached { return nil }
    var raw: UnsafeMutableRawPointer?
    let flags = acquire(view, &raw)
    guard flags & shownFlag != 0, let raw else { return nil }
    // Retained once for us by the engine; released at the end of scope.
    let surface = Unmanaged<IOSurfaceRef>.fromOpaque(raw).takeRetainedValue()
    if flags & moreFlag != 0 {
      // Another finished frame waits behind this one: ask for one more
      // frame. Not from inside Flutter's own call.
      let id = textureId
      DispatchQueue.global(qos: .userInteractive).async { [weak registry] in
        registry?.textureFrameAvailable(id)
      }
    }
    if let cached = buffers.first(where: { $0.surface == raw }) {
      return Unmanaged.passRetained(cached.buffer)
    }
    var created: Unmanaged<CVPixelBuffer>?
    let attributes = [kCVPixelBufferMetalCompatibilityKey: true] as CFDictionary
    let status = CVPixelBufferCreateWithIOSurface(nil, surface, attributes, &created)
    guard status == kCVReturnSuccess, let buffer = created?.takeRetainedValue() else {
      NSLog("fynkel_flutter: CVPixelBufferCreateWithIOSurface failed (\(status))")
      return nil
    }
    if buffers.count >= Self.slots { buffers.removeAll(keepingCapacity: true) }
    buffers.append((raw, buffer))
    return Unmanaged.passRetained(buffer)
  }
}
