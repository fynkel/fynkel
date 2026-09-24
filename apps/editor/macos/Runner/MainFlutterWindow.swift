import Cocoa
import FlutterMacOS

// The editor draws its own title bar (item 9b, lib/src/shell/window_chrome.dart):
// the native one is transparent and its title hidden, the content runs under
// it, and an empty compact toolbar gives it the editor bar's 38 points so the
// traffic lights sit centred on the editor's bar. The editor asks for a drag or
// a double-click on its bar's empty space over the `fynkel.editor/window`
// channel, and hears when the window enters or leaves full screen (where the
// traffic lights are gone and the bar starts at the left edge). In full screen
// the toolbar goes: AppKit would move it into a bar of its own, opaque (white
// in light mode), over the editor's bar.
class MainFlutterWindow: NSWindow {
  private var channel: FlutterMethodChannel?

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    let handedOver = Self.handedOverFrame()
    self.setFrame(handedOver ?? windowFrame, display: true)
    if let frame = handedOver {
      // Again once on screen: Stage Manager places a new window itself.
      DispatchQueue.main.async { [weak self] in self?.setFrame(frame, display: true) }
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in
        self?.setFrame(frame, display: true)
      }
    }

    titlebarAppearsTransparent = true
    titleVisibility = .hidden
    styleMask.insert(.fullSizeContentView)
    let toolbar = NSToolbar(identifier: "fynkel.editor.titlebar")
    toolbar.showsBaselineSeparator = false
    self.toolbar = toolbar
    toolbarStyle = .unifiedCompact
    // The bar is the editor's: left to itself AppKit drags the window from
    // any press in it, a tab or a button included. Only the editor knows
    // which parts are empty, so it starts the drag ("startDrag" below).
    isMovable = false

    RegisterGeneratedPlugins(registry: flutterViewController)

    let channel = FlutterMethodChannel(
      name: "fynkel.editor/window", binaryMessenger: flutterViewController.engine.binaryMessenger)
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self = self else {
        result(nil)
        return
      }
      switch call.method {
      case "isFullScreen":
        result(self.styleMask.contains(.fullScreen))
      case "frame":
        // For a handover: the project editor opens where this window is.
        if self.styleMask.contains(.fullScreen) {
          result(nil)
        } else {
          let f = self.frame
          result([f.origin.x, f.origin.y, f.size.width, f.size.height])
        }
      case "startDrag":
        if let event = NSApp.currentEvent {
          self.performDrag(with: event)
        }
        result(nil)
      case "titleBarDoubleClick":
        self.titleBarDoubleClick()
        result(nil)
      default:
        result(FlutterMethodNotImplemented)
      }
    }
    self.channel = channel

    let drops = FileDropView(
      frame: flutterViewController.view.bounds,
      channel: FlutterMethodChannel(
        name: "fynkel.editor/drop", binaryMessenger: flutterViewController.engine.binaryMessenger))
    drops.autoresizingMask = [.width, .height]
    flutterViewController.view.addSubview(drops)

    let center = NotificationCenter.default
    center.addObserver(
      forName: NSWindow.willEnterFullScreenNotification, object: self, queue: .main
    ) { [weak self] _ in
      self?.toolbar?.isVisible = false
      self?.channel?.invokeMethod("fullScreen", arguments: true)
    }
    center.addObserver(
      forName: NSWindow.willExitFullScreenNotification, object: self, queue: .main
    ) { [weak self] _ in
      self?.toolbar?.isVisible = true
    }
    center.addObserver(
      forName: NSWindow.didExitFullScreenNotification, object: self, queue: .main
    ) { [weak self] _ in
      self?.channel?.invokeMethod("fullScreen", arguments: false)
    }

    super.awakeFromNib()
  }

  /// The frame of the editor that handed a project over to this one
  /// (`FYNKEL_WINDOW_FRAME` = "x,y,w,h", screen points, origin bottom
  /// left), so the new window takes the old one's place (ADR-0021 P3c).
  /// Ignored unless it is on a screen.
  private static func handedOverFrame() -> NSRect? {
    guard let spec = ProcessInfo.processInfo.environment["FYNKEL_WINDOW_FRAME"] else { return nil }
    let v = spec.split(separator: ",").compactMap { Double($0.trimmingCharacters(in: .whitespaces)) }
    guard v.count == 4, v[2] >= 200, v[3] >= 150 else { return nil }
    let frame = NSRect(x: v[0], y: v[1], width: v[2], height: v[3])
    guard NSScreen.screens.contains(where: { $0.visibleFrame.intersects(frame) }) else { return nil }
    return frame
  }

  /// What a double-click on a title bar does, per System Settings → Desktop &
  /// Dock → "Double-click a window's title bar to".
  private func titleBarDoubleClick() {
    let action = UserDefaults.standard.string(forKey: "AppleActionOnDoubleClick")
    switch action {
    case "Minimize":
      miniaturize(nil)
    case "None":
      break
    default:
      zoom(nil)
    }
  }
}

// Files dragged in from Finder (the Flutter view takes no file drags): a
// clear view over it registered for file URLs reports the drag on
// `fynkel.editor/drop` in the view's logical coordinates (origin top left)
// — `over` as it moves, `exit`, `drop` with the paths — and the Project
// browser under the point imports them (lib/src/shell/file_drops.dart).
// It draws nothing and handles no mouse event: clicks go up the responder
// chain to Flutter, as with the desktop_drop plugin.
final class FileDropView: NSView {
  private let channel: FlutterMethodChannel

  init(frame: NSRect, channel: FlutterMethodChannel) {
    self.channel = channel
    super.init(frame: frame)
    registerForDraggedTypes([.fileURL])
  }

  required init?(coder: NSCoder) { nil }

  /// A click on an inactive window acts as the Flutter view beneath says.
  override func acceptsFirstMouse(for event: NSEvent?) -> Bool {
    superview?.subviews.first(where: { $0 !== self })?.acceptsFirstMouse(for: event)
      ?? superview?.acceptsFirstMouse(for: event) ?? true
  }

  private func paths(_ sender: NSDraggingInfo) -> [String] {
    let urls =
      sender.draggingPasteboard.readObjects(
        forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
    return urls.map { $0.path }
  }

  private func point(_ sender: NSDraggingInfo) -> [String: Any] {
    let p = convert(sender.draggingLocation, from: nil)
    return ["x": Double(p.x), "y": Double(isFlipped ? p.y : bounds.height - p.y)]
  }

  override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
    draggingUpdated(sender)
  }

  override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
    if paths(sender).isEmpty { return [] }
    channel.invokeMethod("over", arguments: point(sender))
    return .copy
  }

  override func draggingExited(_ sender: NSDraggingInfo?) {
    channel.invokeMethod("exit", arguments: nil)
  }

  override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
    let files = paths(sender)
    if files.isEmpty { return false }
    var args = point(sender)
    args["paths"] = files
    channel.invokeMethod("drop", arguments: args)
    return true
  }
}
