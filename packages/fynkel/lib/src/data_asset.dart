// Data assets (ADR-0027): a Dart class as an asset type — Unity's
// ScriptableObject. A `@FynkelDataAsset` class extends [DataAsset] and
// exposes fields with `@Expose` as a behaviour does; `fynkel codegen` writes
// its [DataType] (registered by the generated `registerBehaviours`, with
// the behaviour types) and its schema to `schemas/data/<id>.schema.json`.
// A `<name>.data.json` asset holds one instance's field values.
//
// Part of world.dart so the world can bind an instance to its asset.

part of 'world.dart';

/// Marks a [DataAsset] subclass as a data type; `id` is the stable,
/// namespaced type id a `.data.json` names in its `type`
/// (`game.Weapon`). Metadata for the editor: [menu] is the Create menu
/// path (`'Game/Weapon'`, the class name when absent), [icon] a Material
/// icon name, [description] the class doc comment when absent.
class FynkelDataAsset {
  const FynkelDataAsset({required this.id, this.version = 1, this.menu, this.icon, this.description});

  final String id;
  final int version;
  final String? menu;
  final String? icon;
  final String? description;
}

/// The base of every `@FynkelDataAsset` class. An instance is an ordinary
/// Dart object: one built in code (`Weapon(damage: 3)`) has no asset; one
/// loaded from a `.data.json`, or written by `ProjectWriter.writeData`,
/// knows its [asset]. Every reference to one asset shares one instance
/// for the world's life, and game code may change its fields for the run —
/// such changes are never saved (ADR-0027 §3).
abstract class DataAsset {
  AssetRef? _asset;
  DataType? _dataType;

  /// The asset this instance was loaded from or written to; null for one
  /// that exists only in code.
  AssetRef? get asset => _asset;

  /// Its registered type, once the registry made it or saw it.
  DataType? get dataType => _dataType;
}

/// A registered data type: what the generator emits for every
/// `@FynkelDataAsset` class, registered in the [BehaviourRegistry] next to
/// the behaviour types.
class DataType {
  const DataType({required this.id, required this.create, this.version = 1, this.fields = const []});

  final String id;
  final int version;
  final DataAsset Function() create;
  final List<DataField> fields;

  DataField? field(String name) {
    for (final f in fields) {
      if (f.name == name) return f;
    }
    return null;
  }
}

/// One authored field of a data type: [BehaviourField]'s shape over a
/// [DataAsset].
class DataField {
  const DataField({
    required this.name,
    required this.type,
    required this.get,
    required this.set,
    this.target,
    this.dataType,
    this.defaultValue,
    this.label,
    this.tooltip,
    this.min,
    this.max,
    this.step,
  });

  final String name;

  /// The schema type name (`number`, `assetRef`, `list`, `data`…).
  final String type;
  final Object? Function(DataAsset asset) get;
  final void Function(DataAsset asset, Object? value) set;

  /// A data-typed field's instance (or list of instances) as it is.
  final Object? Function(DataAsset asset)? target;

  /// The type id a data-typed field (or list) holds; null otherwise.
  final String? dataType;
  final Object? defaultValue;
  final String? label;
  final String? tooltip;
  final num? min;
  final num? max;
  final num? step;

  /// Whether it holds data assets: its JSON is `{"asset": id}` (a list of
  /// them), and [set] takes the resolved instance(s).
  bool get holdsData => dataType != null;
}

/// The JSON a data-typed field's `get` answers: `{"asset": id}` of the
/// instance's asset, or null (none, or an instance only in code).
/// Generated code calls it.
Map<String, Object?>? dataJson(DataAsset? instance) {
  final ref = instance?._asset;
  return ref == null ? null : {'asset': ref.asset, 'sub': ?ref.sub};
}

/// Bad authored data for a data asset (unknown type or field, a value
/// that does not fit).
class DataAssetError extends Error {
  DataAssetError(this.typeId, this.message, {this.field});

  final String typeId;
  final String? field;
  final String message;

  @override
  String toString() => 'DataAssetError($typeId${field == null ? '' : '.$field'}): $message';
}
