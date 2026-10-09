// Loading, and when an object appears; assets (docs/stage-3.md §3.3, §3.4,
// step 4).

import 'dart:async';
import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// Every load, start and destroy, in order.
final log = <String>[];

/// Loads after [gate] opens, adding what [during] makes first.
class Loads extends GameObject {
  Loads(String name, {this.gate, this.during = const [], super.transform, super.components}) : super(name: name);

  final Future<void>? gate;
  final List<GameObject Function()> during;

  @override
  Future<void> onLoad() async {
    log.add('load $name');
    await super.onLoad();
    for (final make in during) {
      add(make());
    }
    if (gate != null) await gate;
    log.add('loaded $name');
  }

  @override
  void onStart() {
    super.onStart();
    log.add('start $name');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    log.add('destroy $name');
  }
}

/// A load that fails, at once or after its first step.
class Fails extends GameObject {
  Fails({this.at = true}) : super(name: 'fails');

  final bool at;

  @override
  FutureOr<void> onLoad() {
    super.onLoad();
    if (at) throw StateError('broken at once');
    return Future(() => throw const FormatException('broken later'));
  }

  @override
  void onDestroy() {
    super.onDestroy();
    log.add('destroy fails');
  }
}

/// Loads [file] and adds a copy of it.
class Holder extends GameObject {
  Holder(this.file, {super.transform});

  final String file;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    add(await assets.model(file));
  }
}

/// A plane with a texture from the assets, on a material it owns.
class Floor extends GameObject {
  Floor({this.owns = true});

  final bool owns;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final grid = await assets.texture('grid.png');
    final material = Material(baseColorTexture: grid);
    addComponent(MeshRenderer(Mesh.plane(), owns ? own(material) : material));
  }
}

final _assets = Directory.current.uri.resolve('../../tests/assets/');

/// Reads tests/assets, counting each name it reads; a name in [gates]
/// waits for its gate first.
class Source {
  final reads = <String>[];
  final gates = <String, Completer<void>>{};

  Future<Uint8List> call(String name) async {
    reads.add(name);
    await gates[name]?.future;
    final path = name.endsWith('.glb') ? 'gltf/$name' : name;
    return File.fromUri(_assets.resolve(path)).readAsBytes();
  }
}

/// The first mesh drawn at or below [object].
Mesh? meshOf(GameObject object) {
  if (object.get<MeshRenderer>()?.mesh case final mesh?) return mesh;
  for (final child in object.children) {
    if (meshOf(child) case final mesh?) return mesh;
  }
  return null;
}

MeshRenderer cube() => MeshRenderer(Mesh.cube(), Material());
const centre = ViewPoint(50, 50, width: 100, height: 100);

void main() {
  late Source source;
  late World world;
  late GameObject camera;
  setUp(() {
    log.clear();
    source = Source();
    world = World(source: source.call);
    camera = world.add(GameObject(components: [Camera()]));
  });
  tearDown(() => world.dispose());

  /// What the camera sees at the centre, or null.
  GameObject? hit() => world.raycast(camera.rayThrough(centre))?.object;
  Transform ahead() => Transform(position: Vec3(0, 0, -5));

  test('an object that loads nothing appears in the same add', () async {
    final box = world.add(GameObject(transform: ahead(), components: [cube()]));
    expect(hit(), same(box));
    expect(box.activeInHierarchy, isTrue);
    await box.loaded;
  });

  test('an object appears whole: once its load and those of everything added during it are done', () async {
    final parentGate = Completer<void>();
    final childGate = Completer<void>();
    late GameObject child;
    final parent = world.add(
      Loads(
        'parent',
        gate: parentGate.future,
        during: [
          () => child = Loads('child', gate: childGate.future, transform: ahead(), components: [cube()]),
        ],
      ),
    );
    var appeared = false;
    unawaited(parent.loaded.then((_) => appeared = true));
    world.update(0.1, Input(width: 100, height: 100, taps: [centre]));
    await pumpEventQueue();
    expect(log, ['load parent', 'load child'], reason: 'neither started');
    expect([parent.activeInHierarchy, child.activeInHierarchy, hit()], [false, false, null]);
    expect(parent.active, isTrue, reason: 'its own flag is the game\'s');

    parentGate.complete();
    await pumpEventQueue();
    expect([parent.activeInHierarchy, appeared], [false, false], reason: 'it waits for its child');

    childGate.complete();
    await pumpEventQueue();
    expect([parent.activeInHierarchy, child.activeInHierarchy, appeared], [true, true, true]);
    expect(hit(), same(child));
    log.clear();
    world.update(0.1);
    expect(log, ['start parent', 'start child']);
  });

  test('loads run parents first, and objects added before or during a load are waited for alike', () async {
    final gate = Completer<void>();
    final root = Loads('root')..add(Loads('queued', gate: gate.future));
    world.add(root);
    await pumpEventQueue();
    expect(log, ['load root', 'load queued', 'loaded root']);
    expect(root.activeInHierarchy, isFalse);
    gate.complete();
    await root.loaded;
    expect(root.activeInHierarchy, isTrue);
  });

  test('an object destroyed before its load runs never loads', () async {
    late GameObject second;
    final root = GameObject()
      ..add(_Destroys(() => second))
      ..add(second = Loads('second'));
    world.add(root);
    await pumpEventQueue();
    expect(log, ['destroy second'], reason: 'no load');
    await root.loaded;
  });

  test('a child added after its parent appeared loads by itself, and the parent stays', () async {
    final parent = world.add(GameObject());
    final gate = Completer<void>();
    final child = parent.add(Loads('late', gate: gate.future));
    expect([parent.activeInHierarchy, child.activeInHierarchy], [true, false]);
    gate.complete();
    await child.loaded;
    expect(child.activeInHierarchy, isTrue);
  });

  test("a failed load destroys the object and goes to world.errors; the parent's load goes on", () async {
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    final gate = Completer<void>();
    late GameObject later, atOnce;
    final parent = world.add(
      Loads('parent', gate: gate.future, during: [() => later = Fails(at: false), () => atOnce = Fails()]),
    );
    await pumpEventQueue();
    expect(atOnce.isDestroyed, isTrue);
    expect(later.isDestroyed, isTrue);
    expect(log.where((l) => l == 'destroy fails'), hasLength(2));
    expect(errors.map((e) => (e.object, e.error.toString())), [
      (atOnce, 'Bad state: broken at once'),
      (later, 'FormatException: broken later'),
    ]);
    await expectLater(later.loaded, throwsFormatException);
    gate.complete();
    await parent.loaded;
    expect(parent.children.whereType<Fails>(), isEmpty);
  });

  test('with no one listening, a failed load is an uncaught error of the zone the object was added in', () async {
    final uncaught = <Object>[];
    await runZonedGuarded(() async {
      world.add(Fails(at: false));
      await pumpEventQueue();
    }, (error, _) => uncaught.add(error));
    expect(uncaught.single, isFormatException);
  });

  test(
    "an object destroyed while it loads never appears, its parent stops waiting, and its load's end is ignored",
    () async {
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final childGate = Completer<void>();
      late Loads child;
      final parent = world.add(
        Loads(
          'parent',
          during: [
            () => child = Loads('child', gate: childGate.future, during: [GameObject.new]),
          ],
        ),
      );
      await pumpEventQueue();
      expect(parent.activeInHierarchy, isFalse);
      child.destroy();
      await parent.loaded;
      await expectLater(child.loaded, throwsStateError);

      // Its load goes on, and finds it gone.
      final adds = _AddsAfter(childGate.future);
      parent.add(adds);
      adds.destroy();
      childGate.complete();
      await pumpEventQueue();
      expect(errors, isEmpty);
      expect(adds.failed, isA<StateError>());
    },
  );

  test('an inactive object stays inactive once loaded, and its flag can change while it loads', () async {
    final gate = Completer<void>();
    final off = world.add(Loads('off', gate: gate.future)..active = false);
    final on = world.add(Loads('on', gate: gate.future));
    on.active = false;
    on.active = true;
    expect([off.active, on.active, on.activeInHierarchy], [false, true, false]);
    gate.complete();
    await Future.wait([off.loaded, on.loaded]);
    expect([off.active, off.activeInHierarchy, on.active, on.activeInHierarchy], [false, false, true, true]);
    log.clear();
    world.update(0.1);
    expect(log, ['start on']);
  });

  test('assets load once by name and are shared; asks in the same frame wait for the same load', () async {
    final a = world.add(Holder('Box.glb'));
    final b = world.add(Holder('Box.glb'));
    await Future.wait([a.loaded, b.loaded]);
    final c = world.add(Holder('Box.glb'));
    await c.loaded;
    expect(source.reads, ['Box.glb']);
    final meshes = [
      for (final h in [a, b, c]) meshOf(h),
    ];
    expect(meshes.toSet(), hasLength(1));
    expect(identical(a.children.single, b.children.single), isFalse, reason: 'each gets its own copy');
  });

  test('destroying the last object that holds an asset frees all it brought: memory comes back', () async {
    final view = HeadlessView(world, width: 16, height: 16);
    addTearDown(view.dispose);
    (int, int) drawn() {
      view.read(view.frame());
      return (view.stats.meshBytes, view.stats.textureBytes);
    }

    // The built-in plane the floor uses stays, as built-in meshes do.
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 5)),
        components: [MeshRenderer(Mesh.plane(), Material())],
      ),
    );
    final before = drawn();
    final a = world.add(Holder('BoxTextured.glb', transform: ahead()));
    final b = world.add(Holder('BoxTextured.glb', transform: ahead()));
    final floor = world.add(Floor());
    await Future.wait([a.loaded, b.loaded, floor.loaded]);
    final mesh = meshOf(a)!;
    final loaded = drawn();
    expect(loaded.$1, greaterThan(before.$1));
    expect(loaded.$2, greaterThan(before.$2));

    a.destroy();
    expect(drawn(), loaded, reason: 'b still holds the model');
    expect(mesh.isDisposed, isFalse);
    b.destroy();
    floor.destroy();
    expect(drawn(), before);
    expect(mesh.isDisposed, isTrue);

    // Asked for again, it loads again.
    await world.add(Holder('BoxTextured.glb')).loaded;
    expect(source.reads.where((r) => r == 'BoxTextured.glb'), hasLength(2));
  });

  test('each copy of a model holds it too, added or not, until it is destroyed', () async {
    final holder = world.add(Holder('Box.glb'));
    await holder.loaded;
    final mesh = meshOf(holder)!;
    final spare = await holder.assets.model('Box.glb');
    final root = world.add(await holder.assets.model('Box.glb'));
    holder.destroy();
    expect(mesh.isDisposed, isFalse, reason: 'two copies still hold it');
    root.destroy();
    expect(mesh.isDisposed, isFalse, reason: 'the copy never added still does');
    spare.destroy();
    expect(mesh.isDisposed, isTrue);
  });

  test('disposing the world frees what objects own and the assets it holds', () async {
    final floor = world.add(Floor());
    await floor.loaded;
    final texture = await world.assets.texture('grid.png');
    final material = floor.get<MeshRenderer>()!.material!;
    world.dispose();
    expect([material.isDisposed, texture.isDisposed], [true, true]);
  });

  test('an object destroyed while its asset loads never appears, and the asset is not kept', () async {
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    final gate = source.gates['Box.glb'] = Completer<void>();
    final holder = world.add(Holder('Box.glb'));
    await pumpEventQueue();
    holder.destroy();
    gate.complete();
    await expectLater(holder.loaded, throwsStateError);
    await pumpEventQueue(times: 50);
    expect(errors, isEmpty);
    expect(world.all<GameObject>().whereType<Holder>(), isEmpty);
    await world.add(Holder('Box.glb')).loaded;
    expect(source.reads, ['Box.glb', 'Box.glb'], reason: 'freed when it came, so loaded again');
  });

  test('an object destroyed while its asset loads takes no copy, so the last holder frees it', () async {
    final gate = source.gates['Box.glb'] = Completer<void>();
    final gone = world.add(Holder('Box.glb'));
    final kept = world.add(Holder('Box.glb'));
    await pumpEventQueue();
    gone.destroy();
    gate.complete();
    await kept.loaded;
    final mesh = meshOf(kept)!;
    kept.destroy();
    expect(mesh.isDisposed, isTrue);
  });

  test('preloading makes a later object appear without loading anything', () async {
    await world.add(_Preloads()).loaded;
    expect(source.reads, ['Box.glb', 'grid.png']);
    final later = world.add(Holder('Box.glb'));
    await pumpEventQueue();
    expect(later.activeInHierarchy, isTrue);
    expect(source.reads, hasLength(2));
    expect(() => world.assets.preload(['notes.txt']), throwsArgumentError);
  });

  test('own frees what the game made with the object, materials first', () {
    final texture = Texture2D.rgba(1, 1, Uint8List(4));
    final material = Material(baseColorTexture: texture);
    final mesh = Mesh.custom(positions: [Vec3.zero, Vec3(1, 0, 0), Vec3(0, 1, 0)], indices: [0, 1, 2]);
    final object = world.add(GameObject(components: [MeshRenderer(mesh, material)]));
    expect(object.own(texture), same(texture));
    object
      ..own(mesh)
      ..own(material);
    object.destroy();
    expect([texture.isDisposed, material.isDisposed, mesh.isDisposed], [true, true, true]);

    final unadded = GameObject();
    final kept = unadded.own(Material());
    unadded.destroy();
    expect(kept.isDisposed, isTrue, reason: 'an object never added frees what it owns too');

    expect(() => world.add(GameObject()).own(Mesh.cube()), throwsArgumentError);
    expect(() => world.add(GameObject()).own('text'), throwsArgumentError);
    expect(() => unadded.own(Material()), throwsStateError);
  });

  test("an asset that can't be freed is reported, not thrown, when its object is destroyed", () async {
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    final floor = world.add(Floor(owns: false));
    await floor.loaded;
    floor.destroy();
    await pumpEventQueue();
    expect(errors.single.error, isA<FynkelException>().having((e) => e.message, 'message', contains('still used')));
  });

  test('asking for assets needs a source, an added object, and one kind per name', () async {
    final bare = World();
    addTearDown(bare.dispose);
    expect(
      () => bare.assets.model('Box.glb'),
      throwsA(isA<StateError>().having((e) => e.message, 'message', contains('World(source:'))),
    );
    expect(
      () => GameObject().assets,
      throwsA(isA<StateError>().having((e) => e.message, 'message', contains('onLoad'))),
    );

    await world.assets.texture('grid.png');
    expect(() => world.assets.model('grid.png'), throwsArgumentError);
  });

  test('a load that fails is not kept, so asking again tries again', () async {
    var fail = true;
    final flaky = World(
      source: (name) async {
        if (fail) throw const FileSystemException('not there yet');
        return source(name);
      },
    );
    addTearDown(flaky.dispose);
    await expectLater(flaky.assets.model('Box.glb'), throwsA(isA<FileSystemException>()));
    fail = false;
    expect(await flaky.assets.model('Box.glb'), isA<GameObject>());
  });
}

class _Preloads extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await assets.preload(['Box.glb', 'grid.png']);
  }
}

/// Waits for [gate], then adds to itself: after it was destroyed, that
/// throws.
class _AddsAfter extends GameObject {
  _AddsAfter(this.gate);

  final Future<void> gate;
  Object? failed;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await gate;
    try {
      add(GameObject());
    } catch (e) {
      failed = e;
      rethrow;
    }
  }
}

/// Destroys another object in its load, before that one's load runs.
class _Destroys extends GameObject {
  _Destroys(this.other);

  final GameObject Function() other;

  @override
  void onLoad() {
    super.onLoad();
    other().destroy();
  }
}
