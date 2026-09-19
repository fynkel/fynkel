import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  private var fynkelM0: FynkelM0Channel?

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    fynkelM0 = FynkelM0Channel(registrar: flutterViewController.registrar(forPlugin: "FynkelM0"))

    super.awakeFromNib()
  }
}
