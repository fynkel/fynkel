// Hardening G2: destroy(after:), for shots and sparks, instead of a
// routine each.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Spark extends GameObject {
  var destroyed = 0;

  @override
  void onDestroy() {
    super.onDestroy();
    destroyed++;
  }
}

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  // Frames of 0.1 s, under time.maxDelta, for [seconds] of real time.
  void run(double seconds) {
    for (var t = 0.0; t < seconds - 1e-9; t += 0.1) {
      world.update(seconds - t < 0.1 ? seconds - t : 0.1);
    }
  }

  test('destroy(after:) destroys it that much game time later, once', () {
    final spark = world.add(Spark())..destroy(after: 1);
    run(0.5);
    expect(spark.isDestroyed, isFalse);
    run(0.49);
    expect(spark.isDestroyed, isFalse);
    run(0.02);
    expect(spark.isDestroyed, isTrue);
    expect(spark.destroyed, 1);
    run(1);
    expect(spark.destroyed, 1);
  });

  test('it counts game time: slowed by time.scale, still while paused, and while it is hidden', () {
    final spark = world.add(Spark()..active = false)..destroy(after: 1);
    world.time.scale = 0.5;
    run(1.5);
    expect(spark.isDestroyed, isFalse, reason: '0.75 s of game time');
    world.paused = true;
    run(1);
    world.paused = false;
    world.update(0);
    expect(spark.isDestroyed, isFalse, reason: 'paused');
    run(0.5);
    expect(spark.isDestroyed, isTrue, reason: 'hidden, it still went');
  });

  test('before it is added, it counts from when it is', () {
    final spark = Spark()..destroy(after: 1);
    run(5);
    world.add(spark);
    run(0.5);
    expect(spark.isDestroyed, isFalse);
    run(0.6);
    expect(spark.isDestroyed, isTrue);
  });

  test('given twice, the sooner wins; destroyed before then, nothing more happens', () {
    final spark = world.add(Spark())
      ..destroy(after: 2)
      ..destroy(after: 1)
      ..destroy(after: 3);
    run(1.01);
    expect(spark.isDestroyed, isTrue);

    final other = world.add(Spark())..destroy(after: 1);
    other.destroy();
    run(2);
    expect(other.destroyed, 1);
  });

  test('a parent destroyed first takes it along; then its time does nothing', () {
    final parent = world.add(GameObject());
    final spark = parent.add(Spark())..destroy(after: 1);
    parent.destroy(after: 0.5);
    run(0.6);
    expect(spark.isDestroyed, isTrue);
    run(1);
    expect(spark.destroyed, 1);
  });

  test('a negative or NaN time throws', () {
    final spark = world.add(Spark());
    expect(() => spark.destroy(after: -1), throwsArgumentError);
    expect(() => spark.destroy(after: double.nan), throwsArgumentError);
    expect(spark.isDestroyed, isFalse);
  });
}
