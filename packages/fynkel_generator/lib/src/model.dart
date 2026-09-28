/// What the generator knows about a `@FynkelBehaviour` class once the
/// analyzer has been consulted — plain data, so emission is a pure function
/// (`emit.dart`) that tests can drive without resolving Dart.
library;

/// The authored field kinds a behaviour may expose, with their JSON shape
/// and schema type name (the `type` string on `BehaviourField`).
enum FieldKind {
  /// `double` ↔ JSON number (`number`).
  number('number'),

  /// `int` ↔ JSON integer (`integer`).
  integer('integer'),

  /// `bool` ↔ JSON boolean (`boolean`).
  boolean('boolean'),

  /// `String` ↔ JSON string (`string`).
  string('string'),

  /// `Vec3` ↔ `[x, y, z]` (`vec3`).
  vec3('vec3'),

  /// `Quat` ↔ `[x, y, z, w]` (`quat`).
  quat('quat'),

  /// `AssetRef` ↔ `{ "asset": uuid, "sub"?: string }` (`assetRef`), the
  /// envelope's `$defs/assetRef`; a typed asset (`MaterialAsset`, …,
  /// [assetClasses]) too, with its kind implied (ADR-0036 §3).
  assetRef('assetRef'),

  /// `List<Vec3>` ↔ `[[x, y, z], …]` (`vec3List`): a path's points.
  vec3List('vec3List'),

  /// A Dart `enum` ↔ the value's name (`enum`); the Inspector's dropdown.
  enumeration('enum'),

  /// `GameObject?` ↔ `{ "entity": id }` (`gameObject`): an object of the
  /// scene, set from the Inspector (Unity's object field).
  gameObject('gameObject'),

  /// A `@FynkelBehaviour` class, nullable ↔ `{ "entity": id, "component":
  /// id }` (`behaviour`): that script on an object of the scene.
  behaviour('behaviour'),

  /// `NativeComponent<T>?` for a built-in component `T` ↔ `{ "entity": id
  /// }` (`component`): that component on an object of the scene (ADR-0036
  /// §2); an object has at most one of each, so the object names it.
  component('component'),

  /// `List<T>` ↔ a JSON array of T's JSON (`list`), for T one of
  /// [listItems] (ADR-0027 §5); `List<Vec3>` stays [vec3List].
  list('list'),

  /// A `@FynkelDataAsset` class, nullable ↔ `{ "asset": id }` (`data`):
  /// the shared instance of a data asset (ADR-0027 §4), resolved at load.
  data('data'),

  /// A `@FynkelRecord` class ↔ a JSON object of its fields (`record`): a
  /// nested value (ADR-0027 §5), [ExposedField.record] its class.
  record('record');

  /// The kinds a [list] may hold.
  static const listItems = [number, integer, boolean, string, quat, assetRef, enumeration, data, record];

  const FieldKind(this.typeName);

  /// The name `BehaviourField.type` carries.
  final String typeName;

  /// The Dart type the kind maps from (as written in the source).
  String get dartType => switch (this) {
        FieldKind.number => 'double',
        FieldKind.integer => 'int',
        FieldKind.boolean => 'bool',
        FieldKind.string => 'String',
        FieldKind.vec3 => 'Vec3',
        FieldKind.quat => 'Quat',
        FieldKind.assetRef => 'AssetRef (or a typed asset: MaterialAsset, MeshAsset, …)',
        FieldKind.vec3List => 'List<Vec3>',
        FieldKind.enumeration => 'an enum',
        FieldKind.gameObject => 'GameObject?',
        FieldKind.behaviour => 'a @FynkelBehaviour class (nullable)',
        FieldKind.component => 'NativeComponent<T>? (T a built-in component: RigidBody, PointLight, …)',
        FieldKind.list =>
          'List<T> (T one of double, int, bool, String, Quat, AssetRef or a typed asset, an enum, a data class, '
              'a record class)',
        FieldKind.data => 'a @FynkelDataAsset class (nullable)',
        FieldKind.record => 'a @FynkelRecord class',
      };

  /// A reference to an object of the scene, resolved at load.
  bool get isReference => this == FieldKind.gameObject || this == FieldKind.behaviour || this == FieldKind.component;

  static FieldKind? fromDartType(String name) {
    for (final k in values) {
      if (k.dartType == name) return k;
    }
    return null;
  }
}

/// One `@Expose` field.
class ExposedField {
  const ExposedField({
    required this.name,
    required this.kind,
    this.nullable = false,
    this.defaultValue,
    this.hasDefault = false,
    this.label,
    this.tooltip,
    this.min,
    this.max,
    this.step,
    this.group,
    this.order,
    this.header,
    this.unit,
    this.displayKind,
    this.readOnly = false,
    this.hidden = false,
    this.showIf,
    this.drawer,
    this.gizmo,
    this.doc,
    this.typeLibrary,
    this.typeName,
    this.enumValues = const [],
    this.behaviourId,
    this.componentClass,
    this.componentName,
    this.assetClass,
    this.item,
    this.dataId,
    this.record,
  });

  /// The Dart field name; also the JSON/scene field name.
  final String name;
  final FieldKind kind;
  final bool nullable;

  /// The initializer's value in JSON form (`1.0`, `true`, `[0, 1, 0]`,
  /// `{"asset": …}`), when it is a literal the generator could read.
  final Object? defaultValue;

  /// Whether [defaultValue] is meaningful (a literal `null` initializer on a
  /// nullable field sets this too).
  final bool hasDefault;
  final String? label;
  final String? tooltip;
  final num? min;
  final num? max;
  final num? step;

  /// The ADR-0018 display hints, as written in the annotation (`kind` is
  /// `color`, `asset(<kind>)`, `entity`, `multiline`, `angle`, `slider` or
  /// `layerMask`; `showIf` a sibling boolean's name, `!`-prefixed to invert).
  final String? group;
  final int? order;
  final String? header;
  final String? unit;
  final String? displayKind;
  final bool readOnly;
  final bool hidden;
  final String? showIf;

  /// The property drawer it names (`@Expose(drawer:)`, ADR-0028); on a
  /// list, each item's.
  final String? drawer;

  /// Its `@Gizmo` (ADR-0019 §3), if any.
  final GizmoSpec? gizmo;

  /// The field's doc comment, first paragraph, without `///`.
  final String? doc;

  /// The enum's or the referenced script's class, and the library
  /// declaring it (the generated file imports it).
  final String? typeLibrary;
  final String? typeName;

  /// An enum's value names, in declaration order.
  final List<String> enumValues;

  /// The type id of the script a [FieldKind.behaviour] field references.
  final String? behaviourId;

  /// A [FieldKind.component] field's component: its Dart class
  /// (`RigidBody`, exported by `package:fynkel/fynkel.dart`) and schema
  /// name (`fynkel.RigidBody`).
  final String? componentClass;
  final String? componentName;

  /// A typed asset field's class (`MaterialAsset`, [assetClasses]); null
  /// for a plain `AssetRef`. Its `asset(<kind>)` hint is implied.
  final String? assetClass;

  /// A [FieldKind.list]'s item kind (one of [FieldKind.listItems]); an
  /// enum item's class is [typeName], its values [enumValues]. The
  /// display hints (`kind`, `min`, `max`, `step`, `unit`) describe each
  /// item.
  final FieldKind? item;

  /// The type id of the data class a [FieldKind.data] field (or a list of
  /// them) holds.
  final String? dataId;

  /// The class a [FieldKind.record] field (or a list of them) holds.
  final RecordClass? record;

  /// The `x-fynkel` hints beyond the type, in a fixed key order (what the
  /// schema and the metadata both carry).
  Map<String, Object?> get hints => item != null ? _listHints : _hints;

  /// A list's own hints: the item's go on its `items`.
  Map<String, Object?> get _listHints => {
        for (final e in _hints.entries)
          if (!const {'min', 'max', 'step', 'unit', 'kind', 'drawer'}.contains(e.key)) e.key: e.value,
        'items': item!.typeName,
      };

  /// An item's hints, for a list's `items` schema.
  Map<String, Object?> get itemHints => {
        if (min != null) 'min': min,
        if (max != null) 'max': max,
        if (step != null) 'step': step,
        if (unit != null) 'unit': unit,
        if (displayKind != null) 'kind': displayKind,
        if (dataId != null) 'data': dataId,
        if (record != null) 'record': record!.className,
        if (drawer != null) 'drawer': drawer,
      };

  Map<String, Object?> get _hints => {
        if (label != null) 'label': label,
        if (tooltip != null) 'tooltip': tooltip,
        if (min != null) 'min': min,
        if (max != null) 'max': max,
        if (step != null) 'step': step,
        if (group != null) 'group': group,
        if (order != null) 'order': order,
        if (header != null) 'header': header,
        if (unit != null) 'unit': unit,
        if (displayKind != null) 'kind': displayKind,
        if (readOnly) 'readOnly': true,
        if (hidden) 'hidden': true,
        if (showIf != null) 'showIf': showIf,
        if (drawer != null) 'drawer': drawer,
        if (gizmo != null) 'gizmo': gizmo!.json,
        if (behaviourId != null) 'behaviour': behaviourId,
        if (componentName != null) 'component': componentName,
        if (dataId != null) 'data': dataId,
        if (record != null && item == null) 'record': record!.className,
      };
}

/// A `@FynkelRecord` class (ADR-0027 §5): a nested value with `@Expose`
/// fields and no identity, stored as a JSON object of them.
class RecordClass {
  const RecordClass({required this.className, required this.libraryUri, required this.fields, this.doc});

  final String className;

  /// `package:<pkg>/<path>.dart` of the library declaring the class.
  final String libraryUri;

  /// Sorted by name, as a type's are.
  final List<ExposedField> fields;
  final String? doc;

  /// The class and every record class its fields hold, depth first.
  Iterable<RecordClass> get closure sync* {
    yield this;
    for (final f in fields) {
      if (f.record != null) yield* f.record!.closure;
    }
  }
}

/// A field's `@Gizmo`: the shape, and the options that differ from the
/// defaults (`when: selected`, `space: local`, the editor's colour).
class GizmoSpec {
  const GizmoSpec({required this.shape, this.color, this.when = 'selected', this.space = 'local', this.angle});

  static const shapes = ['radius', 'box', 'point', 'direction', 'cone', 'lineTo', 'path'];

  final String shape;

  /// `0xAARRGGBB`.
  final int? color;
  final String when;
  final String space;

  /// The cone's sibling angle field.
  final String? angle;

  Map<String, Object?> get json => {
        'shape': shape,
        if (angle != null) 'angle': angle,
        if (color != null) 'color': color,
        if (when != 'selected') 'when': when,
        if (space != 'local' && shape != 'lineTo') 'space': space,
      };
}

/// Checks a `@Gizmo` against its field and the class's other fields;
/// returns the problem, or null when it fits.
String? gizmoProblem(GizmoSpec g, ExposedField field, List<ExposedField> siblings) {
  final t = field.kind;
  final fits = switch (g.shape) {
    'radius' => t == FieldKind.number || t == FieldKind.integer,
    'box' || 'point' || 'direction' => t == FieldKind.vec3,
    'cone' => t == FieldKind.number,
    'lineTo' => t == FieldKind.string,
    'path' => t == FieldKind.vec3List,
    _ => false,
  };
  if (!fits) {
    final need = switch (g.shape) {
      'radius' => 'a double or int',
      'box' || 'point' || 'direction' => 'a Vec3',
      'cone' => 'a double (the length)',
      'lineTo' => 'a String (an entity name)',
      'path' => 'a List<Vec3>',
      _ => 'nothing',
    };
    return '@Gizmo.${g.shape} needs $need field';
  }
  if (g.shape == 'cone') {
    ExposedField? angle;
    for (final s in siblings) {
      if (s.name == g.angle && s.name != field.name) angle = s;
    }
    if (angle == null || angle.kind != FieldKind.number) {
      return "@Gizmo.cone angle '${g.angle}' must name another @Expose double field of the class (radians)";
    }
  }
  return null;
}

/// The asset kinds `@Expose(kind: 'asset(<kind>)')` accepts.
const exposeAssetKinds = [
  'texture',
  'material',
  'mesh',
  'prefab',
  'scene',
  'audio',
  'clip',
  'skin',
  'rig',
  'volume',
  'physics_material',
  'shader',
  'controller',
];

/// The typed asset classes of `package:fynkel` (ADR-0036 §3) and the
/// asset kind each implies: a `MaterialAsset?` field is an `AssetRef`
/// field with `kind: 'asset(material)'`.
const assetClasses = {
  'MeshAsset': 'mesh',
  'MaterialAsset': 'material',
  'TextureAsset': 'texture',
  'PrefabAsset': 'prefab',
  'SceneAsset': 'scene',
  'AudioClipAsset': 'audio',
  'AnimationClipAsset': 'clip',
  'SkinAsset': 'skin',
  'ShaderAsset': 'shader',
  'AnimatorControllerAsset': 'controller',
  'VolumeProfileAsset': 'volume',
  'PhysicsMaterialAsset': 'physics_material',
  'RigAsset': 'rig',
};

/// Checks an `@Expose(kind:)` value against the field's type; returns the
/// problem, or null when it fits. Pure, so the builder and its tests share it.
String? exposeKindProblem(String kind, FieldKind type) {
  final asset = RegExp(r'^asset\(([a-z_]+)\)$').firstMatch(kind);
  if (asset != null) {
    if (type != FieldKind.assetRef) return "kind '$kind' needs an AssetRef field";
    if (!exposeAssetKinds.contains(asset.group(1))) {
      return "unknown asset kind '${asset.group(1)}'; one of ${exposeAssetKinds.join(', ')}";
    }
    return null;
  }
  return switch (kind) {
    'color' => type == FieldKind.vec3 ? null : "kind 'color' needs a Vec3 field (linear RGB)",
    'entity' || 'multiline' => type == FieldKind.string ? null : "kind '$kind' needs a String field",
    'angle' => type == FieldKind.number ? null : "kind 'angle' needs a double field (radians)",
    'slider' => type == FieldKind.number || type == FieldKind.integer ? null : "kind 'slider' needs a double or int field",
    'layerMask' => type == FieldKind.integer ? null : "kind 'layerMask' needs an int field",
    _ => "unknown kind '$kind'; one of color, asset(<kind>), entity, multiline, angle, slider, layerMask",
  };
}

/// One `@FynkelBehaviour` class, or (with [isData]) one
/// `@FynkelDataAsset` class (ADR-0027).
class BehaviourClass {
  const BehaviourClass({
    required this.id,
    required this.version,
    required this.className,
    required this.libraryUri,
    required this.fields,
    this.doc,
    this.icon,
    this.category,
    this.isData = false,
    this.menu,
  });

  /// The stable, namespaced type id scene components use (`game.Spinner`).
  final String id;
  final int version;
  final String className;

  /// `package:<pkg>/<path>.dart` of the library declaring the class.
  final String libraryUri;
  final List<ExposedField> fields;

  /// The class's doc comment, first paragraph — or `@FynkelBehaviour(description:)` when given.
  final String? doc;

  /// `@FynkelBehaviour(icon:, category:)` for Add Component and the Hierarchy.
  final String? icon;
  final String? category;

  /// A data type: a [DataAsset] subclass, its schema in `schemas/data/`.
  final bool isData;

  /// `@FynkelDataAsset(menu:)`: the Create menu path (`Game/Weapon`).
  final String? menu;
}

/// A generator error: the offending declaration and why.
class BehaviourGenerationError implements Exception {
  BehaviourGenerationError(this.message, {this.library, this.member});

  final String message;
  final String? library;
  final String? member;

  @override
  String toString() {
    final where = [
      if (library != null) library,
      if (member != null) member,
    ].join(' ');
    return 'fynkel_generator: ${where.isEmpty ? '' : '$where: '}$message';
  }
}
