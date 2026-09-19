import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  private var viewport: FynkelViewportChannel?

  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    // Same host glue as macos/Runner/MainFlutterWindow.swift.
    guard let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "FynkelViewport") else {
      fatalError("FynkelViewport: no plugin registrar")
    }
    viewport = FynkelViewportChannel(registrar: registrar)
  }
}
