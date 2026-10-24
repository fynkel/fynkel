// 'Meshes that change': the flag and the water as they're made, the
// ground dug in two places, its collider following, match the golden
// (tests/golden/meshes_change.png; FYNKEL_BLESS=1 writes it).

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:lab/scenes/meshes_change.dart';

import 'golden.dart';

void main() {
  test('Meshes that change: dug ground, its collider following, matches its golden', () {
    final world = World()..quality = Quality.high;
    addTearDown(world.dispose);
    final scene = world.add(MeshesChangeScene(still: true));
    world.update(1 / 60);
    final ground = scene.ground;
    double? depthAt(Vec3 at) => world.physics.raycast(Ray(at + Vec3(0, 3, 0), Vec3(0, -1, 0)))?.point.y;
    final middle = ground.position + Vec3(0.3, 0, 0.2);
    expect(depthAt(middle), closeTo(Ground.top, 1e-3));

    ground
      ..dig(middle)
      ..dig(ground.position + Vec3(-0.6, 0, -0.5));
    world.update(1 / 60);
    expect(depthAt(middle)!, lessThan(Ground.top - 0.1), reason: 'the collider follows the dug mesh');
    expect(scene.water.mesh.vertexCount, 101 * 101);
    expect(scene.flag.mesh.submeshCount, 2);

    final view = HeadlessView(world, width: 320, height: 200);
    addTearDown(view.dispose);
    expectGolden('meshes_change', view.read(view.frame()).rgba, 320, 200);
  });
}
