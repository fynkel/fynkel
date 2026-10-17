#if os(iOS)
import Flutter
import QuartzCore
import UIKit

typealias FyLayerAttach = @convention(c) (UnsafeRawPointer?, UnsafeMutableRawPointer?, UInt32, UInt32) -> Int32
typealias FyLayerDetach = @convention(c) (UnsafeRawPointer?) -> Int32

/// A view the engine draws straight to (its `CAMetalLayer`), under
/// Flutter's see-through view in the window, where the `FynkelView` is
/// (docs/ios-surface.md). Flutter paints a hole there, so the game shows
/// through, and the widgets over it draw over it.
final class FynkelLayerView: UIView {
  override class var layerClass: AnyClass { CAMetalLayer.self }

  let engine: UnsafeRawPointer
  private let attachFn: FyLayerAttach
  private let detachFn: FyLayerDetach
  let free: FyViewFree

  init(engine: UnsafeRawPointer, attach: FyLayerAttach, detach: FyLayerDetach, free: FyViewFree) {
    self.engine = engine
    self.attachFn = attach
    self.detachFn = detach
    self.free = free
    super.init(frame: .zero)
    isUserInteractionEnabled = false
    isOpaque = true
    backgroundColor = .black
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("not from a storyboard") }

  /// The engine draws its next frames here at `width` × `height` pixels.
  func attach(width: Int, height: Int) -> Bool {
    let layer = Unmanaged.passUnretained(self.layer).toOpaque()
    return attachFn(engine, layer, UInt32(max(width, 1)), UInt32(max(height, 1))) == 0
  }

  /// The engine stops drawing here, after a frame in progress.
  func detach() {
    _ = detachFn(engine)
  }

  /// Puts it where the `FynkelView` is: physical pixels from the top left
  /// of Flutter's view.
  func place(left: Int, top: Int, width: Int, height: Int, scale: CGFloat) {
    frame = CGRect(
      x: CGFloat(left) / scale, y: CGFloat(top) / scale,
      width: CGFloat(width) / scale, height: CGFloat(height) / scale)
  }
}

/// The plugin's iOS layers: one per `FynkelView` that draws straight to
/// the screen.
final class FynkelLayers {
  private var live: [Int64: FynkelLayerView] = [:]
  // Ids apart from Flutter's texture ids, which count from 0.
  private var nextId: Int64 = 1 << 40
  /// The view controller showing Flutter's view, the registrar's.
  private let controller: () -> UIViewController?

  init(controller: @escaping () -> UIViewController?) {
    self.controller = controller
  }

  /// Flutter's view, made see-through, and the view that holds it: the
  /// layers go in there, under Flutter's view. Nil with no Flutter view on
  /// screen yet, or one in a view controller of the app's own (add to
  /// app): there the views stay textures.
  private func flutter() -> (UIView, UIView)? {
    guard let controller = controller() as? FlutterViewController, let view = controller.viewIfLoaded,
      let holder = view.superview
    else { return nil }
    if controller.isViewOpaque { controller.isViewOpaque = false }
    return (view, holder)
  }

  func register(_ args: [String: Any], result: FlutterResult) {
    guard let engine = (args["view"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:)),
      let attach = (args["attach"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:)),
      let detach = (args["detach"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:)),
      let free = (args["free"] as? Int).flatMap(UnsafeRawPointer.init(bitPattern:)),
      let width = args["width"] as? Int, let height = args["height"] as? Int
    else {
      result(FlutterError(code: "arguments", message: "a layer needs view, attach, detach, free and a size", details: nil))
      return
    }
    guard let (flutterView, holder) = flutter() else {
      result(FlutterError(code: "nolayer", message: "no Flutter view on screen to put a layer under", details: nil))
      return
    }
    let view = FynkelLayerView(
      engine: engine, attach: unsafeBitCast(attach, to: FyLayerAttach.self),
      detach: unsafeBitCast(detach, to: FyLayerDetach.self), free: unsafeBitCast(free, to: FyViewFree.self))
    let scale = flutterView.contentScaleFactor
    view.layer.contentsScale = scale
    view.place(
      left: args["left"] as? Int ?? 0, top: args["top"] as? Int ?? 0,
      width: args["frameWidth"] as? Int ?? width, height: args["frameHeight"] as? Int ?? height, scale: scale)
    holder.insertSubview(view, belowSubview: flutterView)
    guard view.attach(width: width, height: height) else {
      view.removeFromSuperview()
      result(FlutterError(code: "nolayer", message: "the engine couldn't draw to a layer", details: nil))
      return
    }
    let id = nextId
    nextId += 1
    live[id] = view
    result(["id": id, "onScreen": true])
  }

  /// Whether `id` is one of the layers' (rather than a texture's).
  func owns(_ id: Int64) -> Bool { live[id] != nil }

  func resize(_ id: Int64, width: Int, height: Int, result: FlutterResult) {
    guard let view = live[id] else {
      result(FlutterError(code: "arguments", message: "no such layer", details: nil))
      return
    }
    if view.attach(width: width, height: height) {
      result(nil)
    } else {
      result(FlutterError(code: "layer", message: "the engine couldn't draw to the layer at its new size", details: nil))
    }
  }

  func place(_ id: Int64, _ args: [String: Any]) {
    guard let view = live[id] else { return }
    view.place(
      left: args["left"] as? Int ?? 0, top: args["top"] as? Int ?? 0,
      width: args["width"] as? Int ?? 0, height: args["height"] as? Int ?? 0, scale: view.layer.contentsScale)
  }

  /// Over the other layers (a minimap over the field), or under them.
  func over(_ id: Int64, _ over: Bool) {
    guard let view = live[id], let holder = view.superview, let (flutterView, _) = flutter(),
      flutterView.superview === holder
    else { return }
    if over {
      holder.insertSubview(view, belowSubview: flutterView)
    } else if let lowest = live.values.compactMap({ v in holder.subviews.firstIndex(of: v) }).min() {
      holder.insertSubview(view, at: lowest)
    }
  }

  /// The engine stops drawing to it, and it goes: after this Dart frees
  /// the view.
  func unregister(_ id: Int64) {
    guard let view = live.removeValue(forKey: id) else { return }
    view.detach()
    view.removeFromSuperview()
  }

  /// Lets go of and frees every layer's view: their isolate is gone.
  func releaseOrphans() {
    for (_, view) in live {
      view.detach()
      view.removeFromSuperview()
      view.free(view.engine)
    }
    live.removeAll()
  }
}
#endif
