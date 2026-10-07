#if os(iOS)
import Flutter
#else
import FlutterMacOS
#endif
import Foundation

/// The method channel `fynkel_flutter`. Dart owns each engine view (made
/// through dart:ffi) and passes its address here; this class registers the
/// texture that shows it and tears down in an order the raster thread
/// survives.
public final class FynkelFlutterPlugin: NSObject, FlutterPlugin {
  private let registry: FlutterTextureRegistry
  private var live: [Int64: (texture: FynkelTexture, view: UnsafeRawPointer, setFrameReady: FySetFrameReady)] = [:]

  init(registry: FlutterTextureRegistry) {
    self.registry = registry
  }

  public static func register(with registrar: FlutterPluginRegistrar) {
    #if os(iOS)
    let messenger = registrar.messenger()
    let registry = registrar.textures()
    #else
    let messenger = registrar.messenger
    let registry = registrar.textures
    #endif
    let channel = FlutterMethodChannel(name: "fynkel_flutter", binaryMessenger: messenger)
    registrar.addMethodCallDelegate(FynkelFlutterPlugin(registry: registry), channel: channel)
  }

  public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let args = call.arguments as? [String: Any] ?? [:]
    switch call.method {
    case "register":
      guard let view = (args["view"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:)),
        let acquire = (args["acquire"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:)),
        let setFrameReady = (args["setFrameReady"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:))
      else {
        result(FlutterError(code: "arguments", message: "register needs view, acquire and setFrameReady", details: nil))
        return
      }
      let set = unsafeBitCast(setFrameReady, to: FySetFrameReady.self)
      let texture = FynkelTexture(view: view, acquire: unsafeBitCast(acquire, to: FyAcquire.self), registry: registry)
      texture.textureId = registry.register(texture)
      live[texture.textureId] = (texture, view, set)
      // `live` keeps the texture alive until the callback is cleared.
      _ = set(view, FynkelTexture.frameReady, Unmanaged.passUnretained(texture).toOpaque())
      result(texture.textureId)

    case "unregister":
      guard let id = (args["texture"] as? Int).map(Int64.init), let entry = live.removeValue(forKey: id) else {
        result(FlutterError(code: "arguments", message: "no such texture", details: nil))
        return
      }
      // The order matters: no more frame-ready calls (this waits for one in
      // progress), no more acquires, then Flutter forgets the texture. After
      // this Dart frees the view.
      _ = entry.setFrameReady(entry.view, nil, nil)
      entry.texture.detach()
      registry.unregisterTexture(id)
      result(nil)

    default:
      result(FlutterMethodNotImplemented)
    }
  }

  public func detachFromEngine(for registrar: FlutterPluginRegistrar) {
    for (id, entry) in live {
      _ = entry.setFrameReady(entry.view, nil, nil)
      entry.texture.detach()
      registry.unregisterTexture(id)
    }
    live.removeAll()
  }
}
