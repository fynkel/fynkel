// Input on its own (docs/stage-4.md §3.6, step 5): made-up events and
// times in, fingers, gestures, signals and polled state out. No world.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

late Input input;

/// What the signals said, in order.
final heard = <String>[];

const frame = 1 / 60;

void step([double dt = frame]) => input.update(dt);

void main() {
  setUp(() {
    heard.clear();
    input = Input()..simulate.resize(200, 100);
    void log<T>(Signal<T> signal, String Function(T value) say) => signal.listen((v) => heard.add(say(v)));
    log(input.onFingerDown, (f) => 'down ${f.id}');
    log(input.onFingerUp, (f) => 'up ${f.id}${f.wasCancelled ? ' cancelled' : ''}');
    log(input.onFingerMove, (f) => 'move ${f.id} ${f.delta.x},${f.delta.y}');
    log(input.onHover, (f) => 'hover ${f.id}');
    log(input.onTap, (f) => 'tap ${f.id} x${f.tapCount}');
    log(input.onLongPress, (f) => 'long ${f.id}');
    log(input.onDragStart, (f) => 'drag start ${f.id}');
    log(input.onDrag, (f) => 'drag ${f.id} ${f.delta.x},${f.delta.y}');
    log(input.onDragEnd, (f) => 'drag end ${f.id}');
    log(input.onSwipe, (s) => 'swipe ${s.side.name}');
    log(input.onScroll, (v) => 'scroll ${v.x},${v.y}');
    log(input.onKeyDown, (k) => 'key down $k');
    log(input.onKeyUp, (k) => 'key up $k');
  });

  group('taps', () {
    test('down and up again in place is a tap, with the finger gone after', () {
      input.simulate.fingerDown(1, 50, 50);
      step();
      expect(heard, ['down 1']);
      final finger = input.fingers.single;
      expect(finger.isPressed, isTrue);
      expect(finger.isTap, isTrue, reason: 'it may still tap');
      expect(finger.position, const ViewPoint(50, 50, width: 200, height: 100));

      heard.clear();
      input.simulate.fingerUp(1, 52, 51);
      step();
      expect(heard, ['move 1 2.0,1.0', 'tap 1 x1', 'up 1'], reason: 'within the slop');
      expect(input.fingers, isEmpty);
      expect(finger.isPressed, isFalse);
      expect(finger.isTap, isTrue, reason: 'once up, whether it tapped');
      expect(finger.age, closeTo(frame, 1e-9), reason: 'down one frame, then up');
    });

    test('a tap in one frame, down and up together, still taps', () {
      input.simulate
        ..fingerDown(1, 50, 50)
        ..fingerUp(1, 50, 50);
      step();
      expect(heard, ['down 1', 'tap 1 x1', 'up 1']);
    });

    test('taps soon after each other, near each other, count up', () {
      void tapAt(int id, double x) {
        input.simulate
          ..fingerDown(id, x, 50)
          ..fingerUp(id, x, 50);
        step(0.1);
      }

      tapAt(1, 50);
      tapAt(2, 60);
      tapAt(3, 40);
      expect(heard.where((h) => h.startsWith('tap')), ['tap 1 x1', 'tap 2 x2', 'tap 3 x3']);

      heard.clear();
      step(0.4);
      tapAt(4, 50);
      expect(heard, contains('tap 4 x1'), reason: 'too long after the last');
      heard.clear();
      tapAt(5, 160);
      expect(heard, contains('tap 5 x1'), reason: 'too far from the last');
    });

    test('a finger held too long, or moved too far, doesn\'t tap', () {
      input.settings.longPressTime = 10;
      input.simulate.fingerDown(1, 50, 50);
      // An event's time is its step's.
      step();
      step(0.6);
      input.simulate.fingerUp(1, 50, 50);
      step();
      expect(heard, ['down 1', 'up 1'], reason: 'past the tap time');

      heard.clear();
      input.simulate.fingerDown(2, 50, 50);
      step();
      input.simulate.fingerMove(2, 69, 50);
      step();
      input.simulate.fingerUp(2, 50, 50);
      step();
      expect(heard, isNot(contains(startsWith('tap'))), reason: 'it moved 19 pixels, past the 18 of the slop');
    });

    test('the settings change the slop', () {
      input.settings.tapSlop = 30;
      input.simulate.fingerDown(1, 50, 50);
      step();
      input.simulate
        ..fingerMove(1, 75, 50)
        ..fingerUp(1, 75, 50);
      step();
      expect(heard, contains('tap 1 x1'));
    });

    test('a long press comes once, at its time, and then the finger won\'t tap', () {
      input.simulate.fingerDown(1, 50, 50);
      // Times a double holds exactly.
      step(0.25);
      step(0.25);
      step(0.125);
      expect(heard, ['down 1']);
      step(0.125);
      expect(heard, ['down 1', 'long 1'], reason: '0.5 s down');
      expect(input.fingers.single.isLongPress, isTrue);
      step(0.5);
      input.simulate.fingerUp(1, 50, 50);
      step();
      expect(heard, ['down 1', 'long 1', 'up 1']);
    });

    test('a finger that moves past the slop before its time never long presses', () {
      input.simulate.fingerDown(1, 50, 50);
      step(0.25);
      input.simulate.fingerMove(1, 90, 50);
      step(0.25);
      step(0.5);
      expect(input.fingers.single.age, 0.75, reason: 'well past the long press time');
      expect(heard, isNot(contains('long 1')));
    });

    test('a cancelled finger goes without tapping', () {
      input.simulate.fingerDown(1, 50, 50);
      step();
      input.simulate.fingerCancel(1);
      step();
      expect(heard, ['down 1', 'up 1 cancelled']);
      expect(input.fingers, isEmpty);
    });
  });

  group('drags', () {
    test('past the slop a drag starts; its deltas add up to the whole move', () {
      input.simulate.fingerDown(1, 50, 50);
      step();
      input.simulate
        ..fingerMove(1, 60, 50)
        ..fingerMove(1, 70, 52);
      step();
      expect(heard, ['down 1', 'drag start 1', 'move 1 20.0,2.0', 'drag 1 20.0,2.0'], reason: 'one move a frame');
      final finger = input.fingers.single;
      expect(finger.isDragging, isTrue);
      expect(finger.isTap, isFalse);

      heard.clear();
      input.simulate.fingerMove(1, 100, 40);
      step();
      expect(heard, ['move 1 30.0,-12.0', 'drag 1 30.0,-12.0']);
      expect(finger.totalDelta, Vec2(50, -10));
      expect(finger.startPosition.x, 50);

      heard.clear();
      step();
      expect(heard, isEmpty, reason: 'still: no move, no drag');
      expect(finger.delta, Vec2.zero);

      input.simulate.fingerUp(1, 100, 40);
      step();
      expect(heard, ['drag end 1', 'up 1'], reason: 'held still before letting go: no swipe');
    });

    test('a finger that moves and goes up in one frame tells its move first', () {
      input.simulate.fingerDown(1, 50, 50);
      step();
      input.simulate
        ..fingerMove(1, 80, 50)
        ..fingerUp(1, 80, 50);
      step();
      expect(heard, [
        'down 1',
        'drag start 1',
        'move 1 30.0,0.0',
        'drag 1 30.0,0.0',
        'drag end 1',
        'swipe right',
        'up 1',
      ], reason: '30 pixels in a 60th of a second is 1,800 a second: a swipe');
    });

    test('its velocity is over the last tenth of a second', () {
      input.simulate.fingerDown(1, 0, 50);
      step();
      for (var i = 1; i <= 12; i++) {
        input.simulate.fingerMove(1, i * 10.0, 50);
        step();
      }
      final v = input.fingers.single.velocity;
      expect(v.x, closeTo(600, 1e-6), reason: '10 pixels each 1/60 s');
      expect(v.y, 0);
    });

    test('a fast drag let go swipes, after its drag end; a slow one doesn\'t', () {
      input.simulate.fingerDown(1, 20, 50);
      step();
      for (var i = 1; i <= 4; i++) {
        input.simulate.fingerMove(1, 20 + i * 20.0, 50);
        step();
      }
      final swipes = <Swipe>[];
      input.onSwipe.listen(swipes.add);
      input.simulate.fingerUp(1, 100, 50);
      step();
      expect(heard.sublist(heard.length - 3), ['drag end 1', 'swipe right', 'up 1']);
      // 80 pixels over the last five frames: the one it let go in didn't
      // move.
      expect(swipes.single.speed, closeTo(960, 1e-6));
      expect(swipes.single.direction, Vec2(1, 0));
      expect(swipes.single.distance, 80);
      expect(swipes.single.finger.id, 1);

      heard.clear();
      input.simulate.fingerDown(2, 100, 80);
      step();
      for (var i = 1; i <= 10; i++) {
        input.simulate.fingerMove(2, 100, 80 - i * 5.0);
        step();
      }
      input.simulate.fingerUp(2, 100, 30);
      step();
      expect(heard, isNot(contains(startsWith('swipe'))), reason: '300 pixels a second, under 800');

      input.settings.swipeSpeed = 200;
      input.simulate.fingerDown(3, 100, 80);
      step();
      for (var i = 1; i <= 10; i++) {
        input.simulate.fingerMove(3, 100, 80 - i * 5.0);
        step();
      }
      input.simulate.fingerUp(3, 100, 30);
      step();
      expect(heard, contains('swipe up'));
    });

    test('a cancelled drag ends without a swipe', () {
      input.simulate.fingerDown(1, 20, 50);
      step();
      input.simulate.fingerMove(1, 80, 50);
      step();
      input.simulate.fingerCancel(1);
      step();
      expect(heard.sublist(heard.length - 2), ['drag end 1', 'up 1 cancelled']);
    });
  });

  group('pinch and twist', () {
    late List<Pinch> pinches, twists;
    setUp(() {
      pinches = [];
      twists = [];
      input.onPinch.listen(pinches.add);
      input.onTwist.listen(twists.add);
    });

    test('two fingers moving apart pinch out, about the point between them', () {
      input.simulate
        ..fingerDown(1, 40, 50)
        ..fingerDown(2, 60, 50);
      step();
      expect(pinches, isEmpty, reason: 'they only just went down');
      input.simulate
        ..fingerMove(1, 30, 50)
        ..fingerMove(2, 70, 50);
      step();
      expect(pinches.single.scale, closeTo(2, 1e-9));
      expect(pinches.single.center, const ViewPoint(50, 50, width: 200, height: 100));
      expect(pinches.single.fingers, 2);
      expect(twists, isEmpty);

      input.simulate
        ..fingerMove(1, 40, 50)
        ..fingerMove(2, 60, 50);
      step();
      expect(pinches.last.scale, closeTo(0.5, 1e-9));
    });

    test('two fingers turning twist, clockwise on the view being positive', () {
      input.simulate
        ..fingerDown(1, 40, 50)
        ..fingerDown(2, 60, 50);
      step();
      input.simulate
        ..fingerMove(1, 50, 40)
        ..fingerMove(2, 50, 60);
      step();
      expect(twists.single.twist, closeTo(math.pi / 2, 1e-9));
      expect(pinches, isEmpty, reason: 'no further apart');
    });

    test('three fingers pinch too; one alone, or the mouse, doesn\'t', () {
      input.simulate
        ..fingerDown(1, 40, 40)
        ..fingerDown(2, 60, 40)
        ..fingerDown(3, 50, 60);
      step();
      input.simulate
        ..fingerMove(1, 30, 30)
        ..fingerMove(2, 70, 30)
        ..fingerMove(3, 50, 70);
      step();
      expect(pinches.single.fingers, 3);
      expect(pinches.single.scale, greaterThan(1));

      pinches.clear();
      input.simulate
        ..fingerUp(2, 70, 30)
        ..fingerUp(3, 50, 70)
        ..fingerDown(9, 100, 50, kind: FingerKind.mouse);
      step();
      input.simulate
        ..fingerMove(1, 10, 10)
        ..fingerMove(9, 150, 50);
      step();
      expect(pinches, isEmpty);
    });

    test("a trackpad's pinch and twist add up over the frame, about the mouse", () {
      input.simulate
        ..hover(-1, 120, 30)
        ..trackpad(scale: 1.5)
        ..trackpad(scale: 2, twist: 0.25)
        ..trackpad(twist: 0.5);
      step();
      expect(pinches.single.scale, 3);
      expect(pinches.single.fingers, 0);
      expect(pinches.single.center.x, 120);
      expect(twists.single, same(pinches.single), reason: 'one frame, one Pinch');
      expect(twists.single.twist, 0.75);
      step();
      expect(pinches, hasLength(1), reason: 'nothing more');
    });
  });

  group('the mouse', () {
    test('is a finger while it hovers, and stays one through presses', () {
      input.simulate.hover(-1, 10, 20);
      step();
      final mouse = input.fingers.single;
      expect(mouse.kind, FingerKind.mouse);
      expect(mouse.isPressed, isFalse);
      expect(input.mousePosition, const ViewPoint(10, 20, width: 200, height: 100));
      expect(heard, isEmpty, reason: 'appearing is no move');

      input.simulate.hover(-1, 15, 20);
      step();
      expect(heard, ['hover -1']);

      heard.clear();
      input.simulate.fingerDown(-1, 15, 20, kind: FingerKind.mouse);
      step();
      expect(input.fingers.single, same(mouse));
      expect(mouse.isPressed, isTrue);
      expect(input.isPressed(MouseButton.left), isTrue);
      expect(input.wasPressed(MouseButton.left), isTrue);

      input.simulate.fingerUp(-1, 15, 20);
      step();
      expect(heard, ['down -1', 'tap -1 x1', 'up -1']);
      expect(input.fingers.single, same(mouse), reason: 'still over the view');
      expect(input.wasReleased(MouseButton.left), isTrue);
      expect(input.isPressed(MouseButton.left), isFalse);

      input.simulate.leave(-1);
      step();
      expect(input.fingers, isEmpty);
      expect(input.mousePosition, isNull);
    });

    test('leaving during a drag: it goes when the button comes up, unless it came back', () {
      input.simulate.fingerDown(-1, 50, 50, kind: FingerKind.mouse);
      step();
      input.simulate
        ..fingerMove(-1, 250, 50)
        ..leave(-1);
      step();
      expect(input.fingers, hasLength(1), reason: 'still dragging');
      input.simulate.fingerUp(-1, 260, 50);
      step();
      expect(input.fingers, isEmpty, reason: 'it left the view');
      expect(input.mousePosition, isNull);

      input.simulate.fingerDown(-1, 50, 50, kind: FingerKind.mouse);
      step();
      input.simulate
        ..fingerMove(-1, 250, 50)
        ..leave(-1)
        ..fingerMove(-1, 150, 50)
        ..hover(-1, 150, 50);
      step();
      input.simulate.fingerUp(-1, 150, 50);
      step();
      expect(input.fingers, hasLength(1), reason: 'it came back');
    });

    test('its buttons: which pressed, others pressed meanwhile, and their taps', () {
      input.simulate.fingerDown(-1, 50, 50, kind: FingerKind.mouse, buttons: 2);
      step();
      final mouse = input.fingers.single;
      expect(mouse.button, MouseButton.right);
      expect(input.wasPressed(MouseButton.right), isTrue);
      expect(input.isPressed(MouseButton.left), isFalse);

      input.simulate.fingerMove(-1, 50, 50, buttons: 2 | 4);
      step();
      expect(input.wasPressed(MouseButton.middle), isTrue);
      expect(input.wasPressed(MouseButton.right), isFalse, reason: 'already down');
      expect(mouse.button, MouseButton.right, reason: 'the one that pressed it');

      input.simulate.fingerMove(-1, 50, 50, buttons: 2);
      step();
      expect(input.wasReleased(MouseButton.middle), isTrue);
      expect(input.isPressed(MouseButton.right), isTrue);

      final taps = <Finger>[];
      input.onTap.listen(taps.add);
      input.simulate.fingerUp(-1, 50, 50);
      step();
      expect(taps.single.button, MouseButton.right, reason: 'every button taps');
      expect(input.wasReleased(MouseButton.right), isTrue);
    });

    test('its slop is a pixel', () {
      input.simulate.fingerDown(-1, 50, 50, kind: FingerKind.mouse);
      step();
      input.simulate.fingerMove(-1, 52, 50);
      step();
      expect(heard, contains('drag start -1'));
    });

    test('a touch holds the left button, until the last touch goes up', () {
      input.simulate.fingerDown(1, 50, 50);
      step();
      expect(input.wasPressed(MouseButton.left), isTrue);
      input.simulate.fingerDown(2, 60, 50);
      step();
      expect(input.wasPressed(MouseButton.left), isFalse);
      input.simulate.fingerUp(1, 50, 50);
      step();
      expect(input.isPressed(MouseButton.left), isTrue);
      input.simulate.fingerUp(2, 60, 50);
      step();
      expect(input.wasReleased(MouseButton.left), isTrue);
      expect(input.isPressed(MouseButton.left), isFalse);
    });

    test('mousePosition is the mouse, or else the first finger down', () {
      expect(input.mousePosition, isNull);
      input.simulate
        ..fingerDown(1, 10, 10)
        ..fingerDown(2, 20, 20);
      step();
      expect(input.mousePosition!.x, 10);
      input.simulate.hover(-1, 99, 99);
      step();
      expect(input.mousePosition!.x, 99);
    });

    test('scrolling adds up over the frame, and is told once', () {
      input.simulate
        ..scrollBy(0, 10)
        ..scrollBy(2, 5);
      step();
      expect(input.scroll, Vec2(2, -15), reason: 'towards you, as Unity says it');
      expect(heard, ['scroll 2.0,-15.0']);
      step();
      expect(input.scroll, Vec2.zero);
      expect(heard, hasLength(1));
    });
  });

  group('keys', () {
    test('down, held and up; repeats are no new presses', () {
      input.simulate.keyDown(KeyCode.w);
      step();
      expect(heard, ['key down KeyPress(w)']);
      expect(input.isPressed(KeyCode.w), isTrue);
      expect(input.wasPressed(KeyCode.w), isTrue);

      input.simulate.keyDown(KeyCode.w);
      step();
      expect(input.wasPressed(KeyCode.w), isFalse);
      expect(input.isPressed(KeyCode.w), isTrue);
      expect(heard, hasLength(1));

      input.simulate.keyUp(KeyCode.w);
      step();
      expect(input.wasReleased(KeyCode.w), isTrue);
      expect(input.isPressed(KeyCode.w), isFalse);
      expect(heard.last, 'key up KeyPress(w)');
    });

    test('down and up in one frame counts as both', () {
      input.simulate
        ..keyDown(KeyCode.space)
        ..keyUp(KeyCode.space);
      step();
      expect(input.wasPressed(KeyCode.space), isTrue);
      expect(input.wasReleased(KeyCode.space), isTrue);
      expect(input.isPressed(KeyCode.space), isFalse);
    });

    test('carry the modifiers held', () {
      final presses = <KeyPress>[];
      input.onKeyDown.listen(presses.add);
      input.simulate
        ..keyDown(KeyCode.shiftLeft)
        ..keyDown(KeyCode.metaRight)
        ..keyDown(KeyCode.s);
      step();
      expect(presses.last.key, KeyCode.s);
      expect(presses.last.shift, isTrue);
      expect(presses.last.meta, isTrue);
      expect(presses.last.control, isFalse);
      expect(presses.last.alt, isFalse);
    });

    test('losing focus lets every held key go up', () {
      input.simulate
        ..keyDown(KeyCode.a)
        ..keyDown(KeyCode.d);
      step();
      heard.clear();
      input.simulate.releaseKeys();
      step();
      expect(heard, ['key up KeyPress(a)', 'key up KeyPress(d)']);
      expect(input.wasReleased(KeyCode.a), isTrue);
    });
  });

  group('axes and actions', () {
    test("'move' reads WASD and the arrows, no faster on a diagonal", () {
      expect(input.axis2('move'), Vec2.zero);
      input.simulate.keyDown(KeyCode.w);
      step();
      expect(input.axis2('move'), Vec2(0, 1));
      input.simulate.keyDown(KeyCode.arrowRight);
      step();
      expect(input.axis2('move').x, closeTo(math.sqrt1_2, 1e-12));
      expect(input.axis2('move').length, closeTo(1, 1e-12));
      input.simulate
        ..keyUp(KeyCode.w)
        ..keyDown(KeyCode.a);
      step();
      expect(input.axis2('move'), Vec2.zero, reason: 'left and right cancel out');
      input.simulate.keyDown(KeyCode.s);
      step();
      expect(input.axis2('move'), Vec2(0, -1));
    });

    test('new axes bind to keys and buttons', () {
      input.bindAxis2(
        'look',
        up: const [KeyCode.i],
        down: const [KeyCode.k],
        left: const [MouseButton.left],
        right: const [KeyCode.l],
      );
      input.simulate.fingerDown(1, 5, 5);
      step();
      expect(input.axis2('look'), Vec2(-1, 0));
      expect(() => input.axis2('nothing'), throwsArgumentError);
    });

    test("'jump' is the space bar, and actions hold while any of their buttons does", () {
      final jump = input.action('jump');
      input.simulate.keyDown(KeyCode.space);
      step();
      expect(jump.wasPressed, isTrue);
      expect(jump.isPressed, isTrue);

      final fire = input.bindAction('fire', const [KeyCode.f, MouseButton.left]);
      expect(input.action('fire'), same(fire));
      input.simulate.keyDown(KeyCode.f);
      step();
      expect(fire.wasPressed, isTrue);
      input.simulate.fingerDown(-1, 5, 5, kind: FingerKind.mouse);
      step();
      expect(fire.wasPressed, isFalse, reason: 'f was already held');
      input.simulate.keyUp(KeyCode.f);
      step();
      expect(fire.wasReleased, isFalse, reason: 'the mouse still holds it');
      expect(fire.isPressed, isTrue);
      input.simulate.fingerUp(-1, 5, 5);
      step();
      expect(fire.wasReleased, isTrue);
      expect(() => input.action('dash'), throwsArgumentError);
    });
  });

  group('Flutter controls', () {
    test('an action pressed and released from outside holds in between, as a key would', () {
      final jump = input.action('jump');
      jump.press();
      expect(jump.isPressed, isFalse, reason: 'it waits for the step, as events do');
      step();
      expect(jump.wasPressed, isTrue);
      expect(jump.isPressed, isTrue);
      step();
      expect(jump.wasPressed, isFalse);
      expect(jump.isPressed, isTrue);
      input.simulate.keyDown(KeyCode.space);
      step();
      expect(jump.wasPressed, isFalse, reason: 'the button already held it');
      jump.release();
      step();
      expect(jump.wasReleased, isFalse, reason: 'the space bar still holds it');
      input.simulate.keyUp(KeyCode.space);
      step();
      expect(jump.wasReleased, isTrue);
      expect(jump.isPressed, isFalse);
    });

    test('pressed and released in one frame is a press and a release', () {
      final jump = input.action('jump');
      jump
        ..press()
        ..release();
      step();
      expect(jump.wasPressed, isTrue);
      expect(jump.wasReleased, isTrue);
      expect(jump.isPressed, isFalse);
    });

    test('a stick sets an axis at once, clamped to 1, and adds to its keys', () {
      input.setAxis2('move', Vec2(0, 0.5));
      expect(input.axis2('move'), Vec2(0, 0.5));
      input.setAxis2('move', Vec2(3, 4));
      expect(input.axis2('move').x, closeTo(0.6, 1e-12));
      expect(input.axis2('move').y, closeTo(0.8, 1e-12));
      input.simulate.keyDown(KeyCode.w);
      step();
      expect(input.axis2('move').length, closeTo(1, 1e-12), reason: 'keys and stick together, no faster');
      input.setAxis2('move', Vec2.zero);
      expect(input.axis2('move'), Vec2(0, 1));
      input.setAxis2('steer', Vec2(-1, 0));
      expect(input.axis2('steer'), Vec2(-1, 0), reason: 'a new axis, with no keys');
    });

    test('an axis or action only Flutter feeds is bound with no keys, and can be asked about (finding 40)', () {
      expect(input.hasAxis2('aim'), isFalse);
      expect(() => input.axis2('aim'), throwsArgumentError, reason: 'a name never bound is a typo, most likely');
      input.bindAxis2('aim');
      expect(input.hasAxis2('aim'), isTrue);
      expect(input.axis2('aim'), Vec2.zero, reason: 'zero until the stick sets it');
      input.bindAxis2('strafe', left: const [KeyCode.q], right: const [KeyCode.e]);
      input.simulate.keyDown(KeyCode.e);
      step();
      expect(input.axis2('strafe'), Vec2(1, 0));
      expect((input.hasAction('jump'), input.hasAction('fire')), (true, false));
      final fire = input.bindAction('fire');
      expect(fire.buttons, isEmpty);
      fire.press();
      step();
      expect(fire.wasPressed, isTrue);
    });

    test('binding an action again changes the same one: a held key that is no longer bound lets go', () {
      final fire = input.bindAction('fire', const [KeyCode.f]);
      input.simulate.keyDown(KeyCode.f);
      step();
      expect(fire.isPressed, isTrue);
      expect(input.bindAction('fire', const [KeyCode.g]), same(fire));
      expect(input.action('fire'), same(fire));
      expect(fire.buttons, [KeyCode.g]);
      expect(fire.isPressed, isFalse);
      step();
      expect(fire.wasReleased, isTrue, reason: 'let go in the next frame');
      step();
      expect(fire.wasReleased, isFalse);
      input.simulate.keyDown(KeyCode.g);
      step();
      expect(fire.wasPressed, isTrue);
      input.bindAction('fire', const [KeyCode.g, KeyCode.h]);
      step();
      expect((fire.isPressed, fire.wasReleased), (true, false), reason: 'still held by G');
      input.simulate.keyUp(KeyCode.g);
      step();
      expect(fire.wasReleased, isTrue);
    });

    test('every key has a short label for a controls page', () {
      expect(
        [
          KeyCode.w,
          KeyCode.digit1,
          KeyCode.f12,
          KeyCode.space,
          KeyCode.arrowUp,
          KeyCode.shiftLeft,
          KeyCode.pageDown,
          KeyCode.numpad5,
          KeyCode.backslash,
          KeyCode.escape,
        ].map((k) => k.label),
        ['W', '1', 'F12', 'Space', '↑', 'Left Shift', 'Page Down', 'Num 5', r'\', 'Esc'],
      );
      final labels = KeyCode.values.map((k) => k.label).toSet();
      expect(labels, hasLength(KeyCode.values.length), reason: 'no two the same');
    });

    test('a pressed action stays pressed through a rebind, and its release still lets go', () {
      final fire = input.bindAction('fire');
      fire.press();
      step();
      input.bindAction('fire', const [KeyCode.f]);
      step();
      expect((fire.isPressed, fire.wasReleased), (true, false));
      fire.release();
      step();
      expect((fire.isPressed, fire.wasReleased), (false, true));
    });

    test('the game uses the keys it binds or asks about; the view claims only those', () {
      expect(input.usesKey(KeyCode.w), isTrue, reason: "'move' binds it");
      expect(input.usesKey(KeyCode.space), isTrue, reason: "'jump' binds it");
      expect(input.usesKey(KeyCode.escape), isFalse);
      expect(input.simulate.keyDown(KeyCode.escape), isFalse);
      expect(input.simulate.keyDown(KeyCode.w), isTrue);
      input.isPressed(KeyCode.escape);
      expect(input.usesKey(KeyCode.escape), isTrue, reason: 'asked about once, claimed from then on');
      expect(input.simulate.keyUp(KeyCode.escape), isTrue);
      input.bindAction('fire', const [KeyCode.f]);
      expect(input.simulate.claims(KeyCode.f), isTrue);
      step();
      expect(heard, [
        'key down KeyPress(escape)',
        'key down KeyPress(w)',
        'key up KeyPress(escape)',
      ], reason: 'the game hears every key');
    });
  });

  group('the step', () {
    test('events wait for the next step; one a listener feeds waits for the one after', () {
      input.simulate.fingerDown(1, 50, 50);
      expect(heard, isEmpty);
      input.onFingerDown.listen((f) {
        if (f.id == 1) input.simulate.fingerDown(2, 60, 60);
      });
      step();
      expect(heard, ['down 1']);
      step();
      expect(heard, ['down 1', 'down 2']);
    });

    test('each held finger is told each frame, after its move', () {
      final held = <String>[];
      input.onFingerHeld.listen((f) => held.add('${f.id}'));
      input.simulate
        ..fingerDown(1, 50, 50)
        ..fingerDown(2, 60, 60);
      step();
      step();
      expect(held, ['1', '2', '1', '2']);
    });

    test('a listener that throws stops the step of an input of its own', () {
      input.onFingerDown.listen((_) => throw StateError('boom'));
      input.simulate.fingerDown(1, 50, 50);
      expect(step, throwsStateError);
    });

    test("a world's input runs in World.update only", () {
      final world = World();
      addTearDown(world.dispose);
      expect(() => world.input.update(frame), throwsStateError);
    });
  });

  group('event times', () {
    test('a finger held through a hitch, its down and up in one batch, is no tap', () {
      input.simulate
        ..fingerDown(1, 50, 50, at: const Duration(seconds: 100))
        ..fingerUp(1, 50, 50, at: const Duration(seconds: 101));
      step();
      expect(heard, ['down 1', 'up 1'], reason: 'held 1 s by its own clock');
    });

    test('a swipe\'s speed is from its events\' times, not the frame\'s', () {
      double? speed;
      input.onSwipe.listen((s) => speed = s.speed);
      input.simulate.fingerDown(1, 10, 50, at: const Duration(milliseconds: 1000));
      for (var i = 1; i <= 10; i++) {
        input.simulate.fingerMove(1, 10.0 + i * 10, 50, at: Duration(milliseconds: 1000 + i * 10));
      }
      input.simulate.fingerUp(1, 110, 50, at: const Duration(milliseconds: 1100));
      step();
      expect(speed, closeTo(1000, 1), reason: '100 px in 0.1 s');
    });

    test('in a world, a finger held through a long frame is no tap', () {
      final world = World();
      addTearDown(world.dispose);
      final taps = <Finger>[];
      world.input
        ..simulate.resize(100, 100)
        ..onTap.listen(taps.add);
      world.input.simulate.fingerDown(1, 50, 50);
      world.update(1 / 60);
      world.update(1.0);
      world.input.simulate.fingerUp(1, 50, 50);
      world.update(1 / 60);
      expect(taps, isEmpty, reason: 'held 1 s, though the game counted 1/3 s');
    });
  });
}
