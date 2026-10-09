// Objects the game makes and extends (docs/stage-3.md §3.1, step 1).

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'model_test.dart' show glb;
import 'world_test.dart' show throwsFynkel;

/// A prefab: its own fields, its parts added in its constructor.
class Robot extends GameObject {
  Robot({super.transform}) {
    add(
      GameObject(
        name: 'arm',
        transform: Transform(position: Vec3(1, 0, 0)),
        components: [_cube()],
      ),
    );
  }

  int hits = 0;
}

class Enemy extends GameObject {}

class Boss extends Enemy {}

class Level extends GameObject {}

MeshRenderer _cube() => MeshRenderer(Mesh.cube(), Material());

Matcher throwsStateErrorSaying(String text) =>
    throwsA(isA<StateError>().having((e) => e.message, 'message', contains(text)));

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test('a subclass comes back as itself from a ray, find, children and parent, fields and all', () {
    final robot = world.add(Robot(transform: Transform(position: Vec3(0, 0, -5))));
    robot.hits = 3;

    final hit = world.raycast(Ray(Vec3(1, 0, 0), Vec3(0, 0, -1)))!;
    expect(hit.object.name, 'arm');
    expect(hit.object.ancestor<Robot>(), same(robot));
    expect(hit.object.ancestor<Robot>()!.hits, 3);

    final arm = robot.find('arm')!;
    expect(arm, same(hit.object));
    expect(robot.find('arm'), same(arm), reason: 'the same instance every time');
    expect(robot.children.single, same(arm));
    expect(arm.parent, same(robot));
  });

  test("an object describes itself until it's added, and touching the world before then says so", () {
    final light = GameObject(
      name: 'sun',
      transform: Transform(position: Vec3(0, 3, 0)),
      components: [Light.sun()],
    );
    expect(light.name, 'sun');
    expect(light.transform.position, Vec3(0, 3, 0));
    expect(light.get<Light>()!.intensity, Light.sun().intensity);
    expect(light.get<Camera>(), isNull);
    light
      ..name = 'moon'
      ..get<Light>()!.kind = LightKind.ambient
      ..addComponent(Camera(fovY: 40));
    light.removeComponent(light.get<Camera>()!);
    expect(light.get<Camera>(), isNull);

    for (final touch in <void Function()>[
      () => light.world,
      () => light.id,
      () => light.parent,
      () => light.children,
      () => light.find('x'),
      () => light.worldTransform,
    ]) {
      expect(touch, throwsStateErrorSaying('onLoad'));
    }
    expect(light.isAlive, isFalse);
    expect(light.toString(), 'GameObject (not added)');

    world.add(light);
    expect(light.isAlive, isTrue);
    expect(light.name, 'moon');
    expect(light.transform.position, Vec3(0, 3, 0));
    expect(light.get<Light>()!.kind, LightKind.ambient);
    expect(light.get<Camera>(), isNull);
  });

  test('objects added to one that is not added yet become live with it, in order', () {
    final root = GameObject(name: 'root');
    final a = root.add(GameObject(name: 'a'));
    final b = root.add(GameObject(name: 'b'));
    final c = a.add(GameObject(name: 'c'));
    expect([a, b, c].any((o) => o.isAlive), isFalse);

    world.add(root);
    expect([root, a, b, c].every((o) => o.isAlive), isTrue);
    expect(root.children, [a, b]);
    expect(c.parent, same(a));
    expect(root.find('c'), same(c));

    // Added to a live object: live at once.
    final d = b.add(GameObject(name: 'd'));
    expect(d.isAlive, isTrue);
    expect(d.parent, same(b));
  });

  test('an object is added once, never below itself, and never after it is destroyed', () {
    final a = world.add(GameObject());
    expect(() => world.add(a), throwsStateErrorSaying('already added'));
    expect(() => GameObject().add(a), throwsStateErrorSaying('already added'));

    final queued = GameObject();
    final holder = GameObject()..add(queued);
    expect(() => world.add(queued), throwsStateErrorSaying('already added'), reason: 'it waits in holder');

    final outer = GameObject();
    final inner = outer.add(GameObject());
    expect(() => inner.add(outer), throwsStateErrorSaying('below itself'));
    expect(() => outer.add(outer), throwsStateErrorSaying('below itself'));

    a.destroy();
    expect(() => world.add(a), throwsStateErrorSaying('destroyed'));
    expect(() => a.add(GameObject()), throwsStateErrorSaying('destroyed'));
    world.add(holder);
    expect(queued.parent, same(holder));
  });

  test('destroy takes everything below along, only once, and a queued object never comes', () {
    final keep = world.add(GameObject());
    final root = keep.add(Robot());
    final arm = root.find('arm')!;
    root.destroy();
    expect([root, arm].map((o) => (o.isAlive, o.isDestroyed)), everyElement((false, true)));
    expect(keep.isAlive, isTrue);
    expect(keep.children, isEmpty);
    expect(() => root.find('arm'), throwsFynkel(FynkelStatus.noObject));
    root.destroy(); // again: nothing

    final parent = GameObject();
    final child = parent.add(GameObject(name: 'gone'));
    child.destroy();
    world.add(parent);
    expect(parent.children, isEmpty);
    expect(child.isAlive, isFalse);
  });

  test('all, first and ancestor find objects by class, subclasses included, and forget destroyed ones', () {
    expect(world.all<Enemy>(), isEmpty);
    expect(world.first<Enemy>(), isNull);

    final level = world.add(Level());
    final grunt = level.add(Enemy());
    final boss = level.add(Boss());
    final robot = world.add(Robot());

    expect(world.all<Enemy>(), unorderedEquals([grunt, boss]));
    expect(world.all<Boss>(), [boss]);
    expect(world.first<Boss>(), same(boss));
    expect(world.first<Robot>(), same(robot));
    expect(world.all<GameObject>(), hasLength(5), reason: "every object the game added, robot's arm too");
    expect(boss.ancestor<Level>(), same(level));
    expect(boss.ancestor<Robot>(), isNull);
    expect(level.ancestor<Level>(), isNull, reason: 'not the object itself');

    level.destroy();
    expect(world.all<Enemy>(), isEmpty);
    expect(world.first<Boss>(), isNull);
    expect(world.all<GameObject>(), hasLength(2));
  });

  test("a model's copies are objects to add; their parts come back as the same plain objects", () async {
    final model = await Model.load(glb('CesiumMilkTruck.glb'));
    final copy = model.instantiate(transform: Transform(position: Vec3(2, 0, 0)));
    expect(copy.isAlive, isFalse);
    world.add(copy);
    expect(copy.transform.position, Vec3(2, 0, 0));
    expect(copy.children, isNotEmpty);
    final part = copy.children.first;
    expect(copy.children.first, same(part));
    expect(part.runtimeType, GameObject);
    expect(world.all<GameObject>(), [copy], reason: "a model's parts aren't the game's own objects");

    final second = world.add(model.instantiate());
    expect(second.children.length, copy.children.length);
    model.dispose();
    expect(model.instantiate, throwsStateError);
  });

  test('a refused component leaves the object unadded, to be fixed and added again', () {
    expect(() => GameObject(components: [Light.sun(), Light.ambient()]), throwsArgumentError);

    final camera = GameObject(components: [Camera(fovY: 500)]);
    expect(() => world.add(camera), throwsFynkel(FynkelStatus.invalidValue, 'fov_y'));
    expect(camera.isAlive, isFalse);
    camera.get<Camera>()!.fovY = 60;
    world.add(camera);
    expect(camera.get<Camera>()!.fovY, 60);
  });

  test('an inactive object and everything below it are not hit, and each keeps its own flag', () {
    final behind = world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, -8)),
        components: [_cube()],
      ),
    );
    final robot = world.add(Robot(transform: Transform(position: Vec3(-1, 0, -3))));
    final arm = robot.find('arm')!;
    final ray = Ray(Vec3.zero, Vec3(0, 0, -1));
    expect(world.raycast(ray)!.object, same(arm));

    robot.active = false;
    expect(world.raycast(ray)!.object, same(behind));
    expect((robot.active, robot.activeInHierarchy), (false, false));
    expect((arm.active, arm.activeInHierarchy), (true, false), reason: 'hidden by its parent, its own flag kept');

    arm.active = false;
    robot.active = true;
    expect(world.raycast(ray)!.object, same(behind), reason: 'the arm is still off');
    arm.active = true;
    expect(world.raycast(ray)!.object, same(arm));
  });

  test('an object can be made inactive before it is added, and so can its children', () {
    final robot = Robot(transform: Transform(position: Vec3(-1, 0, -3)))..active = false;
    expect(robot.active, isFalse);
    expect(() => robot.activeInHierarchy, throwsStateErrorSaying('onLoad'));
    final queued = robot.add(GameObject(name: 'off')..active = false);

    world.add(robot);
    expect(robot.active, isFalse);
    expect(queued.active, isFalse);
    expect(robot.find('arm')!.active, isTrue);
    expect(world.raycast(Ray(Vec3.zero, Vec3(0, 0, -1))), isNull);
    robot.active = true;
    expect(world.raycast(Ray(Vec3.zero, Vec3(0, 0, -1)))!.object.name, 'arm');
  });
}
