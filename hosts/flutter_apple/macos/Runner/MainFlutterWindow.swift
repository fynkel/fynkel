import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  private var viewport: FynkelViewportChannel?

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    viewport = FynkelViewportChannel(registrar: flutterViewController.registrar(forPlugin: "FynkelViewport"))

    super.awakeFromNib()
  }
}
