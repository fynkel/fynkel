// 'Building meshes': the shapes with their settings, the extruded letter
// with its holes, the lathed vase and the combined town match the golden
// (tests/golden/meshes_build.png; FYNKEL_BLESS=1 writes it).

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/scenes/meshes_build.dart';

import 'golden.dart';

void main() {
  test('Building meshes: shapes, a letter, a vase and a town match their golden', () {
    final world = World()..quality = Quality.high;
    addTearDown(world.dispose);
    final scene = world.add(MeshesBuildScene(still: true));
    world.update(1 / 60);

    final town = scene.town!.get<MeshRenderer>()!;
    expect(town.mesh!.submeshCount, 2, reason: 'walls and roofs: a draw each for the whole town');
    expect(town.materials, hasLength(2));
    expect(scene.houses, greaterThan(8));
    expect(scene.letter.get<MeshRenderer>()!.mesh!.vertexCount, greaterThan(100));

    final view = HeadlessView(world, width: 320, height: 200);
    addTearDown(view.dispose);
    expectGolden('meshes_build', view.read(view.frame()).rgba, 320, 200);

    // The sliders make the shapes again.
    final before = scene.find('sphere')!.get<MeshRenderer>()!.mesh!.vertexCount;
    scene
      ..segments = 32
      ..remake();
    expect(scene.find('sphere')!.get<MeshRenderer>()!.mesh!.vertexCount, greaterThan(before));
  });
}
