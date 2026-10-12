// world.paused (docs/stage-6-fixes.md, PR 10): time stands still, the
// game's input lets go and stops, and the frame after counts no time.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Counter extends GameObject {
  int updates = 0, fixed = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    updates++;
  }

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    fixed++;
  }
}

void main() {
  late World world;
  setUp(() => world = World()..input.simulate.resize(100, 100));
  tearDown(() => world.dispose());

  test('time stands still, updates still run, unscaled time goes on', () {
    final counter = world.add(Counter());
    world.update(0.1);
    final since = world.time.sinceStart;
    final fixed = counter.fixed;
    world.paused = true;
    expect(world.paused, isTrue);
    world
      ..update(0.1)
      ..update(0.1);
    expect(world.time.delta, 0);
    expect(world.time.sinceStart, since);
    expect(world.time.unscaledDelta, closeTo(0.1, 1e-12));
    expect(counter.updates, 3, reason: 'objects still update, as with a time scale of 0');
    expect(counter.fixed, fixed, reason: 'no fixed steps');
    expect(world.time.scale, 1, reason: 'the scale is left as it was');
  });

  test('the first frame after counts no time, however long the pause', () {
    world.update(0.1);
    world.paused = true;
    world.update(0.1);
    world.paused = false;
    world.update(0.1);
    expect(world.time.delta, 0);
    expect(world.time.unscaledDelta, 0);
    world.update(0.1);
    expect(world.time.delta, closeTo(0.1, 1e-12));
  });

  test('held keys, fingers and actions let go; what comes while paused is dropped', () {
    final input = world.input;
    final heard = <String>[];
    input.onKeyUp.listen((k) => heard.add('key up ${k.key.name}'));
    input.onFingerUp.listen((f) => heard.add('up ${f.id}${f.wasCancelled ? ' cancelled' : ''}'));
    final jump = input.action('jump');
    input.simulate
      ..keyDown(KeyCode.w)
      ..fingerDown(1, 50, 50);
    jump.press();
    world.update(0.1);
    expect(input.axis2('move'), Vec2(0, 1));
    input.setAxis2('steer', Vec2(1, 0));

    world.paused = true;
    world.update(0.1);
    expect(heard, ['key up w', 'up 1 cancelled']);
    expect(input.isPressed(KeyCode.w), isFalse);
    expect(input.fingers, isEmpty);
    expect(jump.isPressed, isFalse);
    expect(jump.wasReleased, isTrue);
    expect(input.axis2('move'), Vec2.zero);
    expect(input.axis2('steer'), Vec2.zero, reason: 'a stick reads nothing while paused');

    heard.clear();
    input.simulate
      ..keyDown(KeyCode.space)
      ..fingerDown(2, 50, 50);
    jump.press();
    world.update(0.1);
    expect(heard, isEmpty);
    expect(input.fingers, isEmpty);
    expect(jump.wasPressed, isFalse);
    expect(input.simulate.claims(KeyCode.space), isFalse, reason: 'the view claims no key while paused');

    world.paused = false;
    world.update(0.1);
    expect(input.isPressed(KeyCode.space), isFalse, reason: 'dropped, not kept for later');
    expect(input.axis2('steer'), Vec2(1, 0), reason: 'the stick is still where it was');
    input.simulate.keyDown(KeyCode.space);
    world.update(0.1);
    expect(jump.wasPressed, isTrue);
  });
}
