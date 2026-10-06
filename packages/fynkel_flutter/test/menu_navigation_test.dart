// Menus by controller and keyboard (ADR-0062 amendment 1, step 1): while
// the app says a menu is open, the game's input gets nothing and the
// controller drives Flutter's focus — the D-pad and the left stick as
// arrow keys (a held direction repeats), south as `gameButtonA` (press),
// east/menu/options as `gameButtonB`/`gameButtonStart`/`gameButtonSelect`.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/gameplay.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  const viewportChannel = MethodChannel('fynkel_flutter/viewport');

  setUp(() {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(viewportChannel, (call) async => call.method == 'registerTexture' ? 42 : null);
    messenger.setMockMethodCallHandler(FynkelInputChannel.channel, (call) async => null);
  });
  tearDown(() {
    final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
    messenger.setMockMethodCallHandler(viewportChannel, null);
    messenger.setMockMethodCallHandler(FynkelInputChannel.channel, null);
  });

  /// One platform batch of gamepad rows: `[control, phase, x, y]`.
  Future<void> pad(List<(String, InputPhase, double, double)> rows) async {
    final message = FynkelInputChannel.channel.codec.encodeMethodCall(
      MethodCall('events', [
        for (final (control, phase, x, y) in rows) [InputDevice.gamepad.index, control, phase.index, x, y],
      ]),
    );
    await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.handlePlatformMessage(
      FynkelInputChannel.channel.name,
      message,
      (_) {},
    );
  }

  Future<void> press(String button) => pad([(button, InputPhase.down, 1, 0), (button, InputPhase.up, 0, 0)]);

  testWidgets('an open menu takes the controller and the keyboard; the game gets nothing', (tester) async {
    final world = World.create();
    addTearDown(world.dispose);
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    final capture = FynkelInputCapture();
    final view = GlobalKey<FynkelGameViewState>();
    final pressed = <String>[];
    final appKeys = <LogicalKeyboardKey>[];
    var volume = 0.5;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: FynkelGameView(
            key: view,
            world: world,
            controller: controller,
            input: capture,
            onKeyEvent: (_, e) {
              if (e is KeyDownEvent) appKeys.add(e.logicalKey);
              return KeyEventResult.ignored;
            },
            overlay: StatefulBuilder(
              builder: (context, setState) => Center(
                child: SizedBox(
                  width: 300,
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      FilledButton(
                        autofocus: true,
                        onPressed: () => pressed.add('Resume'),
                        child: const Text('Resume'),
                      ),
                      FilledButton(onPressed: () => pressed.add('Settings'), child: const Text('Settings')),
                      Slider(value: volume, onChanged: (v) => setState(() => volume = v)),
                      FilledButton(onPressed: () => pressed.add('Quit'), child: const Text('Quit')),
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pump();
    final state = view.currentState!;
    // The loop stays paused so nothing takes the capture's events: what
    // is pending is what the game would get.
    state.setPaused(true);
    capture.take(0);

    String focused() {
      final w = FocusManager.instance.primaryFocus?.context?.widget;
      final button = FocusManager.instance.primaryFocus?.context?.findAncestorWidgetOfExactType<FilledButton>();
      final text = (button?.child as Text?)?.data;
      return text ??
          (FocusManager.instance.primaryFocus?.context?.findAncestorWidgetOfExactType<Slider>() != null
              ? 'Slider'
              : '$w');
    }

    expect(state.menuOpen, isFalse);
    state.setMenuOpen(true);
    expect(state.menuOpen, isTrue);
    expect(capture.take(0), const [InputEvent.reset()], reason: 'the game sees everything released once');
    await tester.pump();
    expect(focused(), 'Resume');

    // The D-pad moves the focus, south presses.
    await press('dpadDown');
    await tester.pump();
    expect(focused(), 'Settings');
    await press('south');
    await tester.pump();
    expect(pressed, ['Settings']);

    // The left stick: one move per push, then repeats while held.
    await pad([('leftStick', InputPhase.axis, 0, -0.9)]);
    await tester.pump();
    expect(focused(), 'Slider');
    await pad([('leftStick', InputPhase.axis, 0.05, -0.85)]);
    await tester.pump(const Duration(milliseconds: 100));
    expect(focused(), 'Slider', reason: 'still the same push: no second move yet');
    // On the slider, left/right adjust it, as the arrow keys do.
    await pad([('leftStick', InputPhase.axis, 0, 0)]);
    await press('dpadRight');
    await tester.pump();
    expect(volume, greaterThan(0.5));
    // Held down: a repeat after the delay moves on.
    await pad([('dpadDown', InputPhase.down, 1, 0)]);
    await tester.pump();
    expect(focused(), 'Quit');
    await pad([('dpadDown', InputPhase.up, 0, 0)]);
    await pad([('dpadUp', InputPhase.down, 1, 0)]);
    await tester.pump();
    expect(focused(), 'Slider');
    await tester.pump(const Duration(milliseconds: 450));
    await tester.pump(const Duration(milliseconds: 150));
    expect(focused(), anyOf('Settings', 'Resume'), reason: 'a held direction repeats');
    await pad([('dpadUp', InputPhase.up, 0, 0)]);
    await tester.pump(const Duration(milliseconds: 500));

    // Back and Start reach the app as game buttons.
    await press('east');
    await press('menu');
    await press('options');
    expect(
      appKeys,
      containsAllInOrder([
        LogicalKeyboardKey.gameButtonB,
        LogicalKeyboardKey.gameButtonStart,
        LogicalKeyboardKey.gameButtonSelect,
      ]),
    );

    // The keyboard: the game gets no keys, the app still sees them.
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyW, physicalKey: PhysicalKeyboardKey.keyW);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyW, physicalKey: PhysicalKeyboardKey.keyW);
    expect(appKeys.last, LogicalKeyboardKey.keyW);
    expect(capture.take(0), isEmpty, reason: 'nothing reached the game while the menu was open');

    // Closed: the controller is the game's again.
    state.setMenuOpen(false);
    expect(capture.take(0), const [InputEvent.reset()]);
    await press('south');
    expect(capture.take(0), const [
      InputEvent.down(InputDevice.gamepad, 'south'),
      InputEvent.up(InputDevice.gamepad, 'south'),
    ]);
    final before = List.of(pressed);
    await tester.pump();
    expect(pressed, before, reason: 'south no longer presses the focused button');

    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  });
}
