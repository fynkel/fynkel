// Tags and layers from Dart (ADR-0020): an object's layer and tags are the
// `fynkel.Layer`/`fynkel.Tags` components, read and written by name through
// the project's lists; tag lookups share the gameplay journal.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart' hide Tags;

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)
      ..layersAndTags = const LayersAndTags(
        layers: ['default', 'ground', 'props'],
        tags: ['player', 'pickup', 'enemy'],
      );
    addTearDown(w.dispose);
    return w;
  }

  test('an object starts on default with no tags', () {
    final w = fresh();
    final go = w.spawn(name: 'plain');
    w.frame(1 / 60);
    expect(go.layer, 'default');
    expect(go.tags, isEmpty);
    expect(go.hasTag('player'), isFalse);
    expect(go.has<Layer>(), isFalse);
    expect(go.has<Tags>(), isFalse);
  });

  test('layer and tags are written by name and read back before the tick', () {
    final w = fresh();
    final go = w.spawn(name: 'coin')
      ..layer = 'props'
      ..addTag('pickup')
      ..addTag('enemy');
    expect(go.isPending, isFalse);
    expect(go.layer, 'props');
    expect(go.tags, {'pickup', 'enemy'});
    w.frame(1 / 60);
    expect(go.get<Layer>()!.layer, 2);
    expect(go.get<Tags>()!.bits, 0x6);
    go
      ..removeTag('enemy')
      ..removeTag('player') // absent: nothing staged
      ..layer = 'default';
    expect(go.tags, {'pickup'});
    w.frame(1 / 60);
    expect(go.tags, {'pickup'});
    expect(go.hasTag('pickup'), isTrue);
    expect(go.layer, 'default');
  });

  test('findWithTag and findAllWithTag read committed configuration immediately', () {
    final w = fresh();
    final a = w.spawn(name: 'A')..addTag('pickup');
    w.spawn(name: 'B');
    final c = w.spawn(name: 'C')
      ..addTag('player')
      ..addTag('pickup');
    expect(w.findWithTag('pickup'), same(a), reason: 'spawn publishes immediately');
    w.frame(1 / 60);
    expect(w.findWithTag('pickup'), same(a));
    expect(w.findAllWithTag('pickup'), [same(a), same(c)]);
    expect(w.findWithTag('player'), same(c));
    expect(w.findAllWithTag('enemy'), isEmpty);
    c.removeTag('pickup');
    w.frame(1 / 60);
    expect(w.findAllWithTag('pickup'), [same(a)]);
    final d = w.spawn(name: 'D')..addTag('enemy');
    w.frame(1 / 60);
    expect(w.findWithTag('enemy'), same(d));
  });

  test('an unknown name throws, naming the project list', () {
    final w = fresh();
    final go = w.spawn(name: 'x');
    expect(
      () => go.layer = 'water',
      throwsA(
        isA<ArgumentError>().having((e) => '${e.message}', 'message', contains('[default, ground, props]')),
      ),
    );
    expect(
      () => go.addTag('boss'),
      throwsA(
        isA<ArgumentError>().having((e) => '${e.message}', 'message', contains('[player, pickup, enemy]')),
      ),
    );
    expect(() => go.hasTag('boss'), throwsArgumentError);
    expect(() => w.findWithTag('boss'), throwsStateError);
    expect(() => w.findAllWithTag('boss'), throwsStateError);
  });

  test('changing the layer at run time moves the colliders to the new group', () {
    final w = fresh();
    final box = w.spawn(name: 'box')
      ..add(
        const Collider(
          shape: ColliderShape.box,
          size: Vec3(1, 1, 1),
          offset: Vec3.zero,
          friction: 0.5,
          restitution: 0,
          isSensor: false,
          mask: 0xffffffff,
        ),
      )
      ..layer = 'ground';
    w.frame(1 / 60);
    EntityRef? hit(String layer) =>
        w.raycast(const Vec3(0, 5, 0), const Vec3(0, -1, 0), mask: w.layerMask([layer]))?.entity;
    expect(hit('ground'), box.entity);
    expect(hit('props'), isNull);
    box.layer = 'props';
    w.frame(1 / 60);
    expect(hit('ground'), isNull);
    expect(hit('props'), box.entity);
  });
}
