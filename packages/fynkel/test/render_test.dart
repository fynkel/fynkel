import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'golden.dart';

/// The stage 1 scene, written the way a game writes it.
({World world, GameObject cube, Material orange}) stage1([World? into]) {
  final world = into ?? World();
  world.add(
    GameObject(
      transform: Transform(position: Vec3(0, 1.5, 4), lookAt: Vec3.zero),
      components: [Camera()],
    ),
  );
  world.add(
    GameObject(
      transform: Transform(forward: Vec3(-0.4, -1, -0.3)),
      components: [Light.sun()],
    ),
  );
  world.add(GameObject(components: [Light.ambient(intensity: 0.15)]));
  final orange = Material(color: Color(0xFFE0613A));
  final cube = world.add(
    GameObject(
      transform: Transform(rotation: Quat.axisAngle(Vec3.up, 30 * math.pi / 180)),
      components: [MeshRenderer(Mesh.cube(), orange)],
    ),
  );
  return (world: world, cube: cube, orange: orange);
}

void main() {
  test('the stage 1 scene, built in Dart, renders to the same golden image as the Rust tests', () {
    final scene = stage1();
    final view = HeadlessView(scene.world, width: 256, height: 256);
    addTearDown(view.dispose);
    addTearDown(scene.world.dispose);

    final frame = view.read(view.frame());
    expectGolden('cube', frame.rgba, 256, 256);
  });

  test('an inactive object and everything below it are missing from the picture', () {
    // A rig made first, so its camera would win, with a cube right in front
    // of the stage 1 camera, then the stage 1 scene around it.
    final world = World();
    final rig = world.add(GameObject()..active = false);
    rig
      ..add(
        GameObject(
          transform: Transform(position: Vec3(0, 0, 6)),
          components: [Camera(fovY: 100, clearColor: Color.rgb(1, 0, 0))],
        ),
      )
      ..add(GameObject(components: [Light.ambient(color: Color.rgb(0, 0, 1), intensity: 5)]))
      ..add(
        GameObject(
          transform: Transform(position: Vec3(0, 1.5, 3)),
          components: [MeshRenderer(Mesh.cube(), Material())],
        ),
      );
    final scene = stage1(world);
    final view = HeadlessView(scene.world, width: 256, height: 256);
    addTearDown(view.dispose);
    addTearDown(world.dispose);

    expectGolden('cube', view.read(view.frame()).rgba, 256, 256);
    rig.active = true;
    final [r, g, b, _] = view.read(view.frame()).rgba.sublist(0, 4);
    expect((r, g, b), (255, 0, 0), reason: "shown, the rig's camera wins and clears to red");
  });

  test('changing a material changes the objects that use it', () {
    final scene = stage1();
    final view = HeadlessView(scene.world, width: 64, height: 64);
    addTearDown(view.dispose);
    addTearDown(scene.world.dispose);

    List<int> centre() {
      final rgba = view.read(view.frame()).rgba;
      final i = (32 * 64 + 32) * 4;
      return rgba.sublist(i, i + 3);
    }

    final [r, g, b] = centre();
    expect(r, greaterThan(b), reason: 'orange');
    scene.orange.color = Color(0xFF3A61E0);
    final [r2, g2, b2] = centre();
    expect(b2, greaterThan(r2), reason: 'blue now');
    expect((g2 - g).abs(), lessThan(8), reason: 'the green channel is the same in both');
  });

  test('a despawned object disappears from the picture', () {
    final scene = stage1();
    final view = HeadlessView(scene.world, width: 64, height: 64);
    addTearDown(view.dispose);
    addTearDown(scene.world.dispose);

    final before = view.read(view.frame()).rgba;
    scene.cube.destroy();
    final after = view.read(view.frame()).rgba;
    final i = (32 * 64 + 32) * 4;
    expect(before.sublist(i, i + 3), isNot(after.sublist(i, i + 3)));
    expect(after.sublist(i, i + 3), after.sublist(0, 3), reason: 'only the clear colour is left');
  });

  test('a view keeps working after its world is disposed, and a disposed view refuses calls', () {
    final scene = stage1();
    final view = HeadlessView(scene.world, width: 16, height: 16);
    scene.world.dispose();
    expect(view.read(view.frame()).rgba, hasLength(16 * 16 * 4));
    view.dispose();
    view.dispose();
    expect(view.frame, throwsStateError);
  });

  test('reading a frame that never comes times out with a clear error', () {
    final world = World();
    final view = HeadlessView(world, width: 8, height: 8);
    addTearDown(view.dispose);
    addTearDown(world.dispose);
    expect(
      () => view.read(view.frame() + 5, timeout: Duration(milliseconds: 50)),
      throwsA(isA<FynkelException>().having((e) => e.status, 'status', FynkelStatus.timeout)),
    );
  });
}
