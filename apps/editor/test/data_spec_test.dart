// Data-typed fields in the Inspector's specs (ADR-0027 §7): the schema's
// `x-fynkel { type: data, data: <type id> }` (alone or on a list's items)
// is an asset field of kind `data` naming its type, and its picker offers
// only data assets of that type.
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/inspector/field_editor.dart';
import 'package:fynkel_editor/src/inspector/field_spec.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';

const assetRef = r'https://fynkel.dev/schemas/envelope.schema.json#/$defs/assetRef';

Map<String, Object?> dataRef({bool nullable = true}) => {
  if (nullable)
    'anyOf': [
      {r'$ref': assetRef},
      {'type': 'null'},
    ]
  else
    r'$ref': assetRef,
  'x-fynkel': {'type': 'data', 'nullable': ?(nullable ? true : null), 'data': 'game.Weapon'},
};

void main() {
  final spec = parseComponentSpec('game.Wielder', {
    'type': 'object',
    'properties': {
      'weapon': dataRef(),
      'spares': {
        'type': 'array',
        'items': dataRef(nullable: false),
        'x-fynkel': {'type': 'list', 'items': 'data', 'data': 'game.Weapon'},
      },
    },
    'x-fynkel': {'kind': 'behaviour', 'id': 'game.Wielder', 'version': 1},
  });

  test('a data-typed field and a list of them', () {
    final weapon = spec.field('weapon')!;
    expect(
      (weapon.kind, weapon.assetKind, weapon.dataType, weapon.nullable),
      (FieldKind.assetRef, 'data', 'game.Weapon', true),
    );
    final spares = spec.field('spares')!;
    expect(spares.kind, FieldKind.array);
    expect(
      (spares.item!.kind, spares.item!.assetKind, spares.item!.dataType, spares.item!.nullable),
      (FieldKind.assetRef, 'data', 'game.Weapon', false),
    );
  });

  test("the picker's assets: data assets of the type only", () {
    const all = [
      AssetEntry(
        id: 's',
        path: 'sword.data.json',
        name: 'Sword',
        kind: 'document',
        document: 'data',
        dataType: 'game.Weapon',
      ),
      AssetEntry(
        id: 'h',
        path: 'shield.data.json',
        name: 'Shield',
        kind: 'document',
        document: 'data',
        dataType: 'game.Shield',
      ),
      AssetEntry(id: 'm', path: 'rust.material.json', name: 'Rust', kind: 'document', document: 'material'),
    ];
    expect(assetsForSpec(all, spec.field('weapon')!).map((a) => a.id), ['s']);
    expect(assetsForSpec(all, spec.field('spares')!.item!).map((a) => a.id), ['s']);
    expect(
      assetsForSpec(all, const FieldSpec(name: 'x', label: 'X', kind: FieldKind.assetRef, assetKind: 'data')).length,
      2,
    );
  });
}
