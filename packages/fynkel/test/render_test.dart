import 'dart:io';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'golden.dart';

/// The stage 1 scene, written the way a game writes it.
({World world, GameObject cube, Material orange}) stage1([World? into]) {
  final world = into ?? World();
  world.add(
    GameObject(
      transform: Transform(position: Vec3(0, 1.5, 4), rotation: Quat.lookRotation(Vec3(0, -1.5, -4))),
      // As the Rust tests' camera, whose goldens it's held to.
      components: [Camera(bloom: null, ambientOcclusion: null)],
    ),
  );
  world.add(
    GameObject(
      transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.3))),
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

  test('every quality level draws the same picture, and a world can change level between frames', () {
    final scene = stage1();
    final world = scene.world;
    final view = HeadlessView(world, width: 256, height: 256);
    addTearDown(view.dispose);
    addTearDown(world.dispose);

    expect(world.quality, isNull, reason: "the device's, unless the game picks");
    if (Platform.isMacOS) expect(Quality.device, Quality.high, reason: 'an Apple GPU');
    for (final quality in [Quality.low, Quality.high, Quality.medium, null]) {
      world.quality = quality;
      expect(world.quality, quality);
      expectGolden('cube', view.read(view.frame()).rgba, 256, 256);
    }
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
          components: [Camera(fovY: 100.deg, clearColor: Color.rgb(1, 0, 0))],
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

  test('a lamp and a spotlight light what they reach, in colour, and a lamp moved away lights nothing', () {
    // A grey wall facing the camera, dark but for the lights put in.
    final world = World();
    addTearDown(world.dispose);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 3)),
        components: [Camera()],
      ),
    );
    world.add(
      GameObject(
        transform: Transform(rotation: Quat.axisAngle(Vec3.right, 90.deg), scale: Vec3.one * 4),
        components: [MeshRenderer(Mesh.plane(), Material(color: Color.rgb(0.5, 0.5, 0.5), roughness: 1))],
      ),
    );
    final view = HeadlessView(world, width: 64, height: 64);
    addTearDown(view.dispose);
    List<int> pixel(int x, int y) {
      final rgba = view.read(view.frame()).rgba;
      final i = (y * 64 + x) * 4;
      return rgba.sublist(i, i + 3);
    }

    expect(pixel(32, 32), [0, 0, 0], reason: 'no light yet');
    final lamp = world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 1)),
        components: [Light.point(color: Color.rgb(1, 0, 0), intensity: 2, range: 5)],
      ),
    );
    final [r, g, b] = pixel(32, 32);
    expect(r, greaterThan(100), reason: 'a red lamp 1 m away');
    expect((g, b), (0, 0), reason: 'red only');
    lamp.get<Light>()!.range = 0.9;
    expect(pixel(32, 32), [0, 0, 0], reason: 'out of its reach');
    lamp.destroy();

    // A narrow spot from 2 m: the middle lit, the corner not.
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 2)),
        components: [Light.spot(intensity: 8, angle: 40.deg, softness: 0)],
      ),
    );
    expect(pixel(32, 32)[0], greaterThan(100));
    expect(pixel(4, 4), [0, 0, 0]);
  });

  test('a sun with shadows darkens the floor under a cube, unless the cube casts none or the floor takes none', () {
    final world = World();
    addTearDown(world.dispose);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 6, 4), rotation: Quat.lookRotation(Vec3(0, -6, -4))),
        components: [Camera()],
      ),
    );
    final grey = Material(color: Color.rgb(0.7, 0.7, 0.7), roughness: 1);
    final floor = world.add(
      GameObject(
        transform: Transform(scale: Vec3.one * 12),
        components: [MeshRenderer(Mesh.plane(), grey)],
      ),
    );
    final cube = world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 1.5, 0)),
        components: [MeshRenderer(Mesh.cube(), grey)],
      ),
    );
    final sun = world.add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(1, -2, 0))),
        components: [Light.sun(shadows: true)],
      ),
    );
    final view = HeadlessView(world, width: 64, height: 64);
    addTearDown(view.dispose);
    int light() => view.read(view.frame()).rgba.fold(0, (sum, v) => sum + v);

    final shadowed = light();
    sun.get<Light>()!.shadows = false;
    final plain = light();
    expect(shadowed, lessThan(plain), reason: 'the cube shadows the floor');
    sun.get<Light>()!.shadows = true;
    cube.get<MeshRenderer>()!.castShadows = false;
    expect(light(), plain, reason: 'a cube that casts none');
    cube.get<MeshRenderer>()!.castShadows = true;
    floor.get<MeshRenderer>()!.receiveShadows = false;
    expect(light(), plain, reason: 'a floor that takes none');
    floor.get<MeshRenderer>()!.receiveShadows = true;
    expect(light(), shadowed);
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

  test("a renderer's own colour draws in place of its material's, and null goes back to it", () {
    final scene = stage1();
    final view = HeadlessView(scene.world, width: 64, height: 64);
    addTearDown(view.dispose);
    addTearDown(scene.world.dispose);

    List<int> centre() {
      final rgba = view.read(view.frame()).rgba;
      final i = (32 * 64 + 32) * 4;
      return rgba.sublist(i, i + 3);
    }

    final orange = centre();
    final renderer = scene.cube.get<MeshRenderer>()!;
    renderer.color = Color(0xFF3A61E0);
    final [r, _, b] = centre();
    expect(b, greaterThan(r), reason: 'blue, its own');
    expect(scene.orange.color, Color(0xFFE0613A), reason: 'the material stays');
    renderer.color = null;
    expect(centre(), orange);
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
