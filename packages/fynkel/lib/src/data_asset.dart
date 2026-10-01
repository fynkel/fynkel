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
abstract class DataAsset extends Asset {
  AssetRef? _asset;
  DataType? _dataType;
  String? _name;
  bool _destroyed = false;
  World? _owner;

  /// The asset this instance was loaded from or written to; null for one
  /// that exists only in code.
  AssetRef? get asset => _asset;

  @override
  AssetRef? get ref => _asset;

  /// The asset's name (its file's, without the extension) when loaded;
  /// the class name for one made in code. A data type may declare its own
  /// `name` field instead.
  @override
  String get name => _name ?? runtimeType.toString();

  /// True unless destroyed in code or newly loaded by an aborted edit.
  /// A successfully published loaded instance lives for the world's life.
  @override
  bool get isAlive => !_destroyed && !(_owner?.isDisposed ?? false);

  /// Drops an instance made in code; one loaded from the project refuses
  /// (Unity refuses to destroy an asset from a script).
  @override
  void destroy() {
    final edit = Zone.current[_gameplayZoneKey] as _GameplayAdapter?;
    if (edit != null) {
      edit.checkOpen();
      edit.reject('data asset destruction is not supported inside World.edit');
    }
    if (_asset != null) throw UnsupportedError('$name is a project asset; it cannot be destroyed at runtime');
    _destroyed = true;
  }

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

/// Binds [instance] to the asset `ProjectWriter.writeData` wrote it to,
/// so a document written afterwards names that asset. For
/// `package:fynkel/authoring.dart` only; `package:fynkel/fynkel.dart`
/// hides it.
void bindWrittenData(DataAsset instance, AssetRef ref, DataType type) => instance
  .._asset = ref
  .._dataType = type;

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

/// The world's data assets (ADR-0027 §3): one instance per asset id, made
/// the first time something needs it and kept for the world's life,
/// across scene loads, so every reference shares it. Game code may change
/// its fields; nothing is ever written back.
final class _DataAssets {
  _DataAssets(this.world);

  final World world;
  final Map<String, DataAsset> byId = {};

  /// The instance for [ref], made from the cooked document when it is the
  /// first use. Its data-typed fields are resolved the same way; one that
  /// names a missing asset stays null with `FYN-B0004` in [warnings].
  DataAsset load(AssetRef ref, List<LoadDiagnostic> warnings) {
    if (world.isDisposed) throw StateError('data assets require a live world');
    final edit = world._gameplay;
    if (edit == null) return _load(ref, warnings, null);
    return edit.guard(() {
      if (byId.containsKey(ref.asset) || edit._dataAssets.containsKey(ref.asset)) {
        return _load(ref, warnings, edit);
      }
      if (!edit._loadingData.add(ref.asset)) edit.reject('reentrant data factory for ${ref.asset}');
      try {
        return _load(ref, warnings, edit);
      } finally {
        edit._loadingData.remove(ref.asset);
      }
    });
  }

  DataAsset _load(AssetRef ref, List<LoadDiagnostic> warnings, _GameplayAdapter? edit) {
    // Validate aliases even when an instance is already cached.
    if (edit != null && (ref.bytes == null || (ref.sub?.isNotEmpty ?? false))) {
      throw ArgumentError.value(ref, 'ref', 'a whole data asset UUID is required');
    }
    final known = byId[ref.asset] ?? edit?._dataAssets[ref.asset];
    if (known != null) return known;
    final id = ref.bytes;
    if (id == null) throw ArgumentError.value(ref, 'ref', 'not a UUID');
    if (ref.sub?.isNotEmpty ?? false) {
      throw ArgumentError.value(ref, 'ref', 'a data asset is a whole asset, not a sub-asset');
    }
    if (world._cacheDir == null) {
      throw StateError('no project is loaded: call loadProject before reading data assets');
    }
    final (status, json) = edit == null
        ? world._native.assetJson(id)
        : (FynkelStatus.FYNKEL_STATUS_OK, edit.native.assetJson(ref.asset));
    if (status == FynkelStatus.FYNKEL_STATUS_LOAD_FAILED) {
      throw ArgumentError.value(ref, 'ref', '${json['message'] ?? 'not in the cache'}');
    }
    if (status != FynkelStatus.FYNKEL_STATUS_OK) throw FynkelBridgeError('assetJson: $status');
    final schema = json[r'$schema'];
    if (schema is! String || !schema.endsWith('/data.schema.json')) {
      throw ArgumentError.value(ref, 'ref', '${json['name'] ?? ref.asset} is not a data asset');
    }
    final typeId = json['type'] as String;
    final type = world.registry.lookupData(typeId);
    if (type == null) {
      throw StateError(
        'data type $typeId (${json['name']}) is not registered: '
        'call registerBehaviours(world.registry) from the generated behaviours.g.dart',
      );
    }
    final data = (json['data'] as Map? ?? const {}).cast<String, Object?>();
    final d = world.registry.instantiateData(typeId, fields: data, version: json['version'] as int?)
      .._owner = world
      .._asset = AssetRef(ref.asset)
      .._name = json['name'] as String?;
    // Kept before its fields resolve: a cycle shares the instance.
    (edit?._dataAssets ?? byId)[ref.asset] = d;
    for (final f in type.fields) {
      if (!f.holdsData) continue;
      final value = resolve(
        data[f.name],
        f.dataType!,
        warnings,
        where: '$typeId.${f.name}',
        document: json['name'] as String?,
        pointer: '/data/${f.name}',
      );
      if (value != null) f.set(d, value);
    }
    return d;
  }

  /// A data-typed field's JSON (`{asset}`, a list of them, or null) as the
  /// instance(s) of [typeId]; a missing or mistyped asset is null (left out
  /// of a list) with `FYN-B0004`.
  Object? resolve(
    Object? value,
    String typeId,
    List<LoadDiagnostic> warnings, {
    required String where,
    String? document,
    String? pointer,
  }) {
    final edit = world._gameplay;
    if (edit != null) return edit.resolveData(value, typeId);
    DataAsset? one(Object? v, String? at) {
      if (v is! Map || v['asset'] is! String) return null;
      final ref = AssetRef(v['asset'] as String);
      if (ref.isNone) return null;
      String problem;
      try {
        final d = load(ref, warnings);
        if (d.dataType?.id == typeId) return d;
        problem = 'names ${ref.asset}, a ${d.dataType?.id} and not a $typeId';
      } on ArgumentError catch (e) {
        problem = 'names ${ref.asset}: ${e.message}';
      } on StateError catch (e) {
        problem = 'names ${ref.asset}: ${e.message}';
      }
      warnings.add(
        LoadDiagnostic(
          code: 'FYN-B0004',
          severity: 'warning',
          message: '$where $problem; it is None',
          document: document,
          pointer: at,
          suggestion: 'pick a $typeId data asset in the Inspector',
        ),
      );
      return null;
    }

    if (value is List) {
      return [for (var i = 0; i < value.length; i++) ?one(value[i], pointer == null ? null : '$pointer/$i')];
    }
    return one(value, pointer);
  }
}
