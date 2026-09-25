// Code first, Tier 1 (docs/code-first.md, ADR-0026 §5): the object graph
// speaks GameObject — `children` and `parent` are objects, with this
// tick's writes applied — and scripts or components are found anywhere
// (`findObjectOfType`, `findObjectsWith`) or up and down the tree
// (`getInChildren`, `getInParent`), skipping inactive objects as Unity
// does. Over the real native library.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

class Enemy extends Behaviour {}

class Marker extends Behaviour {}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-code-first-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    return w;
  }

  void tick(World w) => w.frame(1 / 60);

  group('children and parent', () {
    test('are objects, readable the frame they are spawned', () {
      final w = fresh();
      final root = w.spawn(name: 'root');
      final a = w.spawn(name: 'a', parent: root);
      final b = w.spawn(name: 'b', parent: root);
      expect(root.children, [a, b], reason: 'before the tick: the spawns under it');
      expect(a.parent, root);
      expect(root.parent, isNull);

      tick(w);
      expect(root.children, [a, b], reason: 'the world agrees after the tick');
      expect(a.parent, root);
      expect(root.childEntities, [a.entity, b.entity]);
      expect(a.parentEntity, root.entity);
    });

    test('follow setParent at once: moved away is gone, moved in is last', () {
      final w = fresh();
      final root = w.spawn(name: 'root');
      final a = w.spawn(name: 'a', parent: root);
      final b = w.spawn(name: 'b', parent: root);
      final c = w.spawn(name: 'c');
      tick(w);

      b.setParent(null);
      c.setParent(root);
      expect(root.children, [a, c]);
      expect(b.parent, isNull);
      expect(c.parent, root);

      tick(w);
      expect(root.children, [a, c]);
      expect(w.find('b')!.parent, isNull);
    });

    test('a destroyed child leaves after the tick', () {
      final w = fresh();
      final root = w.spawn(name: 'root');
      final a = w.spawn(name: 'a', parent: root);
      w.spawn(name: 'b', parent: root);
      tick(w);
      a.destroy();
      tick(w);
      expect(root.children.map((c) => c.name), ['b']);
    });
  });

  group('finding', () {
    test('findObjectOfType / findObjectsOfType: scripts anywhere, inactive ones only when asked', () {
      final w = fresh();
      final e1 = w.spawn(name: 'e1').addBehaviour(Enemy());
      final e2 = w.spawn(name: 'e2').addBehaviour(Enemy());
      final off = w.spawn(name: 'off');
      final e3 = off.addBehaviour(Enemy());
      expect(w.findObjectsOfType<Enemy>(), [e1, e2, e3], reason: 'the frame they were added');
      off.setActive(false);
      tick(w);
      tick(w);

      expect(w.findObjectOfType<Enemy>(), e1);
      expect(w.findObjectsOfType<Enemy>(), [e1, e2]);
      expect(w.findObjectsOfType<Enemy>(includeInactive: true), [e1, e2, e3]);
      expect(w.findObjectOfType<Marker>(), isNull);

      e2.destroy();
      expect(w.findObjectsOfType<Enemy>(), [e1], reason: 'a destroy request counts at once');
    });

    test('native components: the value, or the object that carries it', () {
      final w = fresh();
      w.spawn(name: 'empty');
      final cam = w.spawn(name: 'Main Camera')..add(const Camera(fovY: 1.2));
      final lamp = w.spawn(name: 'lamp')..add(const PointLight(range: 7));
      final hidden = w.spawn(name: 'hidden')..add(const PointLight(range: 3));
      hidden.setActive(false);
      tick(w);
      tick(w);

      expect(w.findObjectWith<Camera>(), cam);
      expect(w.findObjectOfType<Camera>()!.fovY, closeTo(1.2, 1e-6));
      expect(w.findObjectsWith<PointLight>(), [lamp]);
      expect(w.findObjectsWith<PointLight>(includeInactive: true), [lamp, hidden]);
      expect(w.findObjectsOfType<PointLight>(includeInactive: true).map((l) => l.range), [7, 3]);
      expect(w.findObjectWith<SpotLight>(), isNull);
    });

    test('entities a scene loaded, with no handle yet, are found and handed out once', () {
      final w = fresh();
      w.loadProject(fixture, cacheDir: cache); // main.scene.json
      tick(w);
      final camera = w.findObjectWith<Camera>();
      expect(camera?.name, 'Camera');
      expect(camera, same(w.find('Camera')), reason: 'the one handle per entity');
      expect(w.findObjectsWith<Collider>().map((o) => o.name), ['Floor', 'Crate B']);
      expect(w.findObjectOfType<RigidBody>()?.bodyType, BodyType.dynamic);
    });

    test('getInChildren / getAllInChildren / getInParent: self first, then down or up; inactive skipped', () {
      final w = fresh();
      final root = w.spawn(name: 'root');
      final mid = w.spawn(name: 'mid', parent: root)..add(const PointLight(range: 4));
      final leaf = w.spawn(name: 'leaf', parent: mid);
      final m1 = leaf.addBehaviour(Marker());
      final side = w.spawn(name: 'side', parent: root);
      final m2 = side.addBehaviour(Marker());

      expect(root.getInChildren<Marker>(), m1, reason: 'depth first, sibling order, before any tick');
      expect(root.getAllInChildren<Marker>(), [m1, m2]);
      expect(leaf.getInChildren<Marker>(), m1, reason: 'the object itself counts');
      expect(leaf.getInParent<PointLight>()!.range, 4);
      expect(root.getInParent<PointLight>(), isNull);
      expect(root.getInChildren<Transform>(), identityTransform, reason: 'a native type: the value');

      tick(w);
      mid.setActive(false);
      tick(w);
      expect(root.getAllInChildren<Marker>(), [m2], reason: 'the inactive subtree is skipped');
      expect(root.getAllInChildren<Marker>(includeInactive: true), [m1, m2]);
      expect(leaf.getInParent<PointLight>(), isNull);
      expect(leaf.getInParent<PointLight>(includeInactive: true)!.range, 4);
    });

    test('a type that is neither a script nor a component says so', () {
      final w = fresh();
      expect(() => w.findObjectOfType<String>(), throwsArgumentError);
    });
  });
}
