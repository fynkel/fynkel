// What package:fynkel/extend.dart's second version adds for packages
// (docs/stage-6-architecture.md §3, item 10): asset kinds, afterUpdate,
// an order between systems, componentRemoved, pointer hits, app pause,
// writing many transforms, and turning off a system that keeps throwing.

import 'dart:async';
import 'dart:ffi';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'tapping.dart';

final log = <String>[];

/// A made-up kind of asset: the bytes as text, one per load.
final class Note {
  Note(this.text);

  final String text;
  var freed = false;
}

final class NoteKind extends AssetKind<Note> {
  const NoteKind();

  static final loaded = <Note>[];

  @override
  String get name => 'note';

  @override
  List<String> get extensions => const ['.txt'];

  @override
  Future<Note> load(World world, String name, Uint8List bytes) async {
    final note = Note(String.fromCharCodes(bytes));
    loaded.add(note);
    return note;
  }

  @override
  void free(World world, Note value) {
    expect(value.freed, isFalse, reason: 'freed once');
    value.freed = true;
    log.add('free ${value.text}');
  }
}

/// Logs its hooks, as [name].
class Logs extends WorldSystem {
  Logs(super.world, [this.name = 'logs']);

  final String name;

  @override
  void beforeUpdate() {
    super.beforeUpdate();
    log.add('$name before update');
  }

  @override
  void afterUpdate() {
    super.afterUpdate();
    log.add('$name after update');
  }

  @override
  void componentAdded(GameObject object, Component component) {
    super.componentAdded(object, component);
    log.add('$name added ${component.runtimeType} to ${object.name}');
  }

  @override
  void componentRemoved(GameObject object, Component component) {
    super.componentRemoved(object, component);
    expect(component.isRemoved, isTrue);
    log.add('$name removed ${component.runtimeType} from ${object.name}${object.isDestroyed ? ' (destroyed)' : ''}');
  }

  @override
  void applicationPause(bool paused) {
    super.applicationPause(paused);
    log.add('$name app paused $paused');
  }
}

class Early extends Logs {
  Early(World world) : super(world, 'early');

  @override
  int get order => -10;
}

class Late extends Logs {
  Late(World world) : super(world, 'late');

  @override
  int get order => 10;
}

class Updates extends GameObject {
  Updates() : super(name: 'updates');

  @override
  void onUpdate() {
    super.onUpdate();
    log.add('update');
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    log.add('late update');
  }

  @override
  void onApplicationPause(bool paused) {
    super.onApplicationPause(paused);
    log.add('$name app paused $paused');
  }
}

class Saves extends Behaviour {
  @override
  void onApplicationPause(bool paused) {
    super.onApplicationPause(paused);
    log.add('saves app paused $paused');
  }
}

class Tag extends Behaviour {}

/// Makes a sphere of radius 1 at [centre] tappable, with no mesh.
class Ball extends WorldSystem {
  Ball(super.world);

  GameObject? target;
  var centre = Vec3.zero;

  @override
  RayHit? pointerHit(Ray ray, LayerMask layers) {
    final target = this.target;
    if (target == null || !layers.contains(target.layer)) return null;
    final direction = ray.direction.normalized;
    final along = (centre - ray.origin).dot(direction);
    final closest = ray.origin + direction * along;
    final off = (centre - closest).length;
    if (along < 0 || off > 1) return null;
    final distance = along - (1 - off * off).clamp(0, 1).toDouble();
    final point = ray.origin + direction * distance;
    return RayHit(target, point, (point - centre).normalized, distance);
  }
}

class Clicked extends GameObject {
  Clicked(String name, {super.transform, super.components}) : super(name: name);

  @override
  bool onClick(Finger finger) {
    log.add('click $name');
    return true;
  }
}

/// Throws from beforeUpdate while [throws].
class Breaks extends WorldSystem {
  Breaks(super.world);

  var throws = true;
  var calls = 0;

  @override
  void beforeUpdate() {
    super.beforeUpdate();
    calls++;
    if (throws) throw StateError('broke $calls');
  }
}

World newWorld({AssetSource? source}) {
  final world = World(source: source);
  addTearDown(world.dispose);
  log.clear();
  return world;
}

void main() {
  group('asset kinds', () {
    late Map<String, String> files;
    late List<String> reads;
    Future<Uint8List> source(String name) async {
      reads.add(name);
      final text = files[name] ?? (throw StateError('no file $name'));
      return Uint8List.fromList(text.codeUnits);
    }

    setUp(() {
      files = {'hello.txt': 'hello', 'bye.txt': 'bye'};
      reads = [];
      NoteKind.loaded.clear();
    });

    test('a package\'s kind loads once, is shared, and is freed when the last holder is destroyed', () async {
      final world = newWorld(source: source);
      final a = world.add(GameObject(name: 'a'));
      final b = world.add(GameObject(name: 'b'));
      final (one, two) = await (
        a.assets.load('hello.txt', const NoteKind()),
        b.assets.load('hello.txt', const NoteKind()),
      ).wait;
      expect(one.text, 'hello');
      expect(two, same(one));
      expect(reads, ['hello.txt'], reason: 'one read');
      a.destroy();
      expect(one.freed, isFalse, reason: 'b still holds it');
      b.destroy();
      expect(one.freed, isTrue);
      expect(log, ['free hello']);

      final again = await world.assets.load('hello.txt', const NoteKind());
      expect(again, isNot(same(one)), reason: 'loaded again once freed');
      expect(reads, ['hello.txt', 'hello.txt']);
    });

    test('preload finds an added kind by its extension; the world frees what it holds when disposed', () async {
      final world = newWorld(source: source);
      await expectLater(world.assets.preload(['hello.txt']), throwsArgumentError, reason: 'not added yet');
      world.addAssetKind(const NoteKind());
      world.addAssetKind(const NoteKind());
      await world.assets.preload(['hello.txt', 'bye.txt']);
      expect(NoteKind.loaded.map((n) => n.text), ['hello', 'bye']);
      final hello = await world.assets.load('hello.txt', const NoteKind());
      expect(hello, same(NoteKind.loaded.first), reason: 'preloaded');
      world.dispose();
      expect(log, unorderedEquals(['free hello', 'free bye']));
    });

    test('a name is loaded as one kind only, and two kinds can\'t share an extension', () async {
      final world = newWorld(source: source);
      await world.assets.load('hello.txt', const NoteKind());
      expect(
        () => world.assets.texture('hello.txt'),
        throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('loaded as a note'))),
      );
      world.addAssetKind(const NoteKind());
      expect(() => world.addAssetKind(const _OtherTextKind()), throwsArgumentError);
    });

    test('what a kind\'s free throws goes to the errors, for the object that held it', () async {
      final world = newWorld(source: source);
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final a = world.add(GameObject(name: 'a'));
      await a.assets.load('bye.txt', const _OtherTextKind());
      a.destroy();
      await pumpEventQueue();
      expect(errors.single.gameObject, a);
      expect(errors.single.error, isA<StateError>());
    });
  });

  group('hooks', () {
    test('afterUpdate runs after the updates, motions and routines, before the late updates', () {
      final world = newWorld();
      world.add(Updates());
      world.system(Logs.new);
      world.update(1 / 60);
      expect(log, ['logs before update', 'update', 'logs after update', 'late update']);
    });

    test('systems run by their order, then in the order they were made', () {
      final world = newWorld();
      world.system(Logs.new);
      world.system(Late.new);
      world.system(Early.new);
      world.update(1 / 60);
      expect(log.where((l) => l.endsWith('before update')), [
        'early before update',
        'logs before update',
        'late before update',
      ]);
    });

    test('a system made inside another\'s hook doesn\'t run that one twice', () {
      final world = newWorld();
      world.system(_MakesEarly.new);
      world.update(1 / 60);
      expect(log, ['makes before update', 'early after update'], reason: 'the early one starts with the next hook');
      log.clear();
      world.update(1 / 60);
      expect(log.where((l) => l.endsWith('before update')), ['early before update', 'makes before update']);
    });

    test('componentRemoved: when a component is removed, and for each component of a destroyed object', () {
      final world = newWorld();
      world.system(Logs.new);
      final a = world.add(GameObject(name: 'a'));
      final tag = a.addComponent(Tag());
      a.addComponent(Camera());
      a.add(GameObject(name: 'child', components: [Tag()]));
      a.removeComponent(tag);
      a.destroy();
      expect(log.where((l) => l.contains('removed')), [
        'logs removed Tag from a',
        'logs removed Tag from child (destroyed)',
        'logs removed Camera from a (destroyed)',
      ]);
    });

    test('systems get their frame hooks while the world is paused', () {
      final world = newWorld();
      world.system(Logs.new);
      world.paused = true;
      world.update(1 / 60);
      expect(log, ['logs before update', 'logs after update']);
    });
  });

  group('app pause', () {
    test('tells the systems, then the shown objects and their enabled behaviours, each change once', () {
      final world = newWorld();
      world.system(Logs.new);
      world.add(Updates()).addComponent(Saves());
      final hidden = world.add(Updates()..active = false);
      hidden.name = 'hidden';
      world.add(Updates()).addComponent(Saves()).enabled = false;
      log.clear();
      world.applicationPaused = true;
      world.applicationPaused = true;
      expect(world.applicationPaused, isTrue);
      expect(log, [
        'logs app paused true',
        'updates app paused true',
        'saves app paused true',
        'updates app paused true',
      ]);
      log.clear();
      world.applicationPaused = false;
      expect(log, [
        'logs app paused false',
        'updates app paused false',
        'saves app paused false',
        'updates app paused false',
      ]);
      expect(world.paused, isFalse, reason: 'it stops nothing');
    });
  });

  group('pointer hits', () {
    test('a system makes what has no mesh tappable; the nearest hit wins', () {
      final world = newWorld();
      world.input.simulate.resize(100, 100);
      world.add(GameObject(name: 'camera', components: [Camera()]));
      final ball = world.system(Ball.new)
        ..target = world.add(Clicked('ball'))
        ..centre = Vec3(0, 0, -5);
      const middle = ViewPoint(50, 50, width: 100, height: 100);
      world.update(1 / 60);
      tap(world, middle);
      world.update(1 / 60);
      expect(log, ['click ball']);

      log.clear();
      final wall = world.add(
        Clicked(
          'wall',
          transform: Transform(position: Vec3(0, 0, -8)),
          components: [MeshRenderer(Mesh.cube(), Material())],
        ),
      );
      tap(world, middle);
      world.update(1 / 60);
      expect(log, ['click ball'], reason: 'the ball is nearer than the wall');

      log.clear();
      ball.centre = Vec3(0, 0, -20);
      tap(world, middle);
      world.update(1 / 60);
      expect(log, ['click wall']);

      log.clear();
      wall.destroy();
      ball.target!.layer = 4;
      world.input.layers = LayerMask.except([4]);
      tap(world, middle);
      world.update(1 / 60);
      expect(log, isEmpty, reason: 'its layer is not tapped');
    });
  });

  group('writeTransforms', () {
    test('sets many objects\' transforms from floats, as readWorldTransforms reads them', () {
      final world = newWorld();
      final parent = world.add(GameObject(transform: Transform(position: Vec3(10, 0, 0))));
      final a = parent.add(GameObject());
      final b = world.add(GameObject());
      final ids = malloc<Uint64>(2);
      final floats = malloc<Float>(20);
      addTearDown(
        () => malloc
          ..free(ids)
          ..free(floats),
      );
      ids[0] = a.id;
      ids[1] = b.id;
      final values = <double>[1, 2, 3, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 0, 2, 2, 2];
      for (var i = 0; i < values.length; i++) {
        floats[i] = values[i];
      }
      final version = world.transformVersion;
      world.writeTransforms(ids, 2, floats);
      expect(a.localPosition, Vec3(1, 2, 3));
      expect(a.position, Vec3(11, 2, 3), reason: 'relative to its parent');
      expect(b.rotation, Quat(0, 1, 0, 0));
      expect(b.localScale, Vec3(2, 2, 2));
      expect(world.transformVersion, greaterThan(version));

      final out = malloc<Float>(20);
      addTearDown(() => malloc.free(out));
      world.readWorldTransforms(ids, 2, out);
      expect([out[0], out[1], out[2]], [11, 2, 3], reason: 'sent with the other moves');

      floats[3] = 2; // not a unit rotation
      expect(() => world.writeTransforms(ids, 2, floats), throwsArgumentError);
      b.destroy();
      floats[3] = 0;
      expect(
        () => world.writeTransforms(ids, 2, floats),
        throwsA(isA<ArgumentError>().having((e) => '$e', 'text', contains('not a live object'))),
      );
    });
  });

  group('a system that keeps throwing', () {
    test('is turned off after a few frames in a row, and that is reported once', () async {
      final world = newWorld();
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final breaks = world.system(Breaks.new);
      for (var i = 0; i < 60; i++) {
        world.update(1 / 60);
      }
      await pumpEventQueue();
      expect(breaks.calls, WorldSystem.turnOffAfter);
      expect(breaks.isTurnedOff, isTrue);
      expect(errors, hasLength(WorldSystem.turnOffAfter), reason: "the last in the turned-off one");
      expect('${errors.last.error}', contains('Breaks threw in ${WorldSystem.turnOffAfter} frames in a row'));
    });

    test('a frame without an error starts the count again', () async {
      final world = newWorld();
      final errors = <ObjectError>[];
      world.errors.listen(errors.add);
      final breaks = world.system(Breaks.new);
      for (var i = 0; i < 20; i++) {
        breaks.throws = i % WorldSystem.turnOffAfter != 0;
        world.update(1 / 60);
      }
      await pumpEventQueue();
      expect(breaks.isTurnedOff, isFalse);
      expect(breaks.calls, 20);
    });
  });
}

/// Another text kind, whose free throws.
final class _OtherTextKind extends AssetKind<Note> {
  const _OtherTextKind();

  @override
  String get name => 'other note';

  @override
  List<String> get extensions => const ['.txt'];

  @override
  Future<Note> load(World world, String name, Uint8List bytes) async => Note(String.fromCharCodes(bytes));

  @override
  void free(World world, Note value) => throw StateError('can\'t free');
}

/// Makes [Early] from its beforeUpdate.
class _MakesEarly extends WorldSystem {
  _MakesEarly(super.world);

  @override
  void beforeUpdate() {
    super.beforeUpdate();
    world.system(Early.new);
    log.add('makes before update');
  }
}
