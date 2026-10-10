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

    // tool/measure.dart's full-screen run.
    if ProcessInfo.processInfo.environment["FYNKEL_FULLSCREEN"] == "1" {
      DispatchQueue.main.asyncAfter(deadline: .now() + 1) { self.toggleFullScreen(nil) }
    }
  }
}
