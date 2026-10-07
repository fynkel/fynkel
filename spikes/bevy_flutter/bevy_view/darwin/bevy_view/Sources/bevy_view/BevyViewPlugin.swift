#if os(iOS)
import Flutter
import UIKit
#else
import Cocoa
import FlutterMacOS
#endif

/// Method-channel glue (`bevy_view`). Dart owns every Rust
/// viewport (created through dart:ffi) and hands us raw pointers; this
/// class only does what must happen inside the Flutter embedder: wrap the
/// IOSurfaces, register the `FlutterTexture`, forward Rust's frame-ready
/// signal, and tear down in an order the raster thread survives.
///
/// Several viewports may be live at once (an editor has more than one
/// view); every call after `registerTexture` names its `textureId`.
public class BevyViewPlugin: NSObject, FlutterPlugin {
  private let textures: FlutterTextureRegistry
  private var live: [Int64: BevyViewTexture] = [:]
  private var setCallbacks: [Int64: BevyViewTexture.SetCallbackFn] = [:]

  public static func register(with registrar: FlutterPluginRegistrar) {
    #if os(iOS)
    let messenger = registrar.messenger()
    let textures = registrar.textures()
    #else
    let messenger = registrar.messenger
    let textures = registrar.textures
    #endif
    let channel = FlutterMethodChannel(name: "bevy_view", binaryMessenger: messenger)
    let instance = BevyViewPlugin(textures: textures)
    registrar.addMethodCallDelegate(instance, channel: channel)
  }

  init(textures: FlutterTextureRegistry) {
    self.textures = textures
  }

  public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let args = call.arguments as? [String: Any] ?? [:]
    switch call.method {
    case "registerTexture":
      guard let ctxAddr = args["ctx"] as? Int,
            let acquireAddr = args["acquireFn"] as? Int,
            let setCbAddr = args["setCallbackFn"] as? Int,
            let surfaces = args["iosurfaces"] as? [Int]
      else {
        result(FlutterError(code: "args", message: "bad arguments", details: nil))
        return
      }
      let ctx = UnsafeMutableRawPointer(bitPattern: ctxAddr)!
      let acquire = unsafeBitCast(UnsafeRawPointer(bitPattern: acquireAddr)!, to: BevyViewTexture.AcquireFn.self)
      let setCallback = unsafeBitCast(UnsafeRawPointer(bitPattern: setCbAddr)!, to: BevyViewTexture.SetCallbackFn.self)
      let ptrs = surfaces.map { UnsafeMutableRawPointer(bitPattern: $0)! }
      guard let tex = BevyViewTexture(ctx: ctx, acquire: acquire, iosurfaces: ptrs) else {
        result(FlutterError(code: "texture", message: "could not wrap IOSurfaces", details: nil))
        return
      }
      tex.registry = textures
      tex.textureId = textures.register(tex)
      live[tex.textureId] = tex
      setCallbacks[tex.textureId] = setCallback

      // Rust -> Swift frame-ready signal. userdata = unretained pointer to `tex`,
      // which `live` keeps alive until unregister.
      let userdata = Unmanaged.passUnretained(tex).toOpaque()
      let cb: BevyViewTexture.FrameReadyFn = { ud in
        guard let ud else { return }
        Unmanaged<BevyViewTexture>.fromOpaque(ud).takeUnretainedValue().onFrameReady()
      }
      setCallback(ctx, cb, userdata)
      result(tex.textureId)

    case "rebindTexture":
      guard let tex = texture(args), let surfaces = args["iosurfaces"] as? [Int] else {
        result(FlutterError(code: "args", message: "no such texture or bad arguments", details: nil))
        return
      }
      let ptrs = surfaces.map { UnsafeMutableRawPointer(bitPattern: $0)! }
      guard tex.rebind(iosurfaces: ptrs) else {
        result(FlutterError(code: "texture", message: "could not wrap IOSurfaces", details: nil))
        return
      }
      // Tell Flutter the next raster should ask us for a (new-size) frame.
      textures.textureFrameAvailable(tex.textureId)
      result(nil)

    case "stats":
      guard let tex = texture(args) else { result(nil); return }
      result(tex.takeStats())

    case "unregisterTexture":
      if let tex = texture(args) {
        // Order matters: stop Rust from signalling into `tex` before the
        // Flutter registry (and we) drop it. `set_frame_ready_callback` waits
        // for any in-progress callback, so no use-after-free window remains.
        setCallbacks[tex.textureId]?(tex.ctx, nil, nil)
        tex.detach()
        textures.unregisterTexture(tex.textureId)
        live[tex.textureId] = nil
        setCallbacks[tex.textureId] = nil
      }
      result(nil)

    default:
      result(FlutterMethodNotImplemented)
    }
  }

  private func texture(_ args: [String: Any]) -> BevyViewTexture? {
    guard let id = args["textureId"] as? Int else { return nil }
    return live[Int64(id)]
  }
}
