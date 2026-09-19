import Cocoa
import FlutterMacOS
import CoreVideo
import IOSurface

/// M0 Path A experiment: a `FlutterTexture` backed by a ring of IOSurfaces that
/// Rust/wgpu renders into. Every `copyPixelBuffer` asks Rust which buffer is
/// the newest GPU-complete one and returns the matching CVPixelBuffer.
/// No pixels are copied on the CPU.
final class FynkelM0Texture: NSObject, FlutterTexture {
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

  /// Counters for the M0 report.
  var copyCalls = 0
  var copyWithNewFrame = 0
  var frameReadySignals = 0
  var copyNoFrame = 0
  var rebinds = 0

  init?(ctx: UnsafeMutableRawPointer, acquire: AcquireFn, iosurfaces: [UnsafeMutableRawPointer]) {
    self.ctx = ctx
    self.acquire = acquire
    guard let buffers = FynkelM0Texture.wrap(iosurfaces) else { return nil }
    self.pixelBuffers = buffers
    super.init()
  }

  /// Swap in a new IOSurface ring (after `fynkel_m0_resize`). The old
  /// CVPixelBuffers are released here; Flutter may still hold a retained one
  /// for an in-flight raster, which keeps that IOSurface alive on its own.
  func rebind(iosurfaces: [UnsafeMutableRawPointer]) -> Bool {
    guard let buffers = FynkelM0Texture.wrap(iosurfaces) else { return false }
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
        NSLog("FynkelM0Texture: CVPixelBufferCreateWithIOSurface failed: \(status)")
        return nil
      }
      buffers.append(pbUnmanaged.takeRetainedValue())
    }
    return buffers
  }

  /// Called from Rust (possibly from a Metal completion thread) when a frame's GPU work finished.
  func onFrameReady() {
    frameReadySignals += 1
    // FlutterEngineMarkExternalTextureFrameAvailable is documented as thread-safe.
    registry?.textureFrameAvailable(textureId)
  }

  // MARK: FlutterTexture (called on Flutter's raster thread)

  func copyPixelBuffer() -> Unmanaged<CVPixelBuffer>? {
    lock.lock(); defer { lock.unlock() }
    copyCalls += 1
    if detached { return nil }
    let index = acquire(ctx)
    if index < 0 || Int(index) >= pixelBuffers.count {
      copyNoFrame += 1
      return nil
    }
    if index != lastIndex { copyWithNewFrame += 1 }
    lastIndex = index
    return Unmanaged.passRetained(pixelBuffers[Int(index)])
  }

  func onTextureUnregistered(_ texture: FlutterTexture) {}
}

/// Method channel glue. Dart owns the Rust context (created via dart:ffi) and
/// hands us raw pointers; we only do what must be done in the Flutter embedder.
final class FynkelM0Channel {
  private let registrar: FlutterPluginRegistrar
  private var texture: FynkelM0Texture?
  private var setCallback: FynkelM0Texture.SetCallbackFn?

  init(registrar: FlutterPluginRegistrar) {
    self.registrar = registrar
    let channel = FlutterMethodChannel(name: "fynkel/m0", binaryMessenger: registrar.messenger)
    channel.setMethodCallHandler { [weak self] call, result in
      self?.handle(call, result: result)
    }
  }

  private func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    switch call.method {
    case "registerTexture":
      guard let args = call.arguments as? [String: Any],
            let ctxAddr = args["ctx"] as? Int,
            let acquireAddr = args["acquireFn"] as? Int,
            let setCbAddr = args["setCallbackFn"] as? Int,
            let surfaces = args["iosurfaces"] as? [Int]
      else {
        result(FlutterError(code: "args", message: "bad arguments", details: nil))
        return
      }
      let ctx = UnsafeMutableRawPointer(bitPattern: ctxAddr)!
      let acquire = unsafeBitCast(UnsafeRawPointer(bitPattern: acquireAddr)!, to: FynkelM0Texture.AcquireFn.self)
      let setCallback = unsafeBitCast(UnsafeRawPointer(bitPattern: setCbAddr)!, to: FynkelM0Texture.SetCallbackFn.self)
      let ptrs = surfaces.map { UnsafeMutableRawPointer(bitPattern: $0)! }
      guard let tex = FynkelM0Texture(ctx: ctx, acquire: acquire, iosurfaces: ptrs) else {
        result(FlutterError(code: "texture", message: "could not wrap IOSurfaces", details: nil))
        return
      }
      tex.registry = registrar.textures
      tex.textureId = registrar.textures.register(tex)
      texture = tex

      // Rust -> Swift frame-ready signal. userdata = unretained pointer to `tex`,
      // which we keep alive in `self.texture` until unregister.
      let userdata = Unmanaged.passUnretained(tex).toOpaque()
      let cb: FynkelM0Texture.FrameReadyFn = { ud in
        guard let ud else { return }
        Unmanaged<FynkelM0Texture>.fromOpaque(ud).takeUnretainedValue().onFrameReady()
      }
      setCallback(ctx, cb, userdata)
      self.setCallback = setCallback
      result(tex.textureId)

    case "rebindTexture":
      guard let tex = texture,
            let args = call.arguments as? [String: Any],
            let surfaces = args["iosurfaces"] as? [Int]
      else {
        result(FlutterError(code: "args", message: "no texture or bad arguments", details: nil))
        return
      }
      let ptrs = surfaces.map { UnsafeMutableRawPointer(bitPattern: $0)! }
      guard tex.rebind(iosurfaces: ptrs) else {
        result(FlutterError(code: "texture", message: "could not wrap IOSurfaces", details: nil))
        return
      }
      // Tell Flutter the next raster should ask us for a (new-size) frame.
      registrar.textures.textureFrameAvailable(tex.textureId)
      result(nil)

    case "stats":
      guard let tex = texture else { result(nil); return }
      result([
        "copyCalls": tex.copyCalls,
        "copyWithNewFrame": tex.copyWithNewFrame,
        "frameReadySignals": tex.frameReadySignals,
        "copyNoFrame": tex.copyNoFrame,
        "rebinds": tex.rebinds,
      ])
      tex.copyCalls = 0; tex.copyWithNewFrame = 0; tex.frameReadySignals = 0
      tex.copyNoFrame = 0; tex.rebinds = 0

    case "unregisterTexture":
      if let tex = texture {
        // Order matters: stop Rust from signalling into `tex` before the
        // Flutter registry (and we) drop it. `set_frame_ready_callback` waits
        // for any in-progress callback, so no use-after-free window remains.
        setCallback?(tex.ctx, nil, nil)
        tex.detach()
        registrar.textures.unregisterTexture(tex.textureId)
        texture = nil
        setCallback = nil
      }
      result(nil)

    default:
      result(FlutterMethodNotImplemented)
    }
  }
}
