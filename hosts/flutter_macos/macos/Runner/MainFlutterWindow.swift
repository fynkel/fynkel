import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  private var fynkelM0: FynkelM0Channel?
  private var pathC: FynkelPathCHost?

  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)

    RegisterGeneratedPlugins(registry: flutterViewController)
    fynkelM0 = FynkelM0Channel(registrar: flutterViewController.registrar(forPlugin: "FynkelM0"))
    // FYNKEL_M0_MODE=c: transparent Flutter over an engine-owned CAMetalLayer
    // (Path C). Otherwise the plain Flutter window used by Path A (Texture
    // widget) and the no-viewport control. Dart reads the same variable.
    if ProcessInfo.processInfo.environment["FYNKEL_M0_MODE"] == "c" {
      let host = FynkelPathCHost(flutterViewController: flutterViewController)
      host.install(registrar: flutterViewController.registrar(forPlugin: "FynkelPathC"))
      pathC = host
    }

    super.awakeFromNib()
  }
}
