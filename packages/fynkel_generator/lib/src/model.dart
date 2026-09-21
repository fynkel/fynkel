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
  /// envelope's `$defs/assetRef`.
  assetRef('assetRef');

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
        FieldKind.assetRef => 'AssetRef',
      };

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
    this.doc,
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

  /// The field's doc comment, first paragraph, without `///`.
  final String? doc;

  /// The `x-fynkel` hints beyond the type, in a fixed key order (what the
  /// schema and the metadata both carry).
  Map<String, Object?> get hints => {
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
      };
}

/// The asset kinds `@Expose(kind: 'asset(<kind>)')` accepts.
const exposeAssetKinds = ['texture', 'material', 'mesh', 'prefab', 'scene', 'audio', 'clip', 'skin', 'rig'];

/// Checks an `@Expose(kind:)` value against the field's type; returns the
/// problem, or null when it fits. Pure, so the builder and its tests share it.
String? exposeKindProblem(String kind, FieldKind type) {
  final asset = RegExp(r'^asset\(([a-z]+)\)$').firstMatch(kind);
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

/// One `@FynkelBehaviour` class.
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
