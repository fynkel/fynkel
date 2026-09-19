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

  private let ctx: UnsafeMutableRawPointer
  private let acquire: AcquireFn
  private let pixelBuffers: [CVPixelBuffer]
  private var lastIndex: Int32 = -1

  var textureId: Int64 = -1
  weak var registry: FlutterTextureRegistry?

  /// Counters for the M0 report.
  var copyCalls = 0
  var copyWithNewFrame = 0
  var frameReadySignals = 0

  init?(ctx: UnsafeMutableRawPointer, acquire: AcquireFn, iosurfaces: [UnsafeMutableRawPointer], width: Int, height: Int) {
    self.ctx = ctx
    self.acquire = acquire
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
    self.pixelBuffers = buffers
    super.init()
  }

  /// Called from Rust (possibly from a Metal completion thread) when a frame's GPU work finished.
  func onFrameReady() {
    frameReadySignals += 1
    // FlutterEngineMarkExternalTextureFrameAvailable is documented as thread-safe.
    registry?.textureFrameAvailable(textureId)
  }

  // MARK: FlutterTexture (called on Flutter's raster thread)

  func copyPixelBuffer() -> Unmanaged<CVPixelBuffer>? {
    copyCalls += 1
    let index = acquire(ctx)
    if index < 0 { return nil }
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
            let surfaces = args["iosurfaces"] as? [Int],
            let width = args["width"] as? Int,
            let height = args["height"] as? Int
      else {
        result(FlutterError(code: "args", message: "bad arguments", details: nil))
        return
      }
      let ctx = UnsafeMutableRawPointer(bitPattern: ctxAddr)!
      let acquire = unsafeBitCast(UnsafeRawPointer(bitPattern: acquireAddr)!, to: FynkelM0Texture.AcquireFn.self)
      let setCallback = unsafeBitCast(UnsafeRawPointer(bitPattern: setCbAddr)!, to: FynkelM0Texture.SetCallbackFn.self)
      let ptrs = surfaces.map { UnsafeMutableRawPointer(bitPattern: $0)! }
      guard let tex = FynkelM0Texture(ctx: ctx, acquire: acquire, iosurfaces: ptrs, width: width, height: height) else {
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
      result(tex.textureId)

    case "stats":
      guard let tex = texture else { result(nil); return }
      result([
        "copyCalls": tex.copyCalls,
        "copyWithNewFrame": tex.copyWithNewFrame,
        "frameReadySignals": tex.frameReadySignals,
      ])
      tex.copyCalls = 0; tex.copyWithNewFrame = 0; tex.frameReadySignals = 0

    case "unregisterTexture":
      if let tex = texture {
        registrar.textures.unregisterTexture(tex.textureId)
        texture = nil
      }
      result(nil)

    default:
      result(FlutterMethodNotImplemented)
    }
  }
}
