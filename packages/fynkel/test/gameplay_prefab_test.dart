import 'dart:convert';
import 'dart:io';

import 'package:fynkel/gameplay.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

const prefabId = 'c0000000-0000-4000-8000-000000000040';
const rootId = 'c0000000-0000-4000-8000-0000000000b1';
const childId = 'd0000000-0000-4000-8000-000000000001';
const rootScript = 'd0000000-0000-4000-8000-000000000002';
const childScript = 'd0000000-0000-4000-8000-000000000003';

class _Script extends Behaviour {
  double speed = 0;
  GameObject? target;
  _Script? peer;
  int created = 0;
  @override
  void onCreate() => created++;
}

void copy(Directory from, Directory to) {
  to.createSync(recursive: true);
  for (final e in from.listSync()) {
    final name = e.uri.pathSegments.where((s) => s.isNotEmpty).last;
    if (name == '.fynkel') continue;
    if (e is Directory) {
      copy(e, Directory('${to.path}/$name'));
    } else if (e is File) {
      e.copySync('${to.path}/$name');
    }
  }
}

void main() {
  late Directory dir;
  late World world;
  late List<_Script> made;
  var failSetter = false;
  _Script? reused;
  setUpAll(() {
    dir = Directory.systemTemp.createTempSync('fynkel-gameplay-prefab-');
    final project = Directory('${dir.path}/project');
    copy(Directory('../../crates/fynkel_document/fixtures/project'), project);
    final file = File('${project.path}/assets/prefabs/crate.prefab.json');
    final j = jsonDecode(file.readAsStringSync()) as Map;
    final entities = j['entities'] as Map;
    (entities[rootId] as Map)['children'] = [childId];
    Map<String, Object?> script(double speed, {bool refs = false}) => {
      'type': 'game.Spinner',
      'version': 1,
      'data': {
        'speed': speed,
        'axis': [0, 1, 0],
        if (refs) 'target': {'entity': childId},
        if (refs) 'peer': {'entity': childId, 'component': childScript},
      },
    };
    ((entities[rootId] as Map)['components'] as Map)[rootScript] = script(7, refs: true);
    entities[childId] = {
      'name': 'Child',
      'parent': rootId,
      'children': [],
      'components': {childScript: script(8)},
    };
    file.writeAsStringSync(jsonEncode(j));
    final schema = File('${project.path}/schemas/behaviours/game.Spinner.schema.json');
    final s = jsonDecode(schema.readAsStringSync()) as Map;
    for (final key in ['target', 'peer']) {
      (s['properties'] as Map)[key] = {
        'type': ['object', 'null'],
      };
    }
    schema.writeAsStringSync(jsonEncode(s));
    cookProject(project.path, cacheDir: '${dir.path}/cache');
  });
  tearDownAll(() => dir.deleteSync(recursive: true));
  setUp(() {
    made = [];
    failSetter = false;
    reused = null;
    world = World.create();
    world.registry.register(
      BehaviourType(
        id: 'game.Spinner',
        create: () {
          if (reused != null) return reused!;
          final b = _Script();
          made.add(b);
          return b;
        },
        fields: [
          BehaviourField(
            name: 'speed',
            type: 'number',
            get: (b) => (b as _Script).speed,
            set: (b, v) {
              if (failSetter) throw StateError('setter');
              (b as _Script).speed = (v as num).toDouble();
            },
          ),
          BehaviourField(name: 'axis', type: 'vec3', get: (_) => null, set: (_, _) {}),
          BehaviourField(
            name: 'target',
            type: 'gameObject',
            get: (_) => null,
            target: (b) => (b as _Script).target,
            set: (b, v) => (b as _Script).target = v as GameObject?,
          ),
          BehaviourField(
            name: 'peer',
            type: 'behaviour',
            get: (_) => null,
            target: (b) => (b as _Script).peer,
            set: (b, v) => (b as _Script).peer = v as _Script?,
          ),
        ],
      ),
    );
    world.loadProject('${dir.path}/project', cacheDir: '${dir.path}/cache');
    world.frame(0);
    made.clear();
  });
  tearDown(() => world.dispose());

  test('cooked prefab and scripts share pending parents, references and commit', () {
    final root = world.edit(() {
      final parent = world.spawn(name: 'parent');
      final root = world.instantiate(prefabId, parent: parent, name: 'instance');
      final b = root.behaviour<_Script>()!;
      expect(root.parent, same(parent));
      expect(b.speed, 7);
      expect(b.target!.parent, same(root));
      expect(b.peer!.gameObject, same(b.target));
      expect(b.created, 0);
      expect(b.peer!.created, 0);
      return root;
    });
    expect(root.isPending, isFalse);
    expect(world.lastInstantiate!.root, root.entity);
    expect(world.lastInstantiate!.behavioursAttached, 2);
    world.frame(0);
    expect(root.behaviour<_Script>()!.created, 1);
  });
  test('abort preserves report and expires all instantiated handles without callbacks', () {
    final before = world.lastInstantiate;
    late GameObject root;
    expect(
      () => world.edit(() {
        root = world.instantiate(prefabId, name: 'discard');
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(root.isAlive, isFalse);
    expect(world.find('discard'), isNull);
    expect(world.lastInstantiate, same(before));
    expect(made.every((b) => b.created == 0), isTrue);
    for (final b in made) {
      expect(() => b.gameObject, throwsStateError);
    }
  });
  test('ObjectSpec prefab nodes work under pending code-created roots', () {
    final spec = ObjectSpec('code', children: [const ObjectSpec.prefab(AssetRef(prefabId), name: 'nested')]);
    final placeholder = spec.children.single.object;
    final root = world.edit(() => world.build(spec));
    expect(spec.children.single.object, same(placeholder));
    expect(placeholder.world, same(world));
    expect(placeholder.isAlive, isTrue);
    expect(spec.children.single.object.parent, same(root));
    expect(spec.children.single.object.behaviour<_Script>(), isNotNull);
  });
  test('prefab root placeholder stays reusable after abort and canonical after commit', () {
    final spec = ObjectSpec.prefab(const AssetRef(prefabId), name: 'placeholder');
    final handle = spec.object;
    expect(
      () => world.edit(() {
        expect(world.build(spec), same(handle));
        expect(handle.behaviour<_Script>()!.peer!.gameObject.parent, same(handle));
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(handle.isAlive, isFalse);
    expect(() => handle.world, throwsStateError);
    expect(world.find('placeholder'), isNull);
    expect(world.edit(() => world.build(spec)), same(handle));
    expect(world.attach(handle.entity), same(handle));
    expect(handle.behaviour<_Script>()!.peer!.gameObject.parent, same(handle));
  });
  test('legacy prefab root also consumes the canonical placeholder', () {
    final spec = ObjectSpec.prefab(const AssetRef(prefabId), name: 'legacy');
    final handle = spec.object;
    expect(world.build(spec), same(handle));
    expect(world.attach(handle.entity), same(handle));
    expect(handle.behaviour<_Script>()!.peer!.gameObject.parent, same(handle));
  });
  test('instances resolve authored references to their own copies', () {
    final (a, b) = world.edit(() => (world.instantiate(prefabId), world.instantiate(prefabId)));
    expect(a.behaviour<_Script>()!.target, isNot(same(b.behaviour<_Script>()!.target)));
    expect(a.behaviour<_Script>()!.peer!.gameObject.parent, same(a));
    expect(b.behaviour<_Script>()!.peer!.gameObject.parent, same(b));
  });
  test('caught field setter failure aborts prior edits and script attachments', () {
    failSetter = true;
    expect(
      () => world.edit(() {
        world.spawn(name: 'discard');
        try {
          world.instantiate(prefabId);
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(world.find('discard'), isNull);
    expect(made.every((b) => b.created == 0), isTrue);
  });
  test('factory reuse is rejected before changing existing script fields', () {
    final existing = world.edit(() => world.spawn(name: 'existing').addBehaviour(_Script()..speed = 123));
    reused = existing;
    expect(() => world.edit(() => world.instantiate(prefabId)), throwsStateError);
    expect(existing.speed, 123);
    expect(existing.gameObject.name, 'existing');
  });
  test('canceling an instance omits scripts and leaves its report root nil', () {
    world.edit(() {
      world.instantiate(prefabId, name: 'canceled').destroy();
    });
    expect(world.find('canceled'), isNull);
    expect(world.lastInstantiate!.root, EntityRef.nil);
    expect(world.lastInstantiate!.behavioursAttached, 0);
    world.frame(0);
    expect(made.every((b) => b.created == 0), isTrue);
  });
  test('final native validation rolls back instantiated objects and all attachments', () {
    final report = world.lastInstantiate;
    expect(
      () => world.edit(() {
        final root = world.instantiate(prefabId, name: 'invalid-final');
        final target = root.behaviour<_Script>()!.target!;
        root.set(Follow(target: target));
        target.destroy();
      }),
      throwsStateError,
    );
    expect(world.find('invalid-final'), isNull);
    expect(world.lastInstantiate, same(report));
    expect(made.every((b) => b.created == 0), isTrue);
    for (final b in made) {
      expect(() => b.gameObject, throwsStateError);
    }
  });

  test('data script factory failure remains sticky', () {
    world.registry.register(
      BehaviourType(
        id: 'game.Spinner',
        create: () => throw StateError('must not create'),
        fields: [
          BehaviourField(
            name: 'speed',
            type: 'data',
            dataType: 'test.Data',
            get: (_) => null,
            set: (_, _) {},
          ),
        ],
      ),
      replace: true,
    );
    expect(
      () => world.edit(() {
        world.spawn(name: 'discard-data');
        try {
          world.instantiate(prefabId);
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(world.find('discard-data'), isNull);
    expect(made, isEmpty);
  });
}
