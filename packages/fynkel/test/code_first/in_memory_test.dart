// The writer without files (ADR-0028): what the editor's extensions use to
// add objects and create data assets through the editor — the same JSON
// writeScene and writeData write.

import 'dart:io';

import 'package:fynkel/authoring.dart';
import 'package:fynkel/gameplay.dart';
import 'package:test/test.dart';

import '../support/weapons.dart';

enum _Mode { fast }

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late ProjectWriter writer;

  setUp(() => writer = ProjectWriter(fixture, registerBehaviours: registerWeapons));

  test('entitiesOf is a document\'s entities map, roots first, children under their parents', () {
    final (:entities, :roots) = writer.entitiesOf([
      const ObjectSpec(
        'Ring',
        transform: Pose(position: Vec3(0, 1, 0)),
        children: [ObjectSpec('Coin 1'), ObjectSpec('Coin 2', active: false)],
      ),
      const ObjectSpec('Light', components: [PointLight(range: 3)]),
    ]);
    expect(roots, hasLength(2));
    expect(entities, hasLength(4));
    final ring = entities[roots.first]! as Map<String, Object?>;
    expect(ring['name'], 'Ring');
    expect(ring['parent'], isNull);
    final children = ring['children']! as List;
    expect([for (final c in children) (entities[c]! as Map)['name']], ['Coin 1', 'Coin 2']);
    expect((entities[children.last]! as Map)['active'], isFalse);
    final transform = (ring['components']! as Map).values.single as Map;
    expect(transform['type'], 'fynkel.Transform');
    expect((transform['data']! as Map)['position'], [0.0, 1.0, 0.0]);
    final light = entities[roots.last]! as Map<String, Object?>;
    expect(
      [for (final c in (light['components']! as Map).values) (c as Map)['type']],
      ['fynkel.Transform', 'fynkel.PointLight'],
    );
    // Nothing was written.
    expect(File('$fixture/assets/Ring.scene.json').existsSync(), isFalse);
  });

  test('dataOf is a data document\'s fields; bindData makes the instance an asset', () {
    final sword = Weapon()..damage = 3;
    final (:type, :version, :data) = writer.dataOf(sword);
    expect((type, version), ('test.Weapon', 1));
    expect(data, {'damage': 3, 'upgrade': null});
    final great = Weapon()..upgrade = sword;
    expect(() => writer.dataOf(great), throwsArgumentError, reason: 'its upgrade was never written');
    writer.bindData(sword, const AssetRef('0b6a3f1e-0000-4000-8000-000000000001'));
    expect(sword.asset, const AssetRef('0b6a3f1e-0000-4000-8000-000000000001'));
    expect(writer.dataOf(great).data['upgrade'], {'asset': '0b6a3f1e-0000-4000-8000-000000000001'});
    expect(() => writer.dataOf(_NotRegistered()), throwsArgumentError);
  });

  test('jsonOf gives a value as a document field holds it', () {
    expect(ProjectWriter.jsonOf(const Vec3(1, 2, 3)), [1.0, 2.0, 3.0]);
    expect(ProjectWriter.jsonOf(const Quat(0, 0, 0, 1)), [0.0, 0.0, 0.0, 1.0]);
    expect(ProjectWriter.jsonOf(const AssetRef('a', sub: 'mesh/0')), {'asset': 'a', 'sub': 'mesh/0'});
    expect(ProjectWriter.jsonOf(_Mode.fast), 'fast');
    expect(ProjectWriter.jsonOf([1, 'x', null, true]), [1, 'x', null, true]);
    expect(ProjectWriter.jsonOf({'at': const Vec2(1, 2)}), {
      'at': [1.0, 2.0],
    });
    expect(ProjectWriter.jsonOf(const PointLight(range: 3)), containsPair('range', 3.0));
    expect(() => ProjectWriter.jsonOf(Weapon()), throwsArgumentError, reason: 'never written');
    expect(() => ProjectWriter.jsonOf(Object()), throwsArgumentError);
  });
}

class _NotRegistered extends DataAsset {}
