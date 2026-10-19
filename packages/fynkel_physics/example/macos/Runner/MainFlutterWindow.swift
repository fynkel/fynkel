import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)

    super.awakeFromNib()

    // tool/measure.dart's runs: above the other windows and on every
    // desktop, as macOS stops drawing a window that's covered.
    let environment = ProcessInfo.processInfo.environment
    let measured = environment["FYNKEL_STATS"] == "1" || environment["FYNKEL_CHECK"] != nil
    if measured && environment["FYNKEL_FULLSCREEN"] != "1" {
      self.level = .floating
      self.collectionBehavior.formUnion([.canJoinAllSpaces, .fullScreenAuxiliary])
    }
    // tool/measure.dart's full-screen run.
    if environment["FYNKEL_FULLSCREEN"] == "1" {
      DispatchQueue.main.asyncAfter(deadline: .now() + 1) { self.toggleFullScreen(nil) }
    }
  }
}
