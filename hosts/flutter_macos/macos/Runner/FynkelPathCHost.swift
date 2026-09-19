import Cocoa
import FlutterMacOS
import QuartzCore

/// Path C host (blueprint §8.2, ADR-0002 fallback): the engine renders into
/// a `CAMetalLayer` that backs `engineView`, which sits *under* a transparent
/// `FlutterView` in the same window. The window server composites the two
/// layers; Flutter never sees the engine's frames and the engine never sees
/// Flutter's. Dart owns the Rust context; we own the view and report
/// size/scale changes.
///
/// Placement: `engineView` is a sibling of `FlutterView` inside the
/// FlutterViewController's own wrapper view (where Flutter puts platform
/// views too). Keeping the controller as the window's contentViewController
/// leaves AppKit's responder chain untouched — hosting Flutter as a child
/// view controller under our own container silently dropped mouseDown
/// before it reached FlutterViewController (keys and hover still worked).
final class FynkelPathCHost {
  let flutterViewController: FlutterViewController
  let engineView = NSView()
  let metalLayer = CAMetalLayer()
  private var channel: FlutterMethodChannel?
  private var observers: [NSObjectProtocol] = []

  init(flutterViewController: FlutterViewController) {
    self.flutterViewController = flutterViewController
    flutterViewController.backgroundColor = .clear

    let wrapper = flutterViewController.view
    metalLayer.isOpaque = true
    metalLayer.backgroundColor = NSColor.black.cgColor
    engineView.layer = metalLayer
    engineView.wantsLayer = true
    engineView.frame = wrapper.bounds
    // Dart reports the viewport widget's rect (`setFrame`); until then, and
    // during live resize between reports, the view tracks the window.
    engineView.autoresizingMask = [.width, .height]
    engineView.postsFrameChangedNotifications = true
    if let flutterView = wrapper.subviews.first {
      wrapper.addSubview(engineView, positioned: .below, relativeTo: flutterView)
    } else {
      wrapper.addSubview(engineView)
    }
  }

  var pixelSize: (Int, Int, Double) {
    let scale = engineView.window?.backingScaleFactor ?? 2.0
    let b = engineView.bounds
    return (Int((b.width * scale).rounded()), Int((b.height * scale).rounded()), scale)
  }

  func install(registrar: FlutterPluginRegistrar) {
    let ch = FlutterMethodChannel(name: "fynkel/m0/pathc", binaryMessenger: registrar.messenger)
    channel = ch
    ch.setMethodCallHandler { [weak self] call, result in
      guard let self else { result(FlutterMethodNotImplemented); return }
      switch call.method {
      case "attach":
        let (w, h, s) = self.pixelSize
        self.metalLayer.contentsScale = s
        self.metalLayer.drawableSize = CGSize(width: w, height: h)
        result(["layer": Int(bitPattern: Unmanaged.passUnretained(self.metalLayer).toOpaque()),
                "width": w, "height": h, "scale": s])
        self.observe()
      case "setFrame":
        // Viewport widget rect in Flutter logical coordinates (origin top-left)
        // → AppKit view frame (origin bottom-left). Lets the engine surface
        // follow Flutter layout instead of covering the whole window.
        let a = call.arguments as! [String: Any]
        let x = a["x"] as! Double, y = a["y"] as! Double, w = a["w"] as! Double, h = a["h"] as! Double
        let wrapper = self.flutterViewController.view
        let frame = NSRect(x: x, y: wrapper.bounds.height - y - h, width: w, height: h)
        if frame != self.engineView.frame {
          self.engineView.autoresizingMask = []
          self.engineView.frame = frame
        }
        result(nil)
      case "size":
        // Polled by Dart on metrics changes as a safety net for a missed
        // frame notification (e.g. the window's autosaved frame restored
        // right around attach time).
        let (w, h, s) = self.pixelSize
        result(["width": w, "height": h, "scale": s])
      case "detach":
        self.observers.forEach(NotificationCenter.default.removeObserver)
        self.observers.removeAll()
        result(nil)
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }

  private func observe() {
    let nc = NotificationCenter.default
    let report: (Notification) -> Void = { [weak self] _ in
      guard let self else { return }
      let (w, h, s) = self.pixelSize
      self.metalLayer.contentsScale = s
      self.channel?.invokeMethod("resized", arguments: ["width": w, "height": h, "scale": s])
    }
    observers.append(nc.addObserver(forName: NSView.frameDidChangeNotification, object: engineView, queue: .main, using: report))
    if let window = engineView.window {
      observers.append(nc.addObserver(forName: NSWindow.didChangeBackingPropertiesNotification, object: window, queue: .main, using: report))
      observers.append(nc.addObserver(forName: NSWindow.didMiniaturizeNotification, object: window, queue: .main) { [weak self] _ in
        self?.channel?.invokeMethod("visibility", arguments: false)
      })
      observers.append(nc.addObserver(forName: NSWindow.didDeminiaturizeNotification, object: window, queue: .main) { [weak self] _ in
        self?.channel?.invokeMethod("visibility", arguments: true)
      })
    }
  }
}
