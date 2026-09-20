#if os(iOS)
import Flutter
import UIKit
#else
import Cocoa
import FlutterMacOS
#endif
import GameController

/// The platform half of the input contract (`fynkel_flutter/input`,
/// docs/input.md): gamepads through the GameController framework on both
/// Apple targets (blueprint item 7 names `gilrs`; one framework for iOS
/// and macOS, with the controllers Apple already pairs, is the smaller
/// dependency) and, on macOS, the captured pointer — cursor hidden,
/// movement decoupled from the cursor, deltas from an NSEvent monitor.
///
/// Dart receives batches of `[device, control, phase, x, y]` rows
/// (`InputDevice`/`InputPhase` ordinals, the control's name in the
/// binding grammar) through `events`, one batch per platform callback;
/// `capturePointer {capture}` and `gamepads` are the calls in.
public class FynkelInputPlugin: NSObject, FlutterPlugin {
  private let channel: FlutterMethodChannel
  private var observers: [NSObjectProtocol] = []
  private var pads: [ObjectIdentifier: PadState] = [:]
  #if os(macOS)
  private var monitor: Any?
  private var captured = false
  #endif

  static let gamepad = 2
  static let mouse = 1
  static let down = 0, up = 1, axis = 2, move = 3

  public static func register(with registrar: FlutterPluginRegistrar) {
    #if os(iOS)
    let messenger = registrar.messenger()
    #else
    let messenger = registrar.messenger
    #endif
    let channel = FlutterMethodChannel(name: "fynkel_flutter/input", binaryMessenger: messenger)
    let instance = FynkelInputPlugin(channel: channel)
    registrar.addMethodCallDelegate(instance, channel: channel)
    instance.watchControllers()
  }

  init(channel: FlutterMethodChannel) {
    self.channel = channel
    super.init()
  }

  deinit {
    for o in observers { NotificationCenter.default.removeObserver(o) }
    #if os(macOS)
    setCaptured(false)
    #endif
  }

  public func handle(_ call: FlutterMethodCall, result: @escaping FlutterResult) {
    let args = call.arguments as? [String: Any] ?? [:]
    switch call.method {
    case "capturePointer":
      #if os(macOS)
      setCaptured(args["capture"] as? Bool ?? false)
      result(captured)
      #else
      result(false)
      #endif
    case "gamepads":
      result(GCController.controllers().filter { $0.extendedGamepad != nil }.count)
    default:
      result(FlutterMethodNotImplemented)
    }
  }

  // MARK: gamepads

  /// The last values sent for one controller, so only changes cross.
  private final class PadState {
    var buttons: [String: Bool] = [:]
    var axes: [String: (Float, Float)] = [:]
  }

  private func watchControllers() {
    let center = NotificationCenter.default
    observers.append(center.addObserver(forName: .GCControllerDidConnect, object: nil, queue: .main) { [weak self] n in
      if let c = n.object as? GCController { self?.attach(c) }
    })
    observers.append(center.addObserver(forName: .GCControllerDidDisconnect, object: nil, queue: .main) { [weak self] n in
      if let c = n.object as? GCController { self?.detach(c) }
    })
    for c in GCController.controllers() { attach(c) }
    GCController.startWirelessControllerDiscovery(completionHandler: nil)
  }

  private func attach(_ controller: GCController) {
    guard let pad = controller.extendedGamepad else { return }
    let key = ObjectIdentifier(controller)
    pads[key] = PadState()
    pad.valueChangedHandler = { [weak self] pad, _ in
      guard let self, let state = self.pads[key] else { return }
      self.send(self.diff(pad, state))
    }
  }

  private func detach(_ controller: GCController) {
    let key = ObjectIdentifier(controller)
    guard let state = pads.removeValue(forKey: key) else { return }
    // Whatever it held goes up.
    var rows: [[Any]] = []
    for (name, held) in state.buttons where held {
      rows.append([Self.gamepad, name, Self.up, 0.0, 0.0])
    }
    for (name, _) in state.axes {
      rows.append([Self.gamepad, name, Self.axis, 0.0, 0.0])
    }
    send(rows)
  }

  /// Rows for every button and axis whose value changed since the last
  /// diff. Names follow the binding grammar (GameController's terms).
  private func diff(_ pad: GCExtendedGamepad, _ state: PadState) -> [[Any]] {
    var rows: [[Any]] = []
    func button(_ name: String, _ b: GCControllerButtonInput?) {
      guard let b else { return }
      let now = b.isPressed
      if state.buttons[name] != now {
        state.buttons[name] = now
        rows.append([Self.gamepad, name, now ? Self.down : Self.up, now ? 1.0 : 0.0, 0.0])
      }
    }
    func axis1(_ name: String, _ v: Float) {
      if state.axes[name]?.0 != v {
        state.axes[name] = (v, 0)
        rows.append([Self.gamepad, name, Self.axis, Double(v), 0.0])
      }
    }
    func axis2(_ name: String, _ x: Float, _ y: Float) {
      let last = state.axes[name]
      if last?.0 != x || last?.1 != y {
        state.axes[name] = (x, y)
        rows.append([Self.gamepad, name, Self.axis, Double(x), Double(y)])
      }
    }
    button("south", pad.buttonA)
    button("east", pad.buttonB)
    button("west", pad.buttonX)
    button("north", pad.buttonY)
    button("leftShoulder", pad.leftShoulder)
    button("rightShoulder", pad.rightShoulder)
    button("leftTrigger", pad.leftTrigger)
    button("rightTrigger", pad.rightTrigger)
    button("leftStickButton", pad.leftThumbstickButton)
    button("rightStickButton", pad.rightThumbstickButton)
    button("dpadUp", pad.dpad.up)
    button("dpadDown", pad.dpad.down)
    button("dpadLeft", pad.dpad.left)
    button("dpadRight", pad.dpad.right)
    button("menu", pad.buttonMenu)
    button("options", pad.buttonOptions)
    button("home", pad.buttonHome)
    axis1("leftTrigger", pad.leftTrigger.value)
    axis1("rightTrigger", pad.rightTrigger.value)
    axis1("leftStickX", pad.leftThumbstick.xAxis.value)
    axis1("leftStickY", pad.leftThumbstick.yAxis.value)
    axis1("rightStickX", pad.rightThumbstick.xAxis.value)
    axis1("rightStickY", pad.rightThumbstick.yAxis.value)
    axis2("leftStick", pad.leftThumbstick.xAxis.value, pad.leftThumbstick.yAxis.value)
    axis2("rightStick", pad.rightThumbstick.xAxis.value, pad.rightThumbstick.yAxis.value)
    axis2("dpad", pad.dpad.xAxis.value, pad.dpad.yAxis.value)
    return rows
  }

  private func send(_ rows: [[Any]]) {
    if rows.isEmpty { return }
    if Thread.isMainThread {
      channel.invokeMethod("events", arguments: rows)
    } else {
      DispatchQueue.main.async { self.channel.invokeMethod("events", arguments: rows) }
    }
  }

  // MARK: pointer capture (macOS)

  #if os(macOS)
  private func setCaptured(_ on: Bool) {
    if on == captured { return }
    captured = on
    if on {
      NSCursor.hide()
      CGAssociateMouseAndMouseCursorPosition(0)
      monitor = NSEvent.addLocalMonitorForEvents(
        matching: [.mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged]
      ) { [weak self] event in
        guard let self, self.captured else { return event }
        // Points → physical pixels (what the viewport's own deltas are
        // in); screen y is down, the action's y is up.
        let scale = Double(event.window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1)
        let dx = Double(event.deltaX) * scale, dy = Double(event.deltaY) * scale
        if dx != 0 || dy != 0 {
          self.send([[Self.mouse, "delta", Self.move, dx, -dy]])
        }
        return event
      }
    } else {
      if let m = monitor { NSEvent.removeMonitor(m) }
      monitor = nil
      CGAssociateMouseAndMouseCursorPosition(1)
      NSCursor.unhide()
    }
  }
  #endif
}
