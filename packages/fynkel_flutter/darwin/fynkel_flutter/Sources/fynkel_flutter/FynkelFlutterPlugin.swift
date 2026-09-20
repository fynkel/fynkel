#if os(iOS)
import Flutter
import UIKit
#else
import Cocoa
import FlutterMacOS
#endif

/// Method-channel glue (`fynkel_flutter/viewport`). Dart owns every Rust
/// viewport (created through dart:ffi) and hands us raw pointers; this
/// class only does what must happen inside the Flutter embedder: wrap the
/// IOSurfaces, register the `FlutterTexture`, forward Rust's frame-ready
/// signal, and tear down in an order the raster thread survives.
///
/// Several viewports may be live at once (an editor has more than one
/// view); every call after `registerTexture` names its `textureId`.
public class FynkelFlutterPlugin: NSObject, FlutterPlugin {
  private let textures: FlutterTextureRegistry
  private var live: [Int64: FynkelViewportTexture] = [:]
  private var setCallbacks: [Int64: FynkelViewportTexture.SetCallbackFn] = [:]

  public static func register(with registrar: FlutterPluginRegistrar) {
    #if os(iOS)
    let messenger = registrar.messenger()
    let textures = registrar.textures()
    #else
    let messenger = registrar.messenger
    let textures = registrar.textures
    #endif
    let channel = FlutterMethodChannel(name: "fynkel_flutter/viewport", binaryMessenger: messenger)
    let instance = FynkelFlutterPlugin(textures: textures)
    registrar.addMethodCallDelegate(instance, channel: channel)
    // The input half (gamepads, captured pointer) shares the registrar:
    // Flutter's tooling registers one class per plugin.
    FynkelInputPlugin.register(with: registrar)
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
      let acquire = unsafeBitCast(UnsafeRawPointer(bitPattern: acquireAddr)!, to: FynkelViewportTexture.AcquireFn.self)
      let setCallback = unsafeBitCast(UnsafeRawPointer(bitPattern: setCbAddr)!, to: FynkelViewportTexture.SetCallbackFn.self)
      let ptrs = surfaces.map { UnsafeMutableRawPointer(bitPattern: $0)! }
      guard let tex = FynkelViewportTexture(ctx: ctx, acquire: acquire, iosurfaces: ptrs) else {
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
      let cb: FynkelViewportTexture.FrameReadyFn = { ud in
        guard let ud else { return }
        Unmanaged<FynkelViewportTexture>.fromOpaque(ud).takeUnretainedValue().onFrameReady()
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

  private func texture(_ args: [String: Any]) -> FynkelViewportTexture? {
    guard let id = args["textureId"] as? Int else { return nil }
    return live[Int64(id)]
  }
}
