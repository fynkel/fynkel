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
///
/// A hot restart starts a new Dart isolate without disposing anything, so
/// the old isolate's views would stay registered, alive and holding their
/// GPU memory. The first `FynkelView` of each isolate calls `reset` before
/// it registers, and every view still here then belongs to an isolate that
/// is gone: this class releases and frees those itself.
public final class FynkelFlutterPlugin: NSObject, FlutterPlugin {
  private struct Entry {
    let texture: FynkelTexture
    let view: UnsafeRawPointer
    let setFrameReady: FySetFrameReady
    let free: FyViewFree
  }

  private let registry: FlutterTextureRegistry
  private var live: [Int64: Entry] = [:]
  #if os(iOS)
  // The views drawn straight to the screen (docs/ios-surface.md).
  private let layers: FynkelLayers

  init(registry: FlutterTextureRegistry, controller: @escaping () -> UIViewController?) {
    self.registry = registry
    self.layers = FynkelLayers(controller: controller)
  }
  #else

  init(registry: FlutterTextureRegistry) {
    self.registry = registry
  }
  #endif

  public static func register(with registrar: FlutterPluginRegistrar) {
    #if os(iOS)
    let messenger = registrar.messenger()
    let registry = registrar.textures()
    let plugin = FynkelFlutterPlugin(registry: registry) { [weak registrar] in registrar?.viewController }
    #else
    let messenger = registrar.messenger
    let registry = registrar.textures
    let plugin = FynkelFlutterPlugin(registry: registry)
    #endif
    let channel = FlutterMethodChannel(name: "fynkel_flutter", binaryMessenger: messenger)
    registrar.addMethodCallDelegate(plugin, channel: channel)
  }

  public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let args = call.arguments as? [String: Any] ?? [:]
    #if os(iOS)
    let id = (args["texture"] as? Int).map(Int64.init)
    switch call.method {
    case "register" where args["layer"] as? Bool == true:
      layers.register(args, result: result)
      return
    case "resize":
      if let id, layers.owns(id) {
        layers.resize(id, width: args["width"] as? Int ?? 1, height: args["height"] as? Int ?? 1, result: result)
      } else {
        result(nil)
      }
      return
    case "place":
      if let id { layers.place(id, args) }
      result(nil)
      return
    case "over":
      if let id { layers.over(id, args["over"] as? Bool ?? false) }
      result(nil)
      return
    case "shown":
      if let id { layers.shown(id, args["shown"] as? Bool ?? true) }
      result(nil)
      return
    case "unregister" where id.map(layers.owns) == true:
      layers.unregister(id!)
      result(nil)
      return
    default:
      break
    }
    #endif
    switch call.method {
    case "register":
      guard let view = (args["view"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:)),
        let acquire = (args["acquire"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:)),
        let setFrameReady = (args["setFrameReady"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:)),
        let free = (args["free"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:))
      else {
        result(FlutterError(code: "arguments", message: "register needs view, acquire, setFrameReady and free", details: nil))
        return
      }
      let set = unsafeBitCast(setFrameReady, to: FySetFrameReady.self)
      let texture = FynkelTexture(view: view, acquire: unsafeBitCast(acquire, to: FyAcquire.self), registry: registry)
      texture.textureId = registry.register(texture)
      live[texture.textureId] = Entry(
        texture: texture, view: view, setFrameReady: set, free: unsafeBitCast(free, to: FyViewFree.self))
      // `live` keeps the texture alive until the callback is cleared.
      _ = set(view, FynkelTexture.frameReady, Unmanaged.passUnretained(texture).toOpaque())
      result(texture.textureId)

    case "unregister":
      guard let id = (args["texture"] as? Int).map(Int64.init), let entry = live.removeValue(forKey: id) else {
        result(FlutterError(code: "arguments", message: "no such texture", details: nil))
        return
      }
      // After this Dart frees the view.
      release(id, entry)
      result(nil)

    case "reset":
      releaseOrphans()
      result(nil)

    default:
      result(FlutterMethodNotImplemented)
    }
  }

  /// The order matters: no more frame-ready calls (this waits for one in
  /// progress), no more acquires, then Flutter forgets the texture. Only
  /// then may the view be freed.
  private func release(_ id: Int64, _ entry: Entry) {
    _ = entry.setFrameReady(entry.view, nil, nil)
    entry.texture.detach()
    registry.unregisterTexture(id)
  }

  /// Releases and frees every view: their isolate is gone, so no Dart code
  /// will.
  private func releaseOrphans() {
    #if os(iOS)
    layers.releaseOrphans()
    #endif
    for (id, entry) in live {
      release(id, entry)
      entry.free(entry.view)
    }
    live.removeAll()
  }

  public func detachFromEngine(for registrar: FlutterPluginRegistrar) {
    releaseOrphans()
  }
}
