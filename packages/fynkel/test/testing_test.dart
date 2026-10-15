// Hardening G8: package:fynkel/testing.dart, what each example's tests
// wrote for themselves.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  late World world;
  setUp(() => world = World()..input.simulate.resize(320, 200));
  tearDown(() => world.dispose());

  test('run runs that many seconds of frames, 60 a second unless told', () {
    var frames = 0;
    world.add(_Counter(() => frames++));
    world.run(0.5);
    expect(frames, 30);
    expect(world.time.sinceStart, closeTo(0.5, 1e-9));
    world.run(1, fps: 10);
    expect(frames, 40);
  });

  test('tap taps there on the next frame, a new finger each time', () {
    final taps = <ViewPoint>[];
    world.input.onTap.listen((finger) => taps.add(finger.position));
    world.tap(10, 20);
    world.run(1 / 60);
    world.tap(30, 40);
    world.run(1 / 60);
    expect(taps, [world.viewPoint(10, 20), world.viewPoint(30, 40)]);
    expect(world.viewPoint(10, 20), const ViewPoint(10, 20, width: 320, height: 200));
  });

  test('without a view size, tap and viewPoint say how to give one', () {
    final bare = World();
    addTearDown(bare.dispose);
    expect(() => bare.tap(1, 1), throwsA(isStateError.having((e) => e.message, 'message', contains('resize'))));
    expect(() => bare.viewPoint(1, 1), throwsStateError);
  });
}

class _Counter extends GameObject {
  _Counter(this.tick);

  final void Function() tick;

  @override
  void onUpdate() {
    super.onUpdate();
    tick();
  }
}
