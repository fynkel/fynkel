// Stage 14's API pass over the core: what a game is told when it gets
// something wrong (docs/stage-14-findings.md, C1, C2 and C6 to C13). Each
// message names what the game wrote, in Dart's names and the game's
// units, and says what to do.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Health extends Behaviour {}

class Shot extends GameObject {}

class Level extends GameObject {}

/// An [ArgumentError] whose text has each of [parts].
Matcher throwsArgumentSaying(List<String> parts) =>
    throwsA(isA<ArgumentError>().having((e) => '$e', 'text', allOf([for (final p in parts) contains(p)])));

/// A [StateError] whose text has each of [parts].
Matcher throwsStateSaying(List<String> parts) =>
    throwsA(isA<StateError>().having((e) => e.message, 'message', allOf([for (final p in parts) contains(p)])));

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  group("values the engine checks are checked in Dart, in the game's names and units (C1)", () {
    test('a camera', () {
      final camera = world.add(GameObject(name: 'eye', components: [Camera()])).get<Camera>()!;
      expect(
        () => camera.fovY = 60,
        throwsArgumentSaying(['Camera.fovY', "'eye'", 'is 60', 'radians', 'did you mean 60.deg?']),
      );
      expect(camera.fovY, 60.deg, reason: 'the old value stays');
      expect(() => camera.fovY = 0.001, throwsArgumentSaying(['Camera.fovY', '1° to 179°']));
      expect(() => camera.near = 0, throwsArgumentSaying(['Camera.near', 'is 0', 'metres']));
      expect(() => camera.far = double.nan, throwsArgumentSaying(['Camera.far', 'NaN']));
      expect(() => camera.orthographicSize = 0, throwsArgumentSaying(['Camera.orthographicSize', 'is 0']));
      expect(() => camera.bloom = const Bloom(strength: 20), throwsArgumentSaying(['Bloom.strength', 'is 20']));
      expect(
        () => camera.ambientOcclusion = const AmbientOcclusion(radius: -1),
        throwsArgumentSaying(['AmbientOcclusion.radius', 'is -1']),
      );
      expect(() => camera.clearColor = Color.rgb(double.nan, 0, 0), throwsArgumentSaying(['Camera.clearColor', 'NaN']));
      final error = (() {
        try {
          camera.near = -1;
        } on ArgumentError catch (e) {
          return e;
        }
      })();
      expect(error?.name, 'near', reason: 'a debugger shows the parameter');
    });

    test('a light', () {
      final light = world.add(GameObject(name: 'lamp', components: [Light.spot()])).get<Light>()!;
      expect(
        () => light.angle = 40,
        throwsArgumentSaying(['Light.angle', "'lamp'", 'radians', 'did you mean 40.deg?']),
      );
      expect(() => light.angle = 3.5, throwsArgumentSaying(['Light.angle', '0° to 179°']));
      expect(() => light.intensity = -1, throwsArgumentSaying(['Light.intensity', 'is -1']));
      expect(() => light.range = -1, throwsArgumentSaying(['Light.range', 'is -1', 'metres']));
      expect(() => light.softness = 2, throwsArgumentSaying(['Light.softness', 'is 2', '0 to 1']));
      expect(light.intensity, 1);
    });

    test("a renderer's own colour", () {
      final renderer = world.add(GameObject(name: 'rock', components: [MeshRenderer()])).get<MeshRenderer>()!;
      expect(
        () => renderer.color = Color.rgb(0, double.infinity, 0),
        throwsArgumentSaying(['MeshRenderer.color', "'rock'", 'NaN or infinity']),
      );
      expect(renderer.color, isNull);
    });

    test("a world's quality", () {
      expect(
        () => world.quality = Quality.high.copyWith(shadowCascades: 5),
        throwsArgumentSaying(['Quality.shadowCascades', 'is 5', '1 to 3']),
      );
      expect(
        () => world.quality = Quality.high.copyWith(shadowResolution: 1000),
        throwsArgumentSaying(['Quality.shadowResolution', 'is 1000', 'power of two']),
      );
      expect(
        () => world.quality = Quality.high.copyWith(spotShadowResolution: 8192),
        throwsArgumentSaying(['Quality.spotShadowResolution', 'is 8192']),
      );
      expect(
        () => world.quality = Quality.high.copyWith(spotShadows: 9),
        throwsArgumentSaying(['Quality.spotShadows', 'is 9', '0 to 4']),
      );
      expect(
        () => world.quality = Quality.high.copyWith(shadowDistance: 0),
        throwsArgumentSaying(['Quality.shadowDistance', 'is 0']),
      );
      expect(world.quality, isNull, reason: 'left as it was');
      world.quality = Quality.high.copyWith(shadows: false, shadowCascades: 5);
    });

    test('a parent below the object names both objects', () {
      final a = world.add(GameObject(name: 'a'));
      final b = a.add(GameObject(name: 'b'));
      expect(() => a.parent = b, throwsArgumentSaying(["'a'", "'b'", 'below']));
      expect(() => a.parent = a, throwsArgumentSaying(["'a'"]));
      expect(b.parent, same(a));
    });
  });

  group('values are checked where they are made, before the object is added (C2)', () {
    test('components', () {
      expect(() => Camera(fovY: 60), throwsArgumentSaying(['Camera.fovY', 'did you mean 60.deg?']));
      expect(() => Light.point(intensity: -1), throwsArgumentSaying(['Light.intensity', 'is -1']));
      expect(() => Light.spot(angle: 45), throwsArgumentSaying(['Light.angle', 'did you mean 45.deg?']));
      expect(() => Camera(bloom: const Bloom(threshold: -1)), throwsArgumentSaying(['Bloom.threshold']));
      final camera = Camera();
      final object = GameObject(name: 'eye', components: [camera]);
      expect(() => camera.near = 0, throwsArgumentSaying(['Camera.near', "'eye'"]));
      expect(camera.near, 0.1);
      world.add(object);
    });

    test('transforms', () {
      expect(
        () => GameObject(transform: Transform(rotation: Quat(0, 0, 0, 2))),
        throwsArgumentSaying(['rotation', 'Quat(0.0, 0.0, 0.0, 2.0)', 'length 2', '.normalized']),
      );
      final o = GameObject(name: 'thing');
      expect(
        () => o.transform = Transform(position: Vec3(double.nan, 0, 0)),
        throwsArgumentSaying(["'thing'", 'position', 'NaN']),
      );
      expect(() => o.position = Vec3(double.nan, 0, 0), throwsArgumentSaying(['position', 'NaN']));
      expect(() => o.localScale = Vec3(1, double.infinity, 1), throwsArgumentSaying(['scale', 'infinity']));
      expect(o.transform, Transform(), reason: 'left as it was');
      world.add(o);
    });
  });

  test("waiting outside a routine says how to start one (C6)", () {
    expect(() => wait(1), throwsStateSaying(['startRoutine(() async {', 'await wait(1);']));
    expect(() => nextFrame, throwsStateSaying(['startRoutine(() async {', 'await nextFrame;']));
    expect(() => waitUntil(() => true), throwsStateSaying(['startRoutine', 'await waitUntil(']));
  });

  test("an object in a message is printed as itself, not as Instance of (C7)", () {
    final root = world.add(GameObject(name: 'root'));
    final p1 = Pool(root, Shot.new), p2 = Pool(root, Shot.new);
    final shot = p1.get();
    expect(() => p2.release(shot), throwsArgumentSaying(['Shot#', "isn't from this pool"]));
    expect(() => root.removeComponent(Health()), throwsArgumentSaying(['Health', 'is not on', "'root'"]));
    expect(
      () => world.fog = const Fog(start: 0, end: 10, height: 0),
      throwsArgumentSaying(['Fog.height', 'is 0', 'above 0']),
    );
    expect(() => race<int>([]), throwsArgumentSaying(['race', 'nothing']));
    for (final text in ['Instance of', 'Instance(']) {
      expect(() => p2.release(shot), isNot(throwsArgumentSaying([text])));
      expect(() => world.fog = const Fog(start: 0, end: 10, height: 0), isNot(throwsArgumentSaying([text])));
      expect(() => race<int>([]), isNot(throwsArgumentSaying([text])));
    }
  });

  test('putting a removed component back says it was taken off (C8)', () {
    final o = world.add(GameObject(name: 'lamp'));
    final light = o.addComponent(Light.point());
    o.removeComponent(light);
    expect(() => o.addComponent(light), throwsStateSaying(['taken off', "'lamp'", 'make a new one']));
    final h = o.addComponent(Health());
    o.removeComponent(h);
    expect(() => world.add(GameObject()).addComponent(h), throwsStateSaying(['taken off', 'make a new one']));
    final on = o.addComponent(Health());
    expect(() => world.add(GameObject()).addComponent(on), throwsStateSaying(['already']));
  });

  test('before the add, a message says what the game did; ancestor and isPartOf walk what it was given to (C9)', () {
    final o = GameObject(name: 'x');
    expect(() => o.children, throwsStateSaying(['children', 'Add it first']));
    expect(() => o.find('a'), throwsStateSaying(['find', 'Add it first']));
    expect(() => o.startRoutine(() async {}), throwsStateSaying(['routine', 'Add it first']));
    final level = Level(), middle = GameObject();
    level.add(middle);
    middle.add(o);
    expect(o.ancestor<Level>(), same(level));
    expect(o.isPartOf(level), isTrue);
    expect(o.isPartOf(GameObject()), isFalse);
  });

  test("an object prints its name (C10)", () {
    expect('${GameObject(name: 'player')}', "GameObject 'player' (not added)");
    final o = world.add(GameObject(name: 'player'));
    expect('$o', "GameObject#${o.id} 'player'");
    final plain = world.add(Shot());
    expect('$plain', 'Shot#${plain.id}');
    o.destroy();
    world.update(1 / 60);
    expect('$o', "GameObject#${o.id} 'player'", reason: 'a destroyed object keeps its name in messages');
  });

  test('worldToScreenPoint with no view size says what to do (C13)', () {
    final camera = world.add(GameObject(components: [Camera()])).get<Camera>()!;
    expect(
      () => camera.worldToScreenPoint(Vec3.zero),
      throwsArgumentSaying(['width:', 'height:', 'input.simulate.resize']),
    );
  });
}
