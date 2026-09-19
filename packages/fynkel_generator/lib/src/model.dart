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

  /// The field's doc comment, first paragraph, without `///`.
  final String? doc;
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
  });

  /// The stable, namespaced type id scene components use (`game.Spinner`).
  final String id;
  final int version;
  final String className;

  /// `package:<pkg>/<path>.dart` of the library declaring the class.
  final String libraryUri;
  final List<ExposedField> fields;

  /// The class's doc comment, first paragraph.
  final String? doc;
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
