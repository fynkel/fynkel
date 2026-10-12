import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart' hide Transform;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/src/fynkel_view.dart' show channel;
import 'package:fynkel_flutter/src/input_feed.dart' show keyCodes;

void main() {
  late World world;
  late List<String> heard;

  setUp(() {
    world = World();
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 4)),
        components: [Camera()],
      ),
    );
    heard = [];
    var texture = 1;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      channel,
      (call) async => call.method == 'register' ? texture++ : null,
    );
  });
  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, null);
    world.dispose();
  });

  // The test window is 800 × 600; the view, 200 × 100 in its middle, has
  // its top-left corner here.
  const corner = Offset(300, 250);

  Widget app(Widget child) => Directionality(
    textDirection: TextDirection.ltr,
    child: Center(child: SizedBox(width: 200, height: 100, child: child)),
  );

  FynkelView view({bool autofocus = true, double resolutionScale = 1}) =>
      FynkelView(world: world, autofocus: autofocus, resolutionScale: resolutionScale);

  Input input() => world.input;

  /// One frame, which runs the world's input step.
  Future<void> frame(WidgetTester tester) => tester.pump(const Duration(milliseconds: 8));

  ViewPoint at(double x, double y) => ViewPoint(x, y, width: 200, height: 100);

  /// Logs what the input's signals say.
  void listen() {
    final input = world.input;
    input.onFingerDown.listen((f) => heard.add('down ${f.kind.name}'));
    input.onFingerUp.listen((f) => heard.add('up${f.wasCancelled ? ' cancelled' : ''}'));
    input.onTap.listen((f) => heard.add('tap ${f.button.name} x${f.tapCount}'));
    input.onDragStart.listen((f) => heard.add('drag start'));
    input.onDragEnd.listen((f) => heard.add('drag end ${f.totalDelta}'));
    input.onHover.listen((f) => heard.add('hover'));
    input.onPinch.listen((p) => heard.add('pinch ${p.scale}'));
    input.onTwist.listen((p) => heard.add('twist ${p.twist}'));
  }

  testWidgets('a tap reaches the input once, at its place in the view in logical pixels', (tester) async {
    // Positions don't depend on the pixels the view draws.
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(app(view(resolutionScale: 0.5)));
    listen();
    await frame(tester);
    expect((input().width, input().height), (200, 100));

    final taps = <Finger>[];
    input().onTap.listen(taps.add);
    await tester.tapAt(corner + const Offset(50, 20));
    expect(taps, isEmpty, reason: 'events wait for the frame');
    await frame(tester);
    expect(taps.single.position, at(50, 20));
    expect(taps.single.kind, FingerKind.touch);
    expect(heard, ['down touch', 'tap left x1', 'up']);
    expect(input().fingers, isEmpty);
  });

  testWidgets('a drag past the slop, across frames; touches are fingers of their own', (tester) async {
    await tester.pumpWidget(app(view()));
    listen();
    final first = await tester.startGesture(corner + const Offset(20, 50), pointer: 1);
    final second = await tester.startGesture(corner + const Offset(150, 80), pointer: 2);
    await frame(tester);
    expect(input().fingers.map((f) => f.position), [at(20, 50), at(150, 80)]);

    for (var i = 0; i < 10; i++) {
      await first.moveBy(const Offset(5, -1));
      await frame(tester);
    }
    expect(input().fingers.first.isDragging, isTrue);
    await first.up();
    await second.up();
    await frame(tester);
    expect(heard.where((h) => h.startsWith('drag')), ['drag start', 'drag end Vec2(50.0, -10.0)']);
    expect(heard.where((h) => h.startsWith('tap')), ['tap left x1'], reason: 'the second, which stayed put, tapped');
  });

  testWidgets('the mouse is one finger, hovering or pressed, with its buttons', (tester) async {
    await tester.pumpWidget(app(view()));
    listen();
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: corner + const Offset(10, 10));
    await mouse.moveTo(corner + const Offset(60, 10));
    await frame(tester);
    final finger = input().fingers.single;
    expect(finger.kind, FingerKind.mouse);
    expect(finger.isPressed, isFalse);
    expect(input().mousePosition, at(60, 10));
    expect(heard, ['hover']);

    heard.clear();
    await mouse.down(corner + const Offset(60, 10));
    await mouse.up();
    await frame(tester);
    expect(heard, ['down mouse', 'tap left x1', 'up']);
    expect(input().fingers.single, same(finger), reason: 'the same finger, still over the view');

    final right = await tester.startGesture(
      corner + const Offset(60, 10),
      kind: PointerDeviceKind.mouse,
      buttons: kSecondaryButton,
    );
    await frame(tester);
    expect(input().isPressed(MouseButton.right), isTrue);
    await right.moveBy(const Offset(4, 0));
    await frame(tester);
    expect(heard.last, 'drag start', reason: "a mouse's slop is 1 pixel");
    await right.up();
    await frame(tester);
    expect(input().wasReleased(MouseButton.right), isTrue);

    await mouse.moveTo(const Offset(5, 5));
    await frame(tester);
    expect(input().fingers, isEmpty, reason: 'it left the view');
    await mouse.removePointer();
  });

  testWidgets('the wheel and the trackpad scroll the game, not the list around it; a trackpad pinches and twists', (
    tester,
  ) async {
    final list = ScrollController();
    addTearDown(list.dispose);
    await tester.pumpWidget(
      Directionality(
        textDirection: TextDirection.ltr,
        child: ListView(
          controller: list,
          children: [
            const SizedBox(height: 250),
            SizedBox(height: 100, child: view()),
            const SizedBox(height: 2000),
          ],
        ),
      ),
    );
    listen();
    final inside = const Offset(400, 300);
    await tester.sendEventToBinding(PointerScrollEvent(position: inside, scrollDelta: const Offset(0, 30)));
    await tester.sendEventToBinding(PointerScrollEvent(position: inside, scrollDelta: const Offset(0, 12)));
    await frame(tester);
    expect(input().scroll, Vec2(0, 42));
    expect(list.offset, 0, reason: 'the view claimed the wheel');
    await frame(tester);
    expect(input().scroll, Vec2.zero);

    final trackpad = await tester.createGesture(kind: PointerDeviceKind.trackpad);
    await trackpad.panZoomStart(inside);
    await trackpad.panZoomUpdate(inside, pan: const Offset(0, -20));
    await frame(tester);
    expect(input().scroll, Vec2(0, 20), reason: 'fingers up scroll down');
    await trackpad.panZoomUpdate(inside, pan: const Offset(0, -20), scale: 2);
    await trackpad.panZoomUpdate(inside, pan: const Offset(0, -20), scale: 3, rotation: 0.5);
    await frame(tester);
    expect(heard, ['pinch 3.0', 'twist 0.5'], reason: "one frame's worth");
    await trackpad.panZoomEnd();
  });

  testWidgets('keys go by where they are, held, pressed and released by frame, and let go when focus goes', (
    tester,
  ) async {
    final other = FocusNode();
    addTearDown(other.dispose);
    // What reaches the widgets around the view: shortcuts, focus moves.
    final around = <KeyEvent>[];
    await tester.pumpWidget(
      app(
        Focus(
          onKeyEvent: (_, event) {
            around.add(event);
            return KeyEventResult.ignored;
          },
          child: Column(
            children: [
              Expanded(child: view()),
              Focus(focusNode: other, child: const SizedBox(height: 1)),
            ],
          ),
        ),
      ),
    );
    await frame(tester);

    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyW);
    await frame(tester);
    expect(input().isPressed(KeyCode.w), isTrue);
    expect(input().wasPressed(KeyCode.w), isTrue);
    expect(input().axis2('move'), Vec2(0, 1));

    await tester.sendKeyRepeatEvent(LogicalKeyboardKey.keyW);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.arrowDown);
    await frame(tester);
    expect(input().wasPressed(KeyCode.w), isFalse, reason: 'a repeat is no new press');
    expect(input().wasPressed(KeyCode.arrowDown), isTrue);
    expect(around, isEmpty, reason: 'the view keeps the keys it has focus for');

    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyW);
    await frame(tester);
    expect(input().wasReleased(KeyCode.w), isTrue);
    expect(input().isPressed(KeyCode.arrowDown), isTrue);

    other.requestFocus();
    await frame(tester);
    expect(input().wasReleased(KeyCode.arrowDown), isTrue, reason: 'its key-up will never come here');
    expect(input().isPressed(KeyCode.arrowDown), isFalse);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.arrowDown);
  });

  testWidgets('keys the game does not use reach Flutter\'s shortcuts, and every key does while paused', (tester) async {
    final shortcuts = <String>[];
    await tester.pumpWidget(
      app(
        CallbackShortcuts(
          bindings: {
            const SingleActivator(LogicalKeyboardKey.escape): () => shortcuts.add('escape'),
            const SingleActivator(LogicalKeyboardKey.keyW): () => shortcuts.add('w'),
          },
          child: view(),
        ),
      ),
    );
    final keys = <String>[];
    input().onKeyDown.listen((k) => keys.add(k.key.name));
    await frame(tester);

    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyW);
    await frame(tester);
    expect(shortcuts, ['escape'], reason: "the game binds W ('move'), not Escape");
    expect(keys, ['escape', 'w'], reason: 'the game hears both');

    world.paused = true;
    await tester.sendKeyEvent(LogicalKeyboardKey.keyW);
    await frame(tester);
    expect(shortcuts, ['escape', 'w'], reason: 'a pause menu gets every key');
    world.paused = false;
  });

  testWidgets('a Listenable\'s changed is a signal: an object hears it with on, until destroyed', (tester) async {
    final volume = ValueNotifier(0.5);
    addTearDown(volume.dispose);
    expect(volume.changed, same(volume.changed), reason: 'one signal per Listenable');
    final ear = world.add(GameObject());
    ear.on(volume.changed, (_) => heard.add('volume ${volume.value}'));
    volume.value = 0.8;
    expect(heard, ['volume 0.8']);
    ear.destroy();
    world.update(0);
    volume.value = 0.2;
    expect(heard, ['volume 0.8']);
  });

  test('every KeyCode has a Flutter key', () {
    expect(keyCodes.values.toSet(), KeyCode.values.toSet());
  });

  testWidgets('without autofocus, keys reach the view once it is tapped', (tester) async {
    await tester.pumpWidget(app(view(autofocus: false)));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.space);
    await frame(tester);
    expect(input().isPressed(KeyCode.space), isFalse);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.space);

    await tester.tapAt(corner + const Offset(5, 5));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.space);
    await frame(tester);
    expect(input().wasPressed(KeyCode.space), isTrue);
    expect(input().action('jump').wasPressed, isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.space);
  });

  testWidgets('gesture widgets around the view still get their gestures', (tester) async {
    var taps = 0;
    var panned = Offset.zero;
    await tester.pumpWidget(
      app(GestureDetector(onTap: () => taps++, onPanUpdate: (d) => panned += d.delta, child: view())),
    );
    listen();
    await tester.tapAt(corner + const Offset(50, 50));
    expect(taps, 1);
    await frame(tester);
    expect(heard, contains('tap left x1'), reason: 'and the view too');

    await tester.dragFrom(corner + const Offset(50, 50), const Offset(60, 0));
    expect(panned.dx, greaterThan(0));
    await frame(tester);
  });

  testWidgets('a widget over the view keeps the pointers it takes', (tester) async {
    var pressed = 0;
    await tester.pumpWidget(
      app(
        Stack(
          children: [
            view(),
            Positioned(
              left: 0,
              top: 0,
              width: 50,
              height: 50,
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => pressed++,
                child: const SizedBox.expand(),
              ),
            ),
          ],
        ),
      ),
    );
    listen();
    await tester.tapAt(corner + const Offset(20, 20));
    await frame(tester);
    expect(pressed, 1);
    expect(heard, isEmpty, reason: 'the button over the view took it');
    await tester.tapAt(corner + const Offset(120, 60));
    await frame(tester);
    expect(heard, contains('tap left x1'));
  });

  testWidgets("the view runs the world's loop: a tap clicks the object under it", (tester) async {
    final box = world.add(_Box());
    await tester.pumpWidget(app(view()));
    await frame(tester);
    expect(box.started, isTrue);
    expect(box.updates, greaterThan(0));

    await tester.tapAt(corner + const Offset(100, 50));
    await frame(tester);
    expect(box.clicks, 1);
    expect(box.where!.z, closeTo(0.5, 1e-5), reason: 'its front face, toward the camera at z 4');
    await tester.tapAt(corner + const Offset(150, 50));
    await frame(tester);
    expect(box.clicks, 1, reason: 'beside it');
  });
}

class _Box extends GameObject {
  _Box() : super(components: [MeshRenderer(Mesh.cube(), Material())]);

  var started = false;
  var updates = 0;
  var clicks = 0;
  Vec3? where;

  @override
  void onStart() {
    super.onStart();
    started = true;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    updates++;
  }

  @override
  bool onClick(Finger finger) {
    clicks++;
    where = finger.over!.point;
    return true;
  }
}
