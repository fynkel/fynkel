// A world lets go of a mesh, material or texture once nothing reaches it
// (docs/stage-4.md, step 9).

import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'gc.dart';
import 'load_test.dart' show Floor, Source;
import 'model_test.dart' show glb;

void main() {
  final collect = garbageCollector();

  late Source source;
  late World world;
  late HeadlessView view;
  setUp(() {
    source = Source();
    world = World(source: source.call);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 5)),
        components: [Camera()],
      ),
    );
    view = HeadlessView(world, width: 16, height: 16);
  });
  tearDown(() {
    view.dispose();
    world.dispose();
  });

  /// What the view has on the GPU after a frame: mesh and texture bytes.
  (int, int) drawn() {
    world.update(1 / 60);
    view.read(view.frame());
    return (view.stats.meshBytes, view.stats.textureBytes);
  }

  // A function of its own, so no local of the test keeps the part.
  Material? materialOf(GameObject root) =>
      root.children.expand((c) => [c, ...c.children]).map((c) => c.get<MeshRenderer>()?.material).nonNulls.firstOrNull;

  Mesh triangle() => Mesh.custom(positions: [Vec3.zero, Vec3(1, 0, 0), Vec3(0, 1, 0)], indices: [0, 1, 2]);
  Texture2D pixel() => Texture2D.rgba(1, 1, Uint8List.fromList([255, 0, 0, 255]));

  test('what nothing reaches any more leaves the world once collected: memory comes back', () async {
    final before = drawn();
    GameObject? object = world.add(
      GameObject(components: [MeshRenderer(triangle(), Material(baseColorTexture: pixel()))]),
    );
    final loaded = drawn();
    expect(loaded.$1, greaterThan(before.$1));
    expect(loaded.$2, before.$2 + 4);

    await collect();
    expect(drawn(), loaded, reason: 'the object still uses them');
    object.destroy();
    object = null;
    expect(drawn(), loaded, reason: 'not collected yet');
    await collect();
    expect(drawn(), before);
  });

  test('a resource the game still holds stays, and can be used again', () async {
    final mesh = triangle();
    final material = Material(baseColorTexture: pixel());
    world.add(GameObject(components: [MeshRenderer(mesh, material)])).destroy();
    final loaded = drawn();
    await collect();
    expect(drawn(), loaded);
    final again = world.add(GameObject(components: [MeshRenderer(mesh, material)]));
    expect(drawn(), loaded);
    expect(again.get<MeshRenderer>()!.material, same(material));
  });

  test("a model's copies keep its meshes and materials while they're there", () async {
    final before = drawn();
    GameObject? copy = world.add((await Model.load(glb('BoxTextured.glb'))).instantiate());
    final loaded = drawn();
    expect(loaded.$1, greaterThan(before.$1));
    expect(loaded.$2, greaterThan(before.$2));

    await collect();
    expect(drawn(), loaded, reason: 'the copy holds the model');
    expect(materialOf(copy), isNotNull, reason: 'read from the engine, after the collection');
    copy.destroy();
    copy = null;
    expect(drawn(), loaded, reason: 'not collected yet');
    await collect();
    expect(drawn(), before);
  });

  test("an asset's texture is freed with its last holder, taking out the materials that use it", () async {
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    // The plane the floor uses is built in, and stays.
    final before = drawn().$2;
    final floor = world.add(Floor());
    await floor.loaded;
    expect(drawn().$2, greaterThan(before));
    final material = floor.get<MeshRenderer>()!.material!;

    floor.destroy();
    expect(drawn().$2, before, reason: 'no object draws with the material, so the texture could go');
    expect(errors, isEmpty);
    expect(
      () => world.add(GameObject(components: [MeshRenderer(Mesh.plane(), material)])),
      throwsA(isA<StateError>().having((e) => e.message, 'message', contains('disposed'))),
      reason: 'the material comes back when used again, but its texture is gone',
    );
  });

  test('an asset texture a drawn material still uses goes when the material is collected', () async {
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    final before = drawn().$2;
    final holder = world.add(GameObject());
    // In a function of its own, so no local of the test keeps the texture.
    Future<GameObject> show() async {
      final grid = await holder.assets.texture('grid.png');
      return world.add(GameObject(components: [MeshRenderer(Mesh.plane(), Material(baseColorTexture: grid))]));
    }

    GameObject? shown = await show();
    final loaded = drawn().$2;
    expect(loaded, greaterThan(before));

    holder.destroy();
    expect(drawn().$2, loaded, reason: 'an object still draws with it');
    expect(errors, isEmpty);
    shown.destroy();
    shown = null;
    expect(drawn().$2, loaded, reason: 'not collected yet');
    await collect();
    expect(drawn().$2, before);
  });

  group('a world lets go of each built-in system\'s components (DC5)', () {
    // What each built-in keeps of an object: a component, or the object's
    // animator. Each made in a function of its own, so no local keeps it.
    final kinds = <String, Object Function(GameObject object)>{
      'particle system': (o) => o.addComponent(ParticleSystem()),
      'particle system waiting for its plane': (o) =>
          o.addComponent(ParticleSystem(collision: ParticleCollision(planes: [GameObject()]))),
      'trail': (o) => o.addComponent(TrailRenderer(time: 1)),
      'light': (o) => o.addComponent(Light.point()),
      'camera': (o) => o.addComponent(Camera()),
    };

    for (final MapEntry(key: kind, value: put) in kinds.entries) {
      test('a $kind, destroyed, removed, or with its world disposed', () async {
        WeakReference<Object> destroyed() {
          final object = world.add(GameObject());
          final made = put(object);
          object.destroy();
          return WeakReference(made);
        }

        WeakReference<Object> removed() {
          final object = world.add(GameObject());
          final made = put(object) as Component;
          object.removeComponent(made);
          return WeakReference(made);
        }

        final other = World();
        WeakReference<Object> disposed() {
          final made = put(other.add(GameObject()));
          other.update(1 / 60);
          return WeakReference(made);
        }

        final gone = [destroyed(), removed(), disposed()];
        world.update(1 / 60);
        other.dispose();
        await collect();
        expect([for (final g in gone) g.target == null], [true, true, true], reason: 'destroyed, removed, disposed');
      });
    }

    test("an animated model's animator, destroyed, or with its world disposed", () async {
      final truck = await Model.load(glb('CesiumMilkTruck.glb'));
      addTearDown(truck.dispose);
      WeakReference<Object> destroyed() {
        final copy = world.add(truck.instantiate());
        final animator = copy.animator!..play('Wheels');
        copy.destroy();
        return WeakReference(animator);
      }

      final other = World();
      WeakReference<Object> disposed() {
        final animator = other.add(truck.instantiate()).animator!..play('Wheels');
        other.update(1 / 60);
        return WeakReference(animator);
      }

      final gone = [destroyed(), disposed()];
      world.update(1 / 60);
      other.dispose();
      await collect();
      expect([for (final g in gone) g.target == null], [true, true], reason: 'destroyed, disposed');
    });
  });
}
