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
    self.setFrame(windowFrame, display: true)

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
