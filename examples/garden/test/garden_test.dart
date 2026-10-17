import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:garden/garden.dart';

const width = 320, height = 200;

void main() {
  late World world;
  late Garden garden;
  setUp(() {
    world = World(source: (asset) async => File('assets/$asset').readAsBytesSync())
      ..input.simulate.resize(width.toDouble(), height.toDouble());
    garden = world.add(Garden());
  });
  tearDown(() => world.dispose());

  /// What the engine holds for drawing: [ViewStats.textureBytes] and
  /// [ViewStats.meshBytes] after a frame.
  (int, int) held(HeadlessView view) {
    view.read(view.frame());
    return (view.stats.textureBytes, view.stats.meshBytes);
  }

  test('the garden loads; the character idles, walks where the ground is tapped, and picks up when tapped', () async {
    await garden.loaded;
    final character = world.first<Character>()!;
    final animator = character.children.single.animator!;
    world.update(0);
    expect(animator.current, 'idle');
    final start = character.position;

    // The sky, top left: nothing happens.
    world.tap(5, 5);
    world.update(1 / 60);
    expect((animator.current, character.position), ('idle', start));

    // The ground below the character, near the bottom of the view.
    world.tap(width / 2, height - 10);
    world.update(1 / 60);
    final goal = character.position;
    world.run(0.5);
    expect(animator.current, 'walk');
    expect(animator.weight('walk'), 1, reason: 'faded in from the idle');
    final walked = character.position;
    expect((walked - start).length, greaterThan(0.5));
    expect(goal, isNot(walked));
    world.run(10);
    expect(animator.current, 'idle', reason: 'there');
    expect((character.position - walked).length, lessThan(Character.speed * 10));

    // The character itself, in the middle of the view's bottom half now.
    final there = character.position;
    final camera = world.first<OrbitCamera>()!.get<Camera>()!;
    final middle = camera.worldToScreenPoint(character.position + Vec3(0, 1, 0))!;
    world.tap(middle.x, middle.y);
    world.update(1 / 60);
    expect(animator.current, 'pick-up');
    world.run(animator.length + 0.5);
    expect(animator.current, 'idle', reason: 'back to the idle when the pick-up ended');
    expect(character.position, there, reason: 'a pick-up stays where it is');

    final view = HeadlessView(world, width: width, height: height);
    addTearDown(view.dispose);
    view.read(view.frame());
    expect(view.stats.textureBytes, greaterThan(1 << 20), reason: 'the grass and the character texture');
    final shot = Platform.environment['GARDEN_SHOT'];
    if (shot != null) File(shot).writeAsBytesSync(view.png());
  });

  test('destroying the garden frees everything it loaded, and a new one loads the same', () async {
    final view = HeadlessView(world, width: width, height: height);
    addTearDown(view.dispose);
    garden.destroy();
    // The plane and the cube stay: the engine's own meshes.
    world.add(GameObject(components: [MeshRenderer(Mesh.plane(), Material())]));
    final empty = held(view);

    final first = world.add(Garden());
    await first.loaded;
    final loaded = held(view);
    expect(loaded.$1, greaterThan(empty.$1 + (1 << 20)));

    for (var i = 0; i < 3; i++) {
      world.first<Garden>()!.destroy();
      expect(world.all<Props>(), isEmpty);
      expect(held(view), empty, reason: 'every texture and mesh freed');
      await world.add(Garden()).loaded;
      expect(held(view), loaded);
    }
  });

  test('a drag goes around the character, and scrolling zooms', () async {
    await garden.loaded;
    final camera = world.first<OrbitCamera>()!;
    world.update(0);
    final start = camera.transform.position;
    world.input.simulate
      ..resize(320, 200)
      ..fingerDown(99, 100, 100);
    world.update(0);
    world.input.simulate.fingerMove(99, 200, 100);
    world.update(0);
    world.input.simulate.fingerUp(99, 200, 100);
    final around = camera.transform.position;
    expect((around - start).length, greaterThan(1));
    expect(around.y, closeTo(start.y, 1e-9), reason: 'sideways, at the same height');

    world.input.simulate.scrollBy(0, -200);
    world.update(0);
    const target = OrbitCamera.target;
    expect((camera.transform.position - target).length, lessThan((around - target).length));
  });
}
