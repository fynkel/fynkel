import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'golden.dart';
import 'mesh_test.dart' show throwsFynkel;

final _models = Directory.current.uri.resolve('../../tests/assets/gltf/');
Uint8List glb(String name) => File.fromUri(_models.resolve(name)).readAsBytesSync();

/// A float the Rust test printed as its bits, so the camera is exactly
/// the one it used.
double f32(int bits) => (ByteData(4)..setUint32(0, bits)).getFloat32(0);

/// crates/fynkel_ffi/tests/models.rs's `render`, with the bounds it
/// computes from the model's vertices written in: the camera looks at the
/// centre from [from], far enough to fit the bounding sphere, under a sun
/// from over its left shoulder.
Future<Uint8List> render(String name, Vec3 centre, double radius, Vec3 from) async {
  final world = World();
  addTearDown(world.dispose);
  final model = await Model.load(glb(name));
  world.add(model.instantiate());
  const fov = 40.0;
  final distance = radius / math.sin(fov * math.pi / 180 / 2) * 1.05;
  final camera = Transform(position: centre + from.normalized * distance, lookAt: centre);
  world.add(
    GameObject(
      transform: camera,
      components: [
        Camera(fovY: fov, near: radius * 0.05, far: radius * 8 + 10, clearColor: Color.rgb(0.16, 0.17, 0.2)),
      ],
    ),
  );
  final shoulder = camera.rotation.rotate(Vec3(-0.5, 0.8, 0.6));
  world.add(
    GameObject(
      transform: Transform(forward: -shoulder),
      components: [Light.sun()],
    ),
  );
  world.add(GameObject(components: [Light.ambient(intensity: 0.25)]));
  final view = HeadlessView(world, width: 256, height: 256);
  addTearDown(view.dispose);
  return view.read(view.frame()).rgba;
}

void main() {
  test('a textured box renders to the same golden image as the Rust test', () async {
    final frame = await render('BoxTextured.glb', Vec3.zero, f32(1063105498), Vec3(1, 0.8, 1.4));
    expectGolden('gltf_box_textured', frame, 256, 256);
  });

  test('the 138-node tower, with its mirrored shape, renders to the same golden image as the Rust test', () async {
    final frame = await render('tower.glb', Vec3(0, f32(1085223731), 0), f32(1091038729), Vec3(0.3, 0.5, 1));
    expectGolden('gltf_tower', frame, 256, 256);
  });

  test("a model's parts: meshes, shared textures, named materials, and what was skipped", () async {
    final truck = await Model.load(glb('CesiumMilkTruck.glb'));
    expect(truck.meshes, hasLength(4));
    expect(truck.textures, hasLength(1));
    expect(truck.materials.map((m) => m.name), ['truck', 'glass', 'window_trim', 'wheels']);
    expect(truck.warnings, ['animations are skipped; the model shows its rest pose']);
    // One image, used by the body's and the wheels' materials.
    final [body, glass, _, wheels] = truck.materials;
    expect(body.baseColorTexture, same(truck.textures.single));
    expect(wheels.baseColorTexture, same(truck.textures.single));
    expect(glass.baseColorTexture, isNull);
    expect(body.toString(), "Material('truck')");

    final alpha = await Model.load(glb('AlphaBlendModeTest.glb'));
    final cutoff = alpha.materials.firstWhere((m) => m.name == 'MatCutoff25');
    expect((cutoff.alphaCutoff, cutoff.doubleSided), (0.25, true));
    // Four images; one packs metallic-roughness and occlusion, as glTF does.
    final bed = alpha.materials.firstWhere((m) => m.name == 'MatBed');
    final t = alpha.textures;
    expect(
      [bed.baseColorTexture, bed.normalTexture, bed.metallicRoughnessTexture, bed.occlusionTexture],
      [same(t[2]), same(t[0]), same(t[1]), same(t[1])],
    );
    expect(bed.emissiveTexture, isNull);
    expect((await Model.load(glb('Box.glb'))).warnings, isEmpty);
  });

  test('a spawned model is a tree of named objects drawing its meshes and materials', () async {
    final world = World();
    addTearDown(world.dispose);
    final truck = await Model.load(glb('CesiumMilkTruck.glb'));
    final root = world.add(truck.instantiate(transform: Transform(position: Vec3(2, 0, 0))));
    expect(root.name, 'Scene');
    expect(root.transform.position, Vec3(2, 0, 0));
    final body = root.find('Cesium_Milk_Truck')!;
    final parts = body.children.where((c) => c.name == null).toList();
    expect(parts.map((p) => p.meshRenderer.material), [for (final m in truck.materials.take(3)) same(m)]);
    final wheel = root.find('Wheels')!.meshRenderer;
    expect(wheel.mesh, same(truck.meshes[3]));
    expect(root.find('Wheels.001')!.meshRenderer.mesh, same(wheel.mesh));
  });

  test('copies of a model share its GPU data and its materials', () async {
    final world = World();
    addTearDown(world.dispose);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 2, 12), lookAt: Vec3.zero),
        components: [Camera()],
      ),
    );
    world.add(GameObject(components: [Light.ambient()]));
    final truck = await Model.load(glb('CesiumMilkTruck.glb'));
    final first = world.add(truck.instantiate(transform: Transform(position: Vec3(-3, 0, 0))));
    final view = HeadlessView(world, width: 64, height: 64);
    addTearDown(view.dispose);
    view.read(view.frame());
    final once = (view.stats.meshBytes, view.stats.textureBytes);
    expect(once.$1, greaterThan(0));
    expect(once.$2, greaterThan(0));

    final second = world.add(truck.instantiate(transform: Transform(position: Vec3(3, 0, 0))));
    view.read(view.frame());
    expect((view.stats.meshBytes, view.stats.textureBytes), once);

    // The same material objects, so changing one changes both copies.
    final glass = truck.materials[1];
    for (final copy in [first, second]) {
      final parts = copy.find('Cesium_Milk_Truck')!.children.where((c) => c.name == null);
      expect(parts.where((p) => identical(p.meshRenderer.material, glass)), hasLength(1));
    }

    // A disposed model can't be spawned again; what was spawned stays.
    truck.dispose();
    expect(() => world.add(truck.instantiate()), throwsStateError);
    expect(first.find('Wheels')!.isAlive, isTrue);
  });

  test('a file that cannot be read is refused, saying why', () async {
    await expectLater(
      Model.load(Uint8List(0)),
      throwsFynkel(FynkelStatus.invalidValue, 'the model is empty (0 bytes)'),
    );
    await expectLater(
      Model.load(Uint8List.fromList('{"asset": {"version": "2.0"}}'.codeUnits)),
      throwsFynkel(FynkelStatus.invalidValue, 'this is a .gltf (JSON) file'),
    );
    final box = glb('Box.glb');
    await expectLater(
      Model.load(Uint8List.sublistView(box, 0, box.length ~/ 2)),
      throwsFynkel(FynkelStatus.invalidValue, 'the .glb is broken'),
    );
  });
}
