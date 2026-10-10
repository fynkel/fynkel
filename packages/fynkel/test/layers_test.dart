// Layers (docs/stage-4.md §3.9, step 8): each object is on one of 32, and
// cameras, rays and fingers take a mask of the layers they see.

import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'golden.dart';
import 'render_test.dart' show stage1;
import 'tapping.dart';

/// Logs its clicks.
class Clicked extends GameObject {
  Clicked(this.log, {super.transform, super.components, super.layer}) : super(name: 'wall');

  final List<String> log;

  @override
  bool onClick(Finger finger) {
    log.add('click $name');
    return true;
  }
}

/// The stage 1 scene with a blue wall on layer 3 between the camera and
/// the cube, the same as fynkel_ffi's test.
({World world, GameObject cube, GameObject wall, List<String> log}) behindAWall() {
  final scene = stage1();
  final log = <String>[];
  final wall = scene.world.add(
    Clicked(
      log,
      layer: 3,
      transform: Transform(position: Vec3(0, 0.5, 2), scale: Vec3(1.5, 1.5, 0.2)),
      components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFF3A8EE0)))],
    ),
  );
  return (world: scene.world, cube: scene.cube, wall: wall, log: log);
}

void main() {
  group('LayerMask', () {
    test('holds the layers it is made of', () {
      final mask = LayerMask.of([0, 3, 31]);
      expect(mask.value, 1 | 1 << 3 | 1 << 31);
      expect(mask.layers, [0, 3, 31]);
      expect([for (var i = 0; i < 32; i++) mask.contains(i)].where((c) => c).length, 3);
      expect(mask.contains(3), isTrue);
      expect(mask.contains(4), isFalse);
      expect(LayerMask.all.value, 0xFFFFFFFF);
      expect(LayerMask.all.layers.length, 32);
      expect(LayerMask.none.layers, isEmpty);
      expect(LayerMask.of([]), LayerMask.none);
    });

    test('is changed by plus, minus, | and &, and is a value', () {
      final mask = LayerMask.of([1, 2]);
      expect(mask.plus(5), LayerMask.of([1, 2, 5]));
      expect(mask.plus(1), mask);
      expect(mask.minus(1), LayerMask.of([2]));
      expect(mask.minus(9), mask);
      expect(mask | LayerMask.of([7]), LayerMask.of([1, 2, 7]));
      expect(mask & LayerMask.of([2, 7]), LayerMask.of([2]));
      expect(LayerMask.except([4]), LayerMask.all.minus(4));
      expect(LayerMask.except([4]).value, 0xFFFFFFFF & ~(1 << 4));
      expect(LayerMask.except([]), LayerMask.all);
      expect(LayerMask.of([3]).hashCode, LayerMask.of([3]).hashCode);
      expect(LayerMask.of([3]), isNot(LayerMask.of([4])));
    });

    test('says what it is', () {
      expect('${LayerMask.all}', 'LayerMask.all');
      expect('${LayerMask.none}', 'LayerMask.none');
      expect('${LayerMask.of([1, 8])}', 'LayerMask.of([1, 8])');
      expect('${LayerMask.except([2])}', 'LayerMask.except([2])');
      expect('${LayerMask.of([for (var i = 0; i < 16; i++) i])}', startsWith('LayerMask.of('));
      expect(
        '${LayerMask.of([for (var i = 0; i < 17; i++) i])}',
        'LayerMask.except(${[for (var i = 17; i < 32; i++) i]})',
      );
    });

    test('refuses a layer that does not exist', () {
      for (final layer in [-1, 32]) {
        expect(() => LayerMask.of([layer]), throwsRangeError);
        expect(() => LayerMask.except([layer]), throwsRangeError);
        expect(() => LayerMask.all.contains(layer), throwsRangeError);
        expect(() => LayerMask.all.plus(layer), throwsRangeError);
        expect(() => LayerMask.all.minus(layer), throwsRangeError);
      }
      expect(
        () => LayerMask.of([32]),
        throwsA(isA<RangeError>().having((e) => e.message, 'message', 'layers are 0 to 31')),
      );
    });
  });

  group('GameObject.layer', () {
    late World world;
    setUp(() => world = World());
    tearDown(() => world.dispose());

    test('is 0 until set, before or after the object is added, and a child keeps its own', () {
      final child = GameObject();
      final parent = GameObject(layer: 5, children: [child]);
      expect((parent.layer, child.layer), (5, 0));
      parent.layer = 6;
      world.add(parent);
      expect((parent.layer, child.layer), (6, 0));
      expect(world.raycast(Ray(Vec3.zero, Vec3.up)), isNull);
      child.layer = 31;
      expect(child.layer, 31);
      parent.layer = 0;
      expect(parent.layer, 0);
    });

    test('is what rays and cameras see in the engine', () {
      final cube = world.add(GameObject(components: [MeshRenderer(Mesh.cube(), Material())]));
      final ray = Ray(Vec3(0, 0, 5), Vec3.forward);
      cube.layer = 9;
      expect(world.raycast(ray, layers: LayerMask.of([9]))?.object, cube);
      expect(world.raycast(ray, layers: LayerMask.except([9])), isNull);
      final early = GameObject(layer: 9, components: [MeshRenderer(Mesh.cube(), Material())]);
      cube.destroy();
      world.update(0);
      world.add(early);
      expect(world.raycast(ray, layers: LayerMask.of([9]))?.object, early, reason: 'set before it was added');
      expect(world.raycast(ray, layers: LayerMask.of([0])), isNull);
    });

    test('past 31 or below 0 is refused, and nothing changes', () {
      final object = world.add(GameObject(layer: 2));
      for (final layer in [-1, 32]) {
        expect(() => object.layer = layer, throwsRangeError);
        expect(() => GameObject(layer: layer), throwsRangeError);
      }
      expect(object.layer, 2);
    });

    test('of a destroyed object cannot be set', () {
      final object = world.add(GameObject());
      object.destroy();
      world.update(0);
      expect(() => object.layer = 1, throwsStateError);
    });
  });

  group('masks', () {
    test('a camera does not draw the layers left out of its culling mask', () {
      final scene = behindAWall();
      final world = scene.world;
      addTearDown(world.dispose);
      final view = HeadlessView(world, width: 256, height: 256);
      addTearDown(view.dispose);
      final camera = world.camera!.get<Camera>()!;
      expect(camera.cullingMask, LayerMask.all);

      bool blueAt(({int number, Uint8List rgba}) frame, int x, int y) {
        final i = (y * 256 + x) * 4;
        final [r, g, b] = frame.rgba.sublist(i, i + 3);
        return b > r && b > g;
      }

      expect(blueAt(view.read(view.frame()), 128, 128), isTrue, reason: 'the wall hides the cube');
      camera.cullingMask = LayerMask.except([3]);
      expectGolden('cube', view.read(view.frame()).rgba, 256, 256);
      camera.cullingMask = LayerMask.of([3]);
      final frame = view.read(view.frame());
      expect(blueAt(frame, 128, 128), isTrue, reason: 'only the wall');
      expect(frame.rgba.sublist(0, 3).every((c) => c < 40), isTrue, reason: 'and the clear colour');
      expect('$camera', contains('cullingMask: LayerMask.of([3])'));
      expect('${Camera()}', isNot(contains('cullingMask')));
    });

    test('a ray goes through the layers left out of its mask', () {
      final scene = behindAWall();
      addTearDown(scene.world.dispose);
      RayHit? hit(LayerMask layers) => scene.world.raycast(Ray(Vec3(0.1, 0.2, 5), Vec3.forward), layers: layers);
      expect(hit(LayerMask.all)?.object, scene.wall);
      expect(hit(LayerMask.except([3]))?.object, scene.cube);
      expect(hit(LayerMask.of([0]))?.object, scene.cube);
      expect(hit(LayerMask.none), isNull);
    });

    test("a finger touches only the layers the camera draws and the input's mask has", () {
      final scene = behindAWall();
      final world = scene.world;
      addTearDown(world.dispose);
      final camera = world.camera!.get<Camera>()!;
      final middle = ViewPoint(200, 200, width: 400, height: 400);
      void over() {
        tap(world, middle);
        world.update(1 / 60);
      }

      over();
      expect(scene.log, ['click wall']);
      scene.log.clear();

      world.input.layers = LayerMask.except([3]);
      over();
      expect(scene.log, isEmpty, reason: 'the input leaves the wall out');

      world.input.layers = LayerMask.all;
      camera.cullingMask = LayerMask.except([3]);
      over();
      expect(scene.log, isEmpty, reason: 'the camera does not draw it');

      camera.cullingMask = LayerMask.all;
      over();
      expect(scene.log, ['click wall']);
    });

    test('a culling mask the engine cannot take is undone', () {
      final world = World();
      final lens = world.add(GameObject(components: [Camera()])).get<Camera>()!;
      world.dispose();
      expect(() => lens.cullingMask = LayerMask.none, throwsStateError);
      expect(lens.cullingMask, LayerMask.all);
    });

    test("a camera's culling mask is read back from one the engine made", () {
      final world = World();
      addTearDown(world.dispose);
      final camera = world.add(
        GameObject(
          components: [
            Camera(cullingMask: LayerMask.of([1])),
          ],
        ),
      );
      expect(camera.get<Camera>()!.cullingMask, LayerMask.of([1]));
    });
  });
}
