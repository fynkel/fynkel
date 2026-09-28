// The host half of the input contract: `FynkelInputCapture` turns keys,
// mouse buttons, movement and the wheel into raw events; the platform
// channel's gamepad batches land in it; a focus loss resets; the touch
// overlay's stick and buttons drive the same controls through the game
// view into a real world.

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const viewportChannel = MethodChannel('fynkel_flutter/viewport');
  final capturePointerCalls = <bool>[];

  setUp(() {
    capturePointerCalls.clear();
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(viewportChannel, (call) async {
      switch (call.method) {
        case 'registerTexture':
          return 42;
        default:
          return null;
      }
    });
    messenger.setMockMethodCallHandler(FynkelInputChannel.channel, (call) async {
      switch (call.method) {
        case 'capturePointer':
          capturePointerCalls.add(call.arguments['capture'] as bool);
          return call.arguments['capture'] as bool;
        case 'gamepads':
          return 1;
        default:
          return null;
      }
    });
  });
  tearDown(() {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(viewportChannel, null);
    messenger.setMockMethodCallHandler(FynkelInputChannel.channel, null);
  });

  test('keys: physical keys by grammar name, repeats are not edges', () {
    final c = FynkelInputCapture();
    const down = KeyDownEvent(
      physicalKey: PhysicalKeyboardKey.keyW,
      logicalKey: LogicalKeyboardKey.keyZ,
      timeStamp: Duration.zero,
    );
    expect(c.handleKey(down), isTrue);
    expect(
      c.handleKey(
        const KeyRepeatEvent(
          physicalKey: PhysicalKeyboardKey.keyW,
          logicalKey: LogicalKeyboardKey.keyZ,
          timeStamp: Duration.zero,
        ),
      ),
      isTrue,
    );
    expect(
      c.handleKey(
        const KeyUpEvent(
          physicalKey: PhysicalKeyboardKey.keyW,
          logicalKey: LogicalKeyboardKey.keyZ,
          timeStamp: Duration.zero,
        ),
      ),
      isTrue,
    );
    expect(
      c.handleKey(
        const KeyDownEvent(
          physicalKey: PhysicalKeyboardKey.mediaPlayPause,
          logicalKey: LogicalKeyboardKey.mediaPlayPause,
          timeStamp: Duration.zero,
        ),
      ),
      isFalse,
      reason: 'not a key the grammar names',
    );
    expect(c.take(1 / 60), const [
      InputEvent.down(InputDevice.keyboard, 'W'),
      InputEvent.up(InputDevice.keyboard, 'W'),
    ]);
    expect(c.take(1 / 60), isEmpty);
    expect(FynkelInputCapture.keyName(PhysicalKeyboardKey.space), 'Space');
    expect(FynkelInputCapture.keyName(PhysicalKeyboardKey.arrowLeft), 'ArrowLeft');
    expect(FynkelInputCapture.keyName(PhysicalKeyboardKey.f12), 'F12');
  });

  test('trackpad: two-finger scrolling is the mouse wheel, as Unity (foundation 4.6)', () {
    final c = FynkelInputCapture();
    const trackpad = PointerDeviceKind.trackpad;
    // macOS sends a trackpad scroll as pan/zoom events, not as scrolls:
    // fingers moving down (natural scrolling) is the same as a wheel
    // turned away from the user, a scrollDelta of -dy.
    c.handlePointer(const PointerPanZoomStartEvent(), 2);
    c.handlePointer(const PointerPanZoomUpdateEvent(pan: Offset(0, 6), panDelta: Offset(0, 6)), 2);
    c.handlePointer(const PointerPanZoomUpdateEvent(pan: Offset(0, 2), panDelta: Offset(0, -4)), 2);
    c.handlePointer(const PointerPanZoomUpdateEvent(pan: Offset(3, 2), panDelta: Offset(3, 0)), 2);
    c.handlePointer(const PointerPanZoomEndEvent(), 2);
    c.handlePointer(const PointerScrollEvent(kind: trackpad, scrollDelta: Offset(0, -10)), 2);
    expect(c.take(1 / 60), const [
      InputEvent.move(InputDevice.mouse, 'wheel', 6),
      InputEvent.move(InputDevice.mouse, 'wheel', -4),
      InputEvent.move(InputDevice.mouse, 'wheel', 10),
    ]);
  });

  test('mouse: button edges from the button mask, deltas in physical pixels (y up), the wheel', () {
    final c = FynkelInputCapture();
    const mouse = PointerDeviceKind.mouse;
    c.handlePointer(const PointerDownEvent(kind: mouse, buttons: kPrimaryMouseButton), 2);
    c.handlePointer(
      const PointerMoveEvent(kind: mouse, buttons: kPrimaryMouseButton | kSecondaryMouseButton, delta: Offset(3, -2)),
      2,
    );
    c.handlePointer(const PointerUpEvent(kind: mouse, buttons: 0), 2);
    c.handlePointer(const PointerHoverEvent(kind: mouse, delta: Offset(0, 1)), 2);
    c.handlePointer(const PointerScrollEvent(kind: mouse, scrollDelta: Offset(0, -10)), 2);
    c.handlePointer(const PointerDownEvent(kind: PointerDeviceKind.touch), 2);
    expect(c.take(1 / 60), const [
      InputEvent.down(InputDevice.mouse, 'left'),
      InputEvent.down(InputDevice.mouse, 'right'),
      InputEvent.move(InputDevice.mouse, 'delta', 6, 4),
      InputEvent.up(InputDevice.mouse, 'left'),
      InputEvent.up(InputDevice.mouse, 'right'),
      InputEvent.move(InputDevice.mouse, 'delta', 0, -2),
      InputEvent.move(InputDevice.mouse, 'wheel', 10),
    ]);
    // Captured: the platform's deltas replace Flutter's.
    c.pointerCaptured = true;
    c.handlePointer(const PointerHoverEvent(kind: mouse, delta: Offset(5, 5)), 2);
    expect(c.take(1 / 60), isEmpty);
    c.reset();
    expect(c.take(1 / 60), const [InputEvent.reset()]);
  });

  test('a recorder sees every taken frame', () {
    final rec = InputRecorder();
    final c = FynkelInputCapture(recorder: rec);
    c.add(const InputEvent.down(InputDevice.keyboard, 'A'));
    c.take(0.016);
    c.take(0.017);
    expect(rec.length, 2);
    expect(rec.recording.frames[0].events, const [InputEvent.down(InputDevice.keyboard, 'A')]);
    expect(rec.recording.frames[1].dt, 0.017);
    expect(rec.recording.frames[1].events, isEmpty);
  });

  testWidgets('the platform channel\'s batches reach an attached capture', (tester) async {
    final c = FynkelInputCapture();
    FynkelInputChannel.attach(c);
    addTearDown(() => FynkelInputChannel.detach(c));
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    final message = FynkelInputChannel.channel.codec.encodeMethodCall(
      const MethodCall('events', [
        [2, 'south', 0, 1.0, 0.0],
        [2, 'leftStick', 2, -0.5, 0.25],
        [1, 'delta', 3, 4.0, -6.0],
      ]),
    );
    await messenger.handlePlatformMessage(FynkelInputChannel.channel.name, message, (_) {});
    expect(c.take(1 / 60), const [
      InputEvent.down(InputDevice.gamepad, 'south'),
      InputEvent.axis(InputDevice.gamepad, 'leftStick', -0.5, 0.25),
      InputEvent.move(InputDevice.mouse, 'delta', 4, -6),
    ]);
    expect(await FynkelInputChannel.gamepadCount(), 1);
  });

  testWidgets('the game view feeds keys, focus loss and the touch overlay into the world', (tester) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    final world = World.create();
    addTearDown(world.dispose);
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    final capture = FynkelInputCapture();
    final key = GlobalKey<FynkelGameViewState>();
    var frames = 0;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: FynkelGameView(
            key: key,
            world: world,
            controller: controller,
            input: capture,
            onFrame: (dt, plan) => frames++,
            overlay: TouchOverlay(
              input: capture,
              visible: true,
              controls: const [OverlayStick('stick', radius: 50, knobRadius: 20), OverlayButton('jump', radius: 30)],
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    expect(key.currentState!.hasFocus, isTrue, reason: 'autofocus');

    // A key down and up through the focus.
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyW, physicalKey: PhysicalKeyboardKey.keyW);
    expect(capture.pending, const [InputEvent.down(InputDevice.keyboard, 'W')]);
    await tester.pump(const Duration(milliseconds: 16));
    expect(capture.pending, isEmpty, reason: 'the frame took it');
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyW, physicalKey: PhysicalKeyboardKey.keyW);
    expect(capture.pending, const [InputEvent.up(InputDevice.keyboard, 'W')]);
    await tester.pump(const Duration(milliseconds: 16));

    // The overlay's button and stick.
    final jump = find.text('jump');
    expect(jump, findsOneWidget);
    final g = await tester.startGesture(tester.getCenter(jump));
    expect(capture.pending, const [InputEvent.down(InputDevice.touch, 'overlay/jump')]);
    await g.up();
    expect(capture.pending, const [
      InputEvent.down(InputDevice.touch, 'overlay/jump'),
      InputEvent.up(InputDevice.touch, 'overlay/jump'),
    ]);
    await tester.pump(const Duration(milliseconds: 16));
    final stickBox = find.byWidgetPredicate((w) => w is SizedBox && w.width == 100 && w.height == 100);
    expect(stickBox, findsOneWidget);
    final centre = tester.getCenter(stickBox);
    final s = await tester.startGesture(centre);
    await s.moveTo(centre + const Offset(0, -30)); // up, inside the ring
    await s.moveTo(centre + const Offset(100, 0)); // far right: clamped to 1
    expect(capture.pending, [
      const InputEvent.axis(InputDevice.touch, 'overlay/stick', 0, 0),
      const InputEvent.axis(InputDevice.touch, 'overlay/stick', 0, 0.6),
      const InputEvent.axis(InputDevice.touch, 'overlay/stick', 1, 0),
    ]);
    await s.up();
    expect(capture.pending.last, const InputEvent.axis(InputDevice.touch, 'overlay/stick', 0, 0));
    await tester.pump(const Duration(milliseconds: 16));

    // Pointer capture goes through the channel; a focus loss releases it
    // and resets.
    expect(await key.currentState!.capturePointer(true), isTrue);
    expect(capture.pointerCaptured, isTrue);
    expect(capturePointerCalls, [true]);
    key.currentState!.setPaused(true);
    expect(capture.pending, const [InputEvent.reset()]);
    await tester.pump();
    expect(capturePointerCalls, [true, false]);
    expect(capture.pointerCaptured, isFalse);
    key.currentState!.setPaused(false);

    // Focus taken by something else: keys no longer reach the capture; a
    // click on the viewport claims it back (the macOS beep otherwise).
    FocusManager.instance.primaryFocus?.unfocus();
    await tester.pump();
    expect(key.currentState!.hasFocus, isFalse);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyW, physicalKey: PhysicalKeyboardKey.keyW);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyW, physicalKey: PhysicalKeyboardKey.keyW);
    expect(capture.pending, isEmpty, reason: 'unfocused: the key went nowhere');
    final click = await tester.startGesture(
      tester.getCenter(find.byType(FynkelViewport)),
      kind: PointerDeviceKind.mouse,
    );
    await click.up();
    await tester.pump();
    expect(key.currentState!.hasFocus, isTrue, reason: 'the click took the keyboard back');
    capture.take(1 / 60); // the mouse button's own events
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyW, physicalKey: PhysicalKeyboardKey.keyW);
    expect(capture.pending, const [InputEvent.down(InputDevice.keyboard, 'W')]);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyW, physicalKey: PhysicalKeyboardKey.keyW);

    await tester.pumpWidget(const SizedBox());
    expect(frames, greaterThan(3));
    expect(world.isPoisoned, isFalse);
  });

  testWidgets('the overlay is hidden off touch platforms and built from bound actions', (tester) async {
    final capture = FynkelInputCapture();
    final actions = [
      const BoundAction(name: 'move', type: 'axis2d', bindings: ['keyboard:WASD', 'touch:overlay/stick']),
      const BoundAction(name: 'jump', type: 'button', bindings: ['keyboard:Space', 'touch:overlay/jump']),
      const BoundAction(name: 'pause', type: 'button', bindings: ['keyboard:Escape']),
    ];
    final overlay = TouchOverlay.fromActions(actions, input: capture);
    expect(overlay.controls.map((c) => c.name), ['stick', 'jump']);
    expect(overlay.controls[0], isA<OverlayStick>());
    expect(overlay.controls[1], isA<OverlayButton>());
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    await tester.pumpWidget(MaterialApp(home: overlay));
    expect(find.text('jump'), findsNothing);
    debugDefaultTargetPlatformOverride = TargetPlatform.iOS;
    await tester.pumpWidget(MaterialApp(home: TouchOverlay.fromActions(actions, input: capture)));
    expect(find.text('jump'), findsOneWidget);
    debugDefaultTargetPlatformOverride = null;
  });
}
