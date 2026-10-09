import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart' hide Transform;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/src/fynkel_view.dart' show channel;

void main() {
  late World world;
  late List<Input> frames;

  setUp(() {
    world = World();
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 4)),
        components: [Camera()],
      ),
    );
    frames = [];
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

  FynkelView view({bool autofocus = true, double resolutionScale = 1}) => FynkelView(
    world: world,
    autofocus: autofocus,
    resolutionScale: resolutionScale,
    onFrame: (_, input) => frames.add(input),
  );

  /// One frame; returns its input.
  Future<Input> frame(WidgetTester tester) async {
    await tester.pump(const Duration(milliseconds: 8));
    return frames.last;
  }

  ViewPoint at(double x, double y) => ViewPoint(x, y, width: 200, height: 100);

  testWidgets('a tap shows once, at its place in the view in logical pixels', (tester) async {
    // Positions don't depend on the pixels the view draws.
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(app(view(resolutionScale: 0.5)));
    expect((await frame(tester)).tapped, isNull);
    expect((frames.last.width, frames.last.height), (200, 100));

    await tester.tapAt(corner + const Offset(50, 20));
    final input = await frame(tester);
    expect(input.tapped, at(50, 20));
    expect(input.taps, [at(50, 20)]);
    expect(input.drag, Vec2.zero);
    expect(input.pointers, isEmpty, reason: 'up again');
    expect((await frame(tester)).tapped, isNull, reason: 'only in the frame it happened');
  });

  testWidgets('a drag adds up to the whole movement, starting once it is past the tap slop', (tester) async {
    await tester.pumpWidget(app(view()));
    final finger = await tester.startGesture(corner + const Offset(20, 50));
    expect((await frame(tester)).pointers.values, [at(20, 50)]);

    final drags = <Vec2>[];
    for (var i = 0; i < 10; i++) {
      await finger.moveBy(const Offset(5, -1));
      drags.add((await frame(tester)).drag);
    }
    // Touch slop is 18: the first three moves (15.3 away) are still a
    // possible tap; the fourth (20.4) counts all four.
    expect(drags.take(3), everyElement(Vec2.zero));
    expect(drags[3], Vec2(20, -4));
    expect(drags.skip(4), everyElement(Vec2(5, -1)));
    expect(frames.last.pointers.values, [at(70, 40)]);

    await finger.up();
    final last = await frame(tester);
    expect(last.tapped, isNull, reason: 'it moved too far to be a tap');
    expect(last.pointers, isEmpty);
  });

  testWidgets('several moves in one frame make one drag, and a tap may move a little', (tester) async {
    await tester.pumpWidget(app(view()));
    final finger = await tester.startGesture(corner + const Offset(100, 50));
    await finger.moveBy(const Offset(10, 0));
    await finger.moveBy(const Offset(0, 10));
    await finger.up();
    expect((await frame(tester)).tapped, at(110, 60), reason: '14 away, within the slop; where it came up');

    final again = await tester.startGesture(corner + const Offset(100, 50));
    await again.moveBy(const Offset(15, 0));
    await again.moveBy(const Offset(15, 0));
    await again.moveBy(const Offset(15, 0));
    expect((await frame(tester)).drag, Vec2(45, 0));
    await again.up();
  });

  testWidgets('only the first finger down drags; the next takes over when it lifts', (tester) async {
    await tester.pumpWidget(app(view()));
    final first = await tester.startGesture(corner + const Offset(20, 20), pointer: 1);
    final second = await tester.startGesture(corner + const Offset(150, 80), pointer: 2);
    await second.moveBy(const Offset(-40, 0));
    final both = await frame(tester);
    expect(both.drag, Vec2.zero);
    expect(both.pointers, {1: at(20, 20), 2: at(110, 80)});

    await first.moveBy(const Offset(0, 30));
    expect((await frame(tester)).drag, Vec2(0, 30));

    await first.up();
    await frame(tester);
    await second.moveBy(const Offset(-3, 0));
    expect((await frame(tester)).drag, Vec2(-3, 0), reason: 'the second is first now, and already dragging');
    await second.up();
    expect((await frame(tester)).taps, isEmpty, reason: 'neither was a tap');
  });

  testWidgets('a mouse taps with its primary button only, and drags with any', (tester) async {
    await tester.pumpWidget(app(view()));
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer(location: corner + const Offset(10, 10));
    await mouse.moveTo(corner + const Offset(60, 10));
    final hovered = await frame(tester);
    expect(hovered.drag, Vec2.zero, reason: 'moving without a button is no drag');
    expect(hovered.pointers, isEmpty);

    await mouse.down(corner + const Offset(60, 10));
    await mouse.up();
    expect((await frame(tester)).tapped, at(60, 10));

    final click = await tester.startGesture(
      corner + const Offset(60, 10),
      kind: PointerDeviceKind.mouse,
      buttons: kSecondaryButton,
    );
    await click.up();
    expect((await frame(tester)).tapped, isNull, reason: 'a right click is no tap');

    final right = await tester.startGesture(
      corner + const Offset(60, 10),
      kind: PointerDeviceKind.mouse,
      buttons: kSecondaryButton,
    );
    expect((await frame(tester)).pointers, hasLength(1));
    await right.moveBy(const Offset(4, 0));
    expect((await frame(tester)).drag, Vec2(4, 0), reason: "a mouse's slop is 1 pixel");
    await right.up();
    expect((await frame(tester)).tapped, isNull);
    await mouse.removePointer();
  });

  testWidgets('the wheel and the trackpad scroll the game, not the list around it', (tester) async {
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
    final inside = const Offset(400, 300);
    await tester.sendEventToBinding(PointerScrollEvent(position: inside, scrollDelta: const Offset(0, 30)));
    await tester.sendEventToBinding(PointerScrollEvent(position: inside, scrollDelta: const Offset(0, 12)));
    expect((await frame(tester)).scroll, Vec2(0, 42));
    expect(list.offset, 0, reason: 'the view claimed the wheel');
    expect((await frame(tester)).scroll, Vec2.zero);

    final trackpad = await tester.createGesture(kind: PointerDeviceKind.trackpad);
    await trackpad.panZoomStart(inside);
    await trackpad.panZoomUpdate(inside, pan: const Offset(0, -20));
    expect((await frame(tester)).scroll, Vec2(0, 20), reason: 'fingers up scroll down');
    await trackpad.panZoomEnd();
  });

  testWidgets('keys are held, pressed and released by frame, and let go when focus goes', (tester) async {
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
    var input = await frame(tester);
    expect(input.keys, {LogicalKeyboardKey.keyW});
    expect(input.wasPressed(LogicalKeyboardKey.keyW), isTrue);

    await tester.sendKeyRepeatEvent(LogicalKeyboardKey.keyW);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.arrowDown);
    input = await frame(tester);
    expect(input.isDown(LogicalKeyboardKey.keyW), isTrue);
    expect(input.pressed, {LogicalKeyboardKey.arrowDown}, reason: 'a repeat is no new press');
    expect(around, isEmpty, reason: 'the view keeps the keys it has focus for');

    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyW);
    input = await frame(tester);
    expect(input.released, {LogicalKeyboardKey.keyW});
    expect(input.keys, {LogicalKeyboardKey.arrowDown});

    other.requestFocus();
    input = await frame(tester);
    expect(input.released, {LogicalKeyboardKey.arrowDown}, reason: 'its key-up will never come here');
    expect(input.keys, isEmpty);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.arrowDown);
  });

  testWidgets('without autofocus, keys reach the view once it is tapped', (tester) async {
    await tester.pumpWidget(app(view(autofocus: false)));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.space);
    expect((await frame(tester)).keys, isEmpty);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.space);

    await tester.tapAt(corner + const Offset(5, 5));
    await tester.sendKeyDownEvent(LogicalKeyboardKey.space);
    expect((await frame(tester)).pressed, {LogicalKeyboardKey.space});
    await tester.sendKeyUpEvent(LogicalKeyboardKey.space);
  });

  testWidgets('gesture widgets around the view still get their gestures', (tester) async {
    var taps = 0;
    var panned = Offset.zero;
    await tester.pumpWidget(
      app(GestureDetector(onTap: () => taps++, onPanUpdate: (d) => panned += d.delta, child: view())),
    );
    await tester.tapAt(corner + const Offset(50, 50));
    expect(taps, 1);
    expect((await frame(tester)).tapped, at(50, 50), reason: 'and the view too');

    await tester.dragFrom(corner + const Offset(50, 50), const Offset(60, 0));
    expect(panned.dx, greaterThan(0));
    await frame(tester);
  });

  testWidgets('a tap aims a ray at what the view shows there', (tester) async {
    final cube = world.add(GameObject(transform: Transform(), components: [MeshRenderer(Mesh.cube(), Material())]));
    // Where the view's camera (the setUp one) is.
    final camera = world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 4)),
        components: [Camera()],
      ),
    );
    await tester.pumpWidget(app(view()));
    await frame(tester);

    for (final (point, hits) in [(const Offset(100, 50), true), (const Offset(150, 50), false)]) {
      await tester.tapAt(corner + point);
      final tapped = (await frame(tester)).tapped!;
      final hit = world.raycast(camera.rayThrough(tapped));
      expect(hit?.object, hits ? cube : null, reason: '$point');
    }
  });

  testWidgets("the view runs the world's loop: a tap reaches the object under it", (tester) async {
    final box = world.add(_Box());
    await tester.pumpWidget(app(view()));
    await frame(tester);
    expect(box.started, isTrue);
    expect(box.updates, greaterThan(0));

    await tester.tapAt(corner + const Offset(100, 50));
    await frame(tester);
    expect(box.taps, 1);
    await tester.tapAt(corner + const Offset(150, 50));
    await frame(tester);
    expect(box.taps, 1, reason: 'beside it');
  });
}

class _Box extends GameObject {
  _Box() : super(components: [MeshRenderer(Mesh.cube(), Material())]);

  var started = false;
  var updates = 0;
  var taps = 0;

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
  bool onTap(Tap tap) {
    taps++;
    return true;
  }
}
