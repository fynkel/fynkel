// Debug drawing (docs/stage-4.md §3.9, step 8): lines over the scene, for a
// frame or for some seconds.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'golden.dart';
import 'render_test.dart' show stage1;

const red = Color.rgb(1, 0, 0);
const green = Color.rgb(0, 1, 0);

/// Draws its lines every frame, as a game would.
class Drawer extends GameObject {
  Drawer(this.draw);

  final void Function(Debug debug) draw;

  @override
  void onUpdate() {
    super.onUpdate();
    draw(debug);
  }
}

/// Draws from a behaviour.
class DrawerBehaviour extends Behaviour {
  @override
  void onUpdate() {
    super.onUpdate();
    debug.line(Vec3.zero, Vec3.up);
  }
}

void main() {
  late World world;
  setUp(() => world = World()..sky = null);
  tearDown(() => world.dispose());

  test('lines drawn in a frame are drawn over it, and gone the frame after', () {
    stage1(world);
    final view = HeadlessView(world, width: 256, height: 256);
    addTearDown(view.dispose);
    var drawing = true;
    world.add(
      Drawer((debug) {
        if (!drawing) return;
        // The same lines as the Rust tests, over the same scene.
        debug
          ..line(Vec3(-2, 0, 0), Vec3(2, 0, 0), color: red)
          ..line(Vec3(0, -2, -3), Vec3(0, 2, -3), color: green);
      }),
    );
    world.update(1 / 60);
    world.update(1 / 60);
    expect(world.debug.count, 0, reason: 'sent, and not kept');
    expectGolden('lines', view.read(view.frame()).rgba, 256, 256);
    world.update(1 / 60);
    expectGolden('lines', view.read(view.frame()).rgba, 256, 256);

    drawing = false;
    world.update(1 / 60);
    expectGolden('cube', view.read(view.frame()).rgba, 256, 256);
  });

  test('a ray, a box, a sphere and a path, from objects and behaviours', () {
    stage1(world);
    final view = HeadlessView(world, width: 256, height: 256);
    addTearDown(view.dispose);
    world.add(
      Drawer((debug) {
        debug
          ..ray(Vec3(-1.5, 0, 1), Vec3(3, 0, 0), color: Color(0xFFFFE000))
          ..box(Vec3.zero, Vec3(1.2, 1.2, 1.2), rotation: Quat.axisAngle(Vec3.up, 0.5), color: green)
          ..sphere(Vec3(0, 0.6, 0), 0.4, color: Color(0xFF40C0FF))
          ..path([Vec3(-1.5, -1, 0), Vec3(-0.5, -0.6, 0), Vec3(0.5, -1, 0), Vec3(1.5, -0.6, 0)], color: red);
      }),
    );
    world.add(GameObject(components: [DrawerBehaviour()]));
    world.update(1 / 60);
    expectGolden('debug_shapes', view.read(view.frame()).rgba, 256, 256);
  });

  test('each shape is so many lines', () {
    final debug = world.debug;
    int lines(void Function() draw) {
      final before = debug.count;
      draw();
      return debug.count - before;
    }

    expect(lines(() => debug.line(Vec3.zero, Vec3.up)), 1);
    expect(lines(() => debug.ray(Vec3.zero, Vec3.up)), 1);
    expect(lines(() => debug.box(Vec3.zero, Vec3.one)), 12);
    expect(lines(() => debug.sphere(Vec3.zero, 1)), 96);
    final square = [Vec3.zero, Vec3.right, Vec3(1, 1, 0), Vec3.up];
    expect(lines(() => debug.path(square)), 3);
    expect(lines(() => debug.path(square, closed: true)), 4);
    expect(lines(() => debug.path(square.take(2).toList(), closed: true)), 1, reason: 'two points: one line');
    expect(lines(() => debug.path([Vec3.zero])), 0);
    expect(lines(() => debug.path([])), 0);
    debug.clear();
    expect(debug.count, 0);
  });

  test('a line with seconds stays that long, in unscaled time, then goes', () {
    final debug = world.debug;
    world.update(0.3);
    debug.line(Vec3.zero, Vec3.up, seconds: 1);
    debug.line(Vec3.zero, Vec3.right);
    expect(debug.count, 2);
    world.update(0.3);
    expect(debug.count, 1, reason: 'the one-frame line went; 0.7 s left on the other');
    world.time.scale = 0;
    world.update(0.3);
    world.update(0.3);
    expect(debug.count, 1, reason: 'unscaled seconds: they pass while paused, and 0.1 s is left');
    world.update(0.2);
    expect(debug.count, 0);

    // Gone the frame its time is up, not drawn once more.
    debug.line(Vec3.zero, Vec3.up, seconds: 0.5);
    world.update(0.25);
    expect(debug.count, 1);
    world.update(0.25);
    expect(debug.count, 0);

    debug.line(Vec3.zero, Vec3.up, seconds: -1);
    debug.line(Vec3.zero, Vec3.up, seconds: double.nan);
    world.update(0);
    expect(debug.count, 0, reason: 'no seconds: one frame');
  });

  test('a line with seconds stays on screen until they pass', () {
    stage1(world);
    final view = HeadlessView(world, width: 256, height: 256);
    addTearDown(view.dispose);
    world.debug
      ..line(Vec3(-2, 0, 0), Vec3(2, 0, 0), color: red, seconds: 1)
      ..line(Vec3(0, -2, -3), Vec3(0, 2, -3), color: green, seconds: 1);
    for (var i = 0; i < 3; i++) {
      world.update(0.3);
    }
    expectGolden('lines', view.read(view.frame()).rgba, 256, 256);
    world.update(0.2);
    expectGolden('cube', view.read(view.frame()).rgba, 256, 256);
  });

  test('off, drawing does nothing and the lines go', () {
    stage1(world);
    final view = HeadlessView(world, width: 256, height: 256);
    addTearDown(view.dispose);
    final debug = world.debug;
    expect(debug.enabled, isTrue, reason: 'on in a debug build');
    debug.line(Vec3(-2, 0, 0), Vec3(2, 0, 0), color: red, seconds: 10);
    world.update(0);
    debug.enabled = false;
    debug
      ..line(Vec3.zero, Vec3.up)
      ..ray(Vec3.zero, Vec3.up)
      ..box(Vec3.zero, Vec3.one)
      ..sphere(Vec3.zero, 1)
      ..path([Vec3.zero, Vec3.up]);
    expect(debug.count, 1, reason: 'only the one from before');
    world.update(0);
    expect(debug.count, 0);
    expectGolden('cube', view.read(view.frame()).rgba, 256, 256);
    debug.enabled = true;
    debug.line(Vec3(-2, 0, 0), Vec3(2, 0, 0), color: red);
    expect(debug.count, 1);
  });

  test('many lines grow the buffer and keep every one', () {
    final debug = world.debug;
    for (var i = 0; i < 1000; i++) {
      debug.line(Vec3(i.toDouble(), 0, 0), Vec3.zero, seconds: i.isEven ? 5 : 0);
    }
    expect(debug.count, 1000);
    world.update(0);
    expect(debug.count, 500);
  });

  test('a world disposed with lines left frees them', () {
    final other = World()..sky = null;
    other.debug.line(Vec3.zero, Vec3.up, seconds: 1);
    other.update(0);
    other.dispose();
    expect(other.isDisposed, isTrue);
  });

  test("drawing in a disposed world throws, and doesn't write into freed memory", () {
    final other = World()..sky = null;
    other.debug.line(Vec3.zero, Vec3.up);
    other.update(1 / 60);
    other.dispose();
    expect(() => other.debug.line(Vec3.zero, Vec3.up), throwsStateError);
    expect(other.debug.count, 0);
  });

  test('drawShapes shows each particle system\'s shape and the planes it bounces off', () {
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 2, 5), rotation: Quat.lookRotation(Vec3(0, -2, -5))),
        components: [Camera(clearColor: Color(0xFF202020), bloom: null, ambientOcclusion: null)],
      ),
    );
    final floor = world.add(
      GameObject(
        name: 'floor',
        transform: Transform(position: Vec3(0, -0.5, 0)),
      ),
    );
    for (final (x, shape) in [
      (-1.5, ParticleShape.cone(radius: 0.3)),
      (0.0, ParticleShape.sphere(radius: 0.4)),
      (1.5, ParticleShape.box(size: Vec3(0.6, 0.6, 0.6))),
    ]) {
      world.add(
        GameObject(
          transform: Transform(position: Vec3(x, 0, 0)),
          components: [
            ParticleSystem(
              shape: shape,
              playOnAwake: false,
              collision: ParticleCollision(planes: [floor]),
            ),
          ],
        ),
      );
    }
    final view = HeadlessView(world, width: 256, height: 256);
    addTearDown(view.dispose);
    world.update(1 / 60);
    expectGolden('debug_particle_shapes', view.read(view.frame()).rgba, 256, 256);
    world.debug.drawShapes = true;
    world.update(1 / 60);
    expect(world.debug.count, 0, reason: 'drawn each frame, kept for none');
    expectGolden('debug_particle_shapes_on', view.read(view.frame()).rgba, 256, 256);
  });

  test('drawSplines shows each container\'s splines and their knots', () {
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 4, 6), rotation: Quat.lookRotation(Vec3(0, -4, -6))),
        components: [Camera(clearColor: Color(0xFF202020), bloom: null, ambientOcclusion: null)],
      ),
    );
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, -1)),
        components: [
          SplineContainer([
            Spline.smooth([Vec3(-3, 0, 1), Vec3(-1, 1, -1), Vec3(1, 0, 1), Vec3(3, 1, -1)]),
            Spline.linear([Vec3(-2, 0, 2), Vec3(2, 0, 2), Vec3(0, 0, 3)], closed: true),
          ]),
        ],
      ),
    );
    final view = HeadlessView(world, width: 256, height: 256);
    addTearDown(view.dispose);
    world.debug.drawSplines = true;
    world.update(1 / 60);
    expect(world.debug.count, 0, reason: 'drawn each frame, kept for none');
    expectGolden('debug_splines', view.read(view.frame()).rgba, 256, 256);
  });
}
