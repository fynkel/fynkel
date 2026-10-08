import 'dart:math' as math;
import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test('a raycast finds the closest object, where and how far', () {
    final cube = Mesh.cube();
    final material = Material();
    final near = world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, -4)),
        components: [MeshRenderer(cube, material)],
      ),
    );
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, -10)),
        components: [MeshRenderer(cube, material)],
      ),
    );

    final hit = world.raycast(Ray(Vec3.zero, Vec3(0, 0, -5)))!;
    expect(hit.object, near);
    expect(hit.point.closeTo(Vec3(0, 0, -3.5), 1e-5), isTrue, reason: '${hit.point}');
    expect(hit.normal.closeTo(Vec3.back, 1e-5), isTrue, reason: '${hit.normal}');
    expect(hit.distance, closeTo(3.5, 1e-5));

    expect(world.raycast(Ray(Vec3.zero, Vec3.up)), isNull);
    expect(world.raycast(Ray(Vec3.zero, Vec3.forward), maxDistance: 3.4), isNull);
    expect(world.raycast(Ray(Vec3.zero, Vec3.forward), maxDistance: 3.6)?.object, near);

    near.transform = Transform(position: Vec3(5, 0, -4));
    expect(world.raycast(Ray(Vec3.zero, Vec3.forward))?.distance, closeTo(9.5, 1e-5), reason: 'moved, no frame needed');
  });

  test('a ray needs a direction, and a raycast a reach that is not negative', () {
    expect(() => Ray(Vec3.zero, Vec3.zero), throwsArgumentError);
    expect(Ray(Vec3.zero, Vec3(0, 3, 0)).direction, Vec3.up);
    expect(
      () => world.raycast(Ray(Vec3.zero, Vec3.up), maxDistance: -1),
      throwsA(isA<FynkelException>().having((e) => e.status, 'status', FynkelStatus.invalidValue)),
    );
  });

  test('a hit on a model is part of it, and names the node', () async {
    final model = await Model.load(File('../../tests/assets/gltf/tower.glb').readAsBytesSync());
    addTearDown(model.dispose);
    final tower = world.add(model.instantiate(transform: Transform(position: Vec3(0, 0, -10))));
    final hit = world.raycast(Ray(Vec3(0, 0.5, 0), Vec3.forward))!;
    expect(hit.object.isPartOf(tower), isTrue);
    expect(hit.object.name, 'Block 0.3');
  });

  test('rayThrough needs a camera and a view with an area', () {
    final empty = world.add(GameObject(transform: Transform()));
    expect(
      () => empty.rayThrough(const ViewPoint(1, 1, width: 2, height: 2)),
      throwsA(isA<FynkelException>().having((e) => e.status, 'status', FynkelStatus.noComponent)),
    );
    final camera = world.add(GameObject(transform: Transform(), components: [Camera()]));
    expect(() => camera.rayThrough(const ViewPoint(0, 0, width: 0, height: 2)), throwsArgumentError);
  });

  test('rayThrough the centre goes straight ahead from the near plane, and a corner at half the field of view', () {
    final camera = world.add(
      GameObject(
        transform: Transform(position: Vec3(1, 2, 3), rotation: Quat.axisAngle(Vec3.up, math.pi / 2)),
        components: [Camera(fovY: 90, near: 0.5)],
      ),
    );
    final centre = camera.rayThrough(const ViewPoint(100, 50, width: 200, height: 100));
    // Turned a quarter left, the camera looks along -X.
    expect(centre.direction.closeTo(Vec3.left, 1e-6), isTrue, reason: '${centre.direction}');
    expect(centre.origin.closeTo(Vec3(0.5, 2, 3), 1e-6), isTrue, reason: '${centre.origin}');
    // The top edge's middle is 45° up; the top-left corner also 2 × 45°
    // wide in the camera's frame (tan 45° × aspect 2), so 63.4° to the side.
    final top = camera.rayThrough(const ViewPoint(100, 0, width: 200, height: 100));
    expect(top.direction.closeTo(Vec3(-1, 1, 0).normalized, 1e-6), isTrue, reason: '${top.direction}');
    final corner = camera.rayThrough(const ViewPoint(0, 0, width: 200, height: 100));
    // Camera frame (-2, 1, -1): its right is world -Z, so left is +Z.
    expect(corner.direction.closeTo(Vec3(-1, 1, 2).normalized, 1e-6), isTrue, reason: '${corner.direction}');
  });

  test('a ray through each pixel hits the object the view drew there', () {
    // Flat colours, one per object, so a pixel says which object it shows.
    Material flat(Color c) => Material(color: Color.rgb(0, 0, 0), emissive: c, roughness: 1, metallic: 0);
    final red = world.add(
      GameObject(
        transform: Transform(position: Vec3(-1.2, 0.5, 0), rotation: Quat.axisAngle(Vec3(1, 1, 0), 0.6)),
        components: [MeshRenderer(Mesh.cube(), flat(Color.rgb(1, 0, 0)))],
      ),
    );
    final green = world.add(
      GameObject(
        transform: Transform(position: Vec3(1.1, 0.6, -0.5), scale: Vec3(1.5, 1, 1)),
        components: [MeshRenderer(Mesh.sphere(), flat(Color.rgb(0, 1, 0)))],
      ),
    );
    final blue = world.add(
      GameObject(
        transform: Transform(scale: Vec3(6, 1, 3)),
        components: [MeshRenderer(Mesh.plane(), flat(Color.rgb(0, 0, 1)))],
      ),
    );
    // The camera hangs from a turned, moved rig, so its world transform is
    // what counts.
    final rig = world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 1, 0), rotation: Quat.axisAngle(Vec3.up, 0.3)),
      ),
    );
    final camera = world.add(
      GameObject(
        transform: Transform(position: Vec3(0.6, 1.5, 5), lookAt: Vec3(0.3, -0.6, 0)),
        components: [Camera(fovY: 50, clearColor: Color.rgb(0, 0, 0))],
      ),
    );
    camera.parent = rig;

    // Not square, so the aspect counts.
    const width = 96, height = 54;
    final view = HeadlessView(world, width: width, height: height);
    addTearDown(view.dispose);
    final rgba = view.read(view.frame()).rgba;

    // 0: nothing, 1–3: red, green, blue, by the brightest channel.
    int drawn(int x, int y) {
      final i = (y * width + x) * 4;
      final c = [rgba[i], rgba[i + 1], rgba[i + 2]];
      final top = c.reduce(math.max);
      return top < 16 ? 0 : c.indexOf(top) + 1;
    }

    final objects = [red, green, blue];
    final counts = List.filled(4, 0);
    var compared = 0;
    for (var y = 1; y < height - 1; y++) {
      for (var x = 1; x < width - 1; x++) {
        final shown = drawn(x, y);
        // Edge pixels mix two things; compare only those all of whose
        // neighbours show the same.
        var inside = true;
        for (var dy = -1; dy <= 1; dy++) {
          for (var dx = -1; dx <= 1; dx++) {
            if (drawn(x + dx, y + dy) != shown) inside = false;
          }
        }
        if (!inside) continue;
        final hit = world.raycast(
          camera.rayThrough(ViewPoint(x + 0.5, y + 0.5, width: width.toDouble(), height: height.toDouble())),
        );
        final found = hit == null ? 0 : objects.indexOf(hit.object) + 1;
        expect(found, shown, reason: 'pixel ($x, $y)');
        counts[shown]++;
        compared++;
      }
    }
    expect(compared, greaterThan(width * height * 0.7));
    expect(counts, everyElement(greaterThan(50)), reason: 'the background and each object fill some pixels: $counts');
  });
}
