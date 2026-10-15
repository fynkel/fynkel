// Hardening G3: every component can be turned off with `enabled`, and on
// again as it was, without touching its object or its other components.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'model_test.dart' show glb;

/// A grey wall facing a camera at 3 m, lit by [lights], drawn 64 × 64.
({World world, GameObject wall, List<int> Function() centre}) wall(List<Light> lights) {
  final world = World();
  addTearDown(world.dispose);
  world.add(
    GameObject(
      transform: Transform(position: Vec3(0, 0, 3)),
      components: [Camera(clearColor: Color.rgb(0, 0, 0))],
    ),
  );
  final wall = world.add(
    GameObject(
      transform: Transform(rotation: Quat.axisAngle(Vec3.right, 90.deg), scale: Vec3.one * 4),
      components: [MeshRenderer(Mesh.plane(), Material(color: Color.rgb(0.5, 0.5, 0.5), roughness: 1))],
    ),
  );
  for (final light in lights) {
    world.add(GameObject(components: [light]));
  }
  final view = HeadlessView(world, width: 64, height: 64);
  addTearDown(view.dispose);
  List<int> centre() {
    final rgba = view.read(view.frame()).rgba;
    const i = (32 * 64 + 32) * 4;
    return rgba.sublist(i, i + 3);
  }

  return (world: world, wall: wall, centre: centre);
}

void main() {
  test('every component has enabled, on unless set', () {
    final Component camera = Camera(), light = Light.sun(), renderer = MeshRenderer();
    for (final component in [camera, light, renderer]) {
      expect(component.enabled, isTrue);
      component.enabled = false;
      expect(component.enabled, isFalse);
    }
  });

  test('a mesh renderer turned off draws nothing, and on again draws as it was, changes made while off too', () {
    final scene = wall([Light.ambient(intensity: 1)]);
    final lit = scene.centre();
    expect(lit[0], greaterThan(50), reason: 'the wall is drawn');

    final renderer = scene.wall.get<MeshRenderer>()!..enabled = false;
    expect(scene.centre(), [0, 0, 0], reason: "off: the camera's clear colour");
    expect(renderer.mesh, isNotNull, reason: 'it keeps its mesh');

    renderer.color = Color.rgb(1, 0, 0);
    expect(scene.centre(), [0, 0, 0], reason: 'a change while off draws nothing either');
    renderer.enabled = true;
    final red = scene.centre();
    expect(red[0], greaterThan(100));
    expect(red[0], greaterThan(red[1] * 3), reason: 'on again, with the colour set while it was off: $red');
  });

  test('a light turned off lights nothing, and on again lights as it was', () {
    final lamp = Light.point(color: Color.rgb(1, 0, 0), intensity: 2, range: 5);
    final scene = wall([lamp]);
    // The lamp 1 m in front of the wall.
    lamp.gameObject.transform = Transform(position: Vec3(0, 0, 1));
    final lit = scene.centre();
    expect(lit[0], greaterThan(50));

    lamp.enabled = false;
    expect(scene.centre(), [0, 0, 0], reason: 'off');
    lamp.intensity = 4;
    expect(scene.centre(), [0, 0, 0], reason: 'a change while off lights nothing either');
    lamp.enabled = true;
    expect(scene.centre()[0], greaterThan(lit[0]), reason: 'on again, as bright as set while off');
  });

  test('a camera turned off draws nothing: the next one draws, and turned on it draws again', () {
    final world = World();
    addTearDown(world.dispose);
    final red = Camera(clearColor: Color.rgb(1, 0, 0));
    final blue = Camera(clearColor: Color.rgb(0, 0, 1));
    world.add(GameObject(components: [red]));
    world.add(GameObject(components: [blue]));
    final view = HeadlessView(world, width: 8, height: 8);
    addTearDown(view.dispose);
    List<int> corner() => view.read(view.frame()).rgba.sublist(0, 3);

    expect(corner(), [255, 0, 0], reason: 'the first camera draws');
    expect(world.camera, same(red));
    red.enabled = false;
    expect(world.camera, same(blue));
    expect(corner(), [0, 0, 255], reason: 'off, the next one draws');
    red.enabled = true;
    expect(corner(), [255, 0, 0]);
  });

  test("a model part's renderer can be changed and turned off and on, and still draws its mesh", () async {
    final world = World();
    addTearDown(world.dispose);
    final model = await Model.load(glb('Box.glb'));
    final box = world.add(model.instantiate());
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 3)),
        components: [Camera(clearColor: Color.rgb(0, 0, 0))],
      ),
    );
    world.add(GameObject(components: [Light.ambient(intensity: 1)]));
    final view = HeadlessView(world, width: 64, height: 64);
    addTearDown(view.dispose);
    List<int> centre() {
      final rgba = view.read(view.frame()).rgba;
      const i = (32 * 64 + 32) * 4;
      return rgba.sublist(i, i + 3);
    }

    final drawn = centre();
    expect(drawn, isNot([0, 0, 0]), reason: 'the box is drawn');
    final part = box.getInChildren<MeshRenderer>()!;
    part.castShadows = false;
    expect(centre(), drawn, reason: 'changed, it still draws its mesh');
    part.enabled = false;
    expect(centre(), [0, 0, 0]);
    part.enabled = true;
    expect(centre(), drawn, reason: 'on again, its mesh');
  });
}
