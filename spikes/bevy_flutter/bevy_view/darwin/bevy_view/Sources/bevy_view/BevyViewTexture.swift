#if os(iOS)
import Flutter
import UIKit
#else
import Cocoa
import FlutterMacOS
#endif
import CoreVideo
import IOSurface

/// Path A viewport (ADR-0002, platform/apple): a `FlutterTexture` backed by a
/// ring of IOSurfaces that Rust/wgpu renders into. Every `copyPixelBuffer`
/// asks Rust which slot is the newest GPU-complete one and returns the
/// matching CVPixelBuffer. No pixels are copied on the CPU.
///
/// One file for macOS and iOS (ADR-0009): only the imports differ.
final class BevyViewTexture: NSObject, FlutterTexture {
  typealias AcquireFn = @convention(c) (UnsafeMutableRawPointer?) -> Int32
  typealias FrameReadyFn = @convention(c) (UnsafeMutableRawPointer?) -> Void
  typealias SetCallbackFn = @convention(c) (UnsafeMutableRawPointer?, FrameReadyFn?, UnsafeMutableRawPointer?) -> Void

  let ctx: UnsafeMutableRawPointer
  private let acquire: AcquireFn
  /// Guards `pixelBuffers`/`lastIndex`: written on the platform thread
  /// (rebind after a resize), read on the raster thread.
  private let lock = NSLock()
  private var pixelBuffers: [CVPixelBuffer]
  private var lastIndex: Int32 = -1
  /// Set on unregister; after this the raster thread never calls into Rust
  /// again, so the host may free `ctx`.
  private var detached = false

  var textureId: Int64 = -1
  weak var registry: FlutterTextureRegistry?

  /// Counters for the stats overlay / benchmark capture.
  private var copyCalls = 0
  private var copyWithNewFrame = 0
  private var frameReadySignals = 0
  private var copyNoFrame = 0
  private var rebinds = 0
  private var moreQueued = 0

  init?(ctx: UnsafeMutableRawPointer, acquire: AcquireFn, iosurfaces: [UnsafeMutableRawPointer]) {
    self.ctx = ctx
    self.acquire = acquire
    guard let buffers = BevyViewTexture.wrap(iosurfaces) else { return nil }
    self.pixelBuffers = buffers
    super.init()
  }

  /// Swap in a new IOSurface ring (after `bf_resize`). The old
  /// CVPixelBuffers are released here; Flutter may still hold a retained one
  /// for an in-flight raster, which keeps that IOSurface alive on its own.
  func rebind(iosurfaces: [UnsafeMutableRawPointer]) -> Bool {
    guard let buffers = BevyViewTexture.wrap(iosurfaces) else { return false }
    lock.lock()
    pixelBuffers = buffers
    lastIndex = -1
    rebinds += 1
    lock.unlock()
    return true
  }

  func detach() {
    lock.lock()
    detached = true
    pixelBuffers = []
    lock.unlock()
  }

  private static func wrap(_ iosurfaces: [UnsafeMutableRawPointer]) -> [CVPixelBuffer]? {
    var buffers: [CVPixelBuffer] = []
    for ptr in iosurfaces {
      let surface = Unmanaged<IOSurface>.fromOpaque(ptr).takeUnretainedValue()
      var pbUnmanaged: Unmanaged<CVPixelBuffer>?
      let attrs: [CFString: Any] = [
        kCVPixelBufferMetalCompatibilityKey: true,
      ]
      let status = CVPixelBufferCreateWithIOSurface(nil, surface, attrs as CFDictionary, &pbUnmanaged)
      guard status == kCVReturnSuccess, let pbUnmanaged else {
        NSLog("BevyViewTexture: CVPixelBufferCreateWithIOSurface failed: \(status)")
        return nil
      }
      buffers.append(pbUnmanaged.takeRetainedValue())
    }
    return buffers
  }

  /// Called from Rust (possibly from a Metal completion thread) when a frame's GPU work finished.
  func onFrameReady() {
    lock.lock()
    frameReadySignals += 1
    lock.unlock()
    // FlutterEngineMarkExternalTextureFrameAvailable is documented as thread-safe.
    registry?.textureFrameAvailable(textureId)
  }

  // MARK: FlutterTexture (called on Flutter's raster thread)

  func copyPixelBuffer() -> Unmanaged<CVPixelBuffer>? {
    lock.lock()
    copyCalls += 1
    if detached { lock.unlock(); return nil }
    let raw = acquire(ctx)
    // Bit 8: another finished frame is queued behind this one (the viewport's
    // frame queue); the engine copies once per signal, so ask for another.
    let more = raw >= 0 && (raw & 0x100) != 0
    let index = raw >= 0 ? raw & 0xff : raw
    if index < 0 || Int(index) >= pixelBuffers.count {
      copyNoFrame += 1
      lock.unlock()
      return nil
    }
    if index != lastIndex { copyWithNewFrame += 1 }
    lastIndex = index
    let buffer = Unmanaged.passRetained(pixelBuffers[Int(index)])
    let id = textureId
    let registry = self.registry
    lock.unlock()
    if more { moreQueued += 1 }
    if more { DispatchQueue.global(qos: .userInteractive).async { registry?.textureFrameAvailable(id) } }
    return buffer
  }

  /// Read and reset under the same lock as raster/completion-thread writes.
  func takeStats() -> [String: Int] {
    lock.lock(); defer { lock.unlock() }
    let result = [
      "copyCalls": copyCalls,
      "copyWithNewFrame": copyWithNewFrame,
      "frameReadySignals": frameReadySignals,
      "copyNoFrame": copyNoFrame,
      "rebinds": rebinds,
      "copyQueued": moreQueued,
      "thermalState": ProcessInfo.processInfo.thermalState.rawValue,
    ]
    copyCalls = 0; copyWithNewFrame = 0; frameReadySignals = 0
    copyNoFrame = 0; rebinds = 0; moreQueued = 0
    return result
  }

  func onTextureUnregistered(_ texture: FlutterTexture) {}
}
