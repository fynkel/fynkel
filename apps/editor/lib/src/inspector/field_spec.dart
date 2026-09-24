// What the Inspector draws for one component type: a `ComponentSpec` with
// one `FieldSpec` per property, built from the type's JSON Schema — the
// generated `schemas/components/<type>.schema.json` for a native component,
// `schemas/behaviours/<id>.schema.json` (plus the committed
// `behaviours.g.json` entry) for a behaviour — and the ADR-0018 display
// hints under `x-fynkel`. One code path: a hint means the same thing
// wherever it came from. Until the Rust schema attributes carry the hints
// for native components, `native_hints.dart` supplies them from the editor's
// own table, underneath whatever the schema already says (the schema wins).
//
// Nothing here touches a document or a widget; it is pure data the model
// and the tests share.
import 'native_hints.dart';

/// The widget family a field gets.
enum FieldKind {
  number,
  integer,
  boolean,
  string,
  enumeration,
  vec2,
  vec3,
  vec4,

  /// A unit quaternion `[x, y, z, w]`, edited as Euler degrees.
  quaternion,

  /// Linear RGB `[r, g, b]` (or RGBA), edited with a colour picker.
  color,

  /// `{ asset, sub? }` — the envelope's `$defs/assetRef`.
  assetRef,

  /// An entity's name (authored components reference entities by name).
  entity,

  /// A script's `GameObject?` field: `{ entity: id }` or null (Unity's
  /// object field — pick, or drag a Hierarchy row onto it).
  objectRef,

  /// A script's field of another script type ([FieldSpec.behaviourType]):
  /// `{ entity: id, component: id }` or null.
  scriptRef,

  /// An integer of physics-layer bits (`layer`, `mask`).
  layerMask,

  /// A variable-length array of [FieldSpec.item]s.
  array,

  /// A nested object with [FieldSpec.children].
  object,

  /// No usable type information: edited as JSON text.
  any,
}

/// One property of a component's `data`.
final class FieldSpec {
  const FieldSpec({
    required this.name,
    required this.label,
    required this.kind,
    this.tooltip,
    this.nullable = false,
    this.required = false,
    this.min,
    this.max,
    this.step,
    this.unit,
    this.group,
    this.order,
    this.header,
    this.readOnly = false,
    this.hidden = false,
    this.showIf,
    this.enumValues = const [],
    this.enumName,
    this.assetKind,
    this.angle = false,
    this.slider = false,
    this.multiline = false,
    this.hasDefault = false,
    this.defaultValue,
    this.item,
    this.fixedLength,
    this.children = const [],
    this.declarationIndex = 0,
    this.gizmo,
    this.behaviourType,
  });

  /// The key in the component's `data` (a nested field's key in its parent).
  final String name;

  /// What the row shows: the `label` hint, else the name humanised.
  final String label;
  final FieldKind kind;

  /// The `tooltip` hint, else the schema description.
  final String? tooltip;
  final bool nullable;
  final bool required;
  final num? min;
  final num? max;
  final num? step;
  final String? unit;
  final String? group;
  final int? order;
  final String? header;
  final bool readOnly;
  final bool hidden;

  /// `showIf` as written: a sibling boolean's name, `!`-prefixed to invert.
  final String? showIf;
  final List<String> enumValues;

  /// `x-fynkel.enum` (`fynkel.Projection`).
  final String? enumName;

  /// `asset(<kind>)`: `mesh`, `material`, `prefab`, `texture`, `clip`, `skin`, ...
  final String? assetKind;

  /// Stored in radians, edited in degrees.
  final bool angle;

  /// Drawn as a slider (needs [min] and [max]).
  final bool slider;
  final bool multiline;
  final bool hasDefault;
  final Object? defaultValue;

  /// For [FieldKind.array]: the element spec (name `item`).
  final FieldSpec? item;

  /// For a vector kind: 2, 3 or 4; for an array with `minItems == maxItems`.
  final int? fixedLength;

  /// For [FieldKind.object]: the nested properties, in display order.
  final List<FieldSpec> children;

  /// Position in the schema's `properties` (the tie-breaker under `order`).
  final int declarationIndex;

  /// Its declared Scene-view gizmo (`@Gizmo`, ADR-0019 §3), if any.
  final GizmoHint? gizmo;

  /// For [FieldKind.scriptRef]: the script type it takes (`game.Spinner`).
  final String? behaviourType;

  /// The sibling [showIf] names, without the `!`.
  String? get showIfField {
    final s = showIf;
    if (s == null) return null;
    return s.startsWith('!') ? s.substring(1) : s;
  }

  bool get showIfNegated => showIf?.startsWith('!') ?? false;

  /// How many numbers a vector kind holds.
  int get components => switch (kind) {
    FieldKind.vec2 => 2,
    FieldKind.vec3 => 3,
    FieldKind.vec4 || FieldKind.quaternion => 4,
    FieldKind.color => fixedLength ?? 3,
    _ => fixedLength ?? 0,
  };

  bool get isNumeric => kind == FieldKind.number || kind == FieldKind.integer;

  /// Whether the field is visible given its siblings' values ([showIf]).
  /// A sibling the data does not carry counts as its schema default
  /// ([siblings] supplies it — a `Spinner { speed }` without `spinning`
  /// still shows `speed`, because `spinning` defaults to true).
  bool visibleWith(Map<String, Object?> data, {List<FieldSpec> siblings = const []}) {
    if (hidden) return false;
    final f = showIfField;
    if (f == null) return true;
    Object? v;
    if (data.containsKey(f)) {
      v = data[f];
    } else {
      for (final s in siblings) {
        if (s.name == f) v = s.defaultValue;
      }
    }
    final on = v == true;
    return showIfNegated ? !on : on;
  }

  @override
  String toString() => 'FieldSpec($name: $kind)';
}

/// A field's `gizmo` hint: what the Scene view draws for it and drags
/// (`@Gizmo.radius()` and the rest, ADR-0019 §3; native components'
/// through `native_hints.dart`).
final class GizmoHint {
  const GizmoHint({required this.shape, this.color, this.always = false, this.world = false, this.angle});

  static const shapes = {'radius', 'box', 'point', 'direction', 'cone', 'lineTo', 'path'};

  /// `radius`, `box`, `point`, `direction`, `cone`, `lineTo` or `path`.
  final String shape;

  /// `0xAARRGGBB`; null for the editor's gizmo colour.
  final int? color;

  /// Drawn on every entity carrying the component, not only the selection.
  final bool always;

  /// In world space, not the entity's.
  final bool world;

  /// A cone's sibling angle field (radians).
  final String? angle;

  /// Something to drag (a `lineTo` is only drawn).
  bool get draggable => shape != 'lineTo';

  /// Null when [v] is not a gizmo hint the editor knows.
  static GizmoHint? parse(Object? v) {
    if (v is! Map) return null;
    final shape = v['shape'];
    if (shape is! String || !shapes.contains(shape)) return null;
    if (shape == 'cone' && v['angle'] is! String) return null;
    return GizmoHint(
      shape: shape,
      color: (v['color'] as num?)?.toInt(),
      always: v['when'] == 'always',
      world: v['space'] == 'world',
      angle: v['angle'] as String?,
    );
  }
}

/// One component type as the Inspector shows it.
final class ComponentSpec {
  const ComponentSpec({
    required this.type,
    required this.fields,
    this.description,
    this.isBehaviour = false,
    this.version,
    this.library,
    this.className,
    this.icon,
    this.category,
    this.allowed = true,
    this.authored = true,
    this.missingSchema = false,
    this.additionalProperties = false,
  });

  /// `fynkel.Camera`, `game.Player`.
  final String type;

  /// In display order (the `order` hints, then declaration order).
  final List<FieldSpec> fields;
  final String? description;
  final bool isBehaviour;
  final int? version;

  /// A behaviour's `package:<pkg>/<path>.dart` and class, from the metadata
  /// or the schema's `x-fynkel.dart`.
  final String? library;
  final String? className;
  final String? icon;
  final String? category;

  /// The project's component allow-list admits it.
  final bool allowed;

  /// Meant to be put on an entity by hand (`Skin` is not).
  final bool authored;

  /// A behaviour the project declares without a schema file: its `data` is
  /// shown as it is, every key as JSON.
  final bool missingSchema;

  /// The schema admits keys it does not list (they are shown as JSON rows).
  final bool additionalProperties;

  /// `Camera`, `Player`.
  String get title => type.contains('.') ? type.substring(type.indexOf('.') + 1) : type;

  /// `fynkel`, `game`.
  String get namespace => type.contains('.') ? type.substring(0, type.indexOf('.')) : '';

  FieldSpec? field(String name) {
    for (final f in fields) {
      if (f.name == name) return f;
    }
    return null;
  }

  /// The group names in the order they first appear; `null` is the ungrouped run.
  List<String?> get groups {
    final out = <String?>[];
    for (final f in fields) {
      if (!out.contains(f.group)) out.add(f.group);
    }
    return out;
  }

  /// The `data` a fresh component starts with: every field's schema
  /// `default` (a behaviour's class initialisers). Native components take
  /// their Rust `Default` from the session instead (`componentDefaults`).
  Map<String, Object?> schemaDefaults() => {
    for (final f in fields)
      if (f.hasDefault) f.name: _copy(f.defaultValue),
  };

  @override
  String toString() => 'ComponentSpec($type, ${fields.length} fields)';
}

Object? _copy(Object? v) => switch (v) {
  List() => [for (final e in v) _copy(e)],
  Map() => {for (final e in v.entries) e.key as String: _copy(e.value)},
  _ => v,
};

/// `cast_shadows` → `Cast Shadows`, `fovY` → `Fov Y`, `ccd` → `Ccd`.
String humanize(String name) {
  final spaced = name
      .replaceAll(RegExp(r'[_\-]+'), ' ')
      .replaceAllMapped(RegExp(r'([a-z0-9])([A-Z])'), (m) => '${m[1]} ${m[2]}')
      .trim();
  return spaced.split(' ').where((w) => w.isNotEmpty).map((w) => w[0].toUpperCase() + w.substring(1)).join(' ');
}

/// The envelope's asset reference definition, as native and behaviour schemas name it.
const assetRefDef = r'envelope.schema.json#/$defs/assetRef';

/// Builds the spec of [type] from its JSON [schema] (null for a behaviour
/// without a schema file). [metadata] is the type's `behaviours.g.json`
/// entry when there is one; [nativeHints] the editor's table for native
/// components (the schema's own `x-fynkel` wins over it).
ComponentSpec parseComponentSpec(
  String type,
  Map<String, Object?>? schema, {
  Map<String, Object?>? metadata,
  Map<String, Map<String, Map<String, Object?>>> nativeHints = nativeFieldHints,
  bool allowed = true,
  bool authored = true,
}) {
  final isBehaviour = !type.startsWith('fynkel.');
  final x = _map(schema?['x-fynkel']);
  String? library, className;
  final dart = (metadata?['library'] as String?) ?? (x['dart'] as String?);
  if (dart != null) {
    final hash = dart.indexOf('#');
    library = hash < 0 ? dart : dart.substring(0, hash);
    className = (metadata?['class'] as String?) ?? (hash < 0 ? null : dart.substring(hash + 1));
  }
  if (schema == null) {
    return ComponentSpec(
      type: type,
      fields: const [],
      description: metadata?['description'] as String?,
      isBehaviour: isBehaviour,
      version: (metadata?['version'] as num?)?.toInt(),
      library: library,
      className: className,
      icon: metadata?['icon'] as String?,
      category: metadata?['category'] as String?,
      allowed: allowed,
      authored: authored,
      missingSchema: true,
      additionalProperties: true,
    );
  }
  final properties = _map(schema['properties']);
  final required = {for (final r in schema['required'] as List? ?? const []) r as String};
  final metaFields = <String, Map<String, Object?>>{
    for (final f in metadata?['fields'] as List? ?? const [])
      if (f is Map) (f['name'] as String): _map(f),
  };
  final table = nativeHints[type] ?? const {};
  final fields = <FieldSpec>[];
  var i = 0;
  for (final entry in properties.entries) {
    final name = entry.key;
    final p = _map(entry.value);
    // Hint precedence: the schema's x-fynkel, the metadata entry, the native table.
    final hints = <String, Object?>{...?table[name], ...?metaFields[name], ..._map(p['x-fynkel'])};
    fields.add(_parseField(name, p, hints, required: required.contains(name), index: i++, defs: schema));
  }
  _sortFields(fields);
  return ComponentSpec(
    type: type,
    fields: fields,
    description: (metadata?['description'] as String?) ?? schema['description'] as String?,
    isBehaviour: isBehaviour,
    version: (metadata?['version'] as num?)?.toInt() ?? (x['version'] as num?)?.toInt(),
    library: library,
    className: className,
    icon: (metadata?['icon'] as String?) ?? x['icon'] as String?,
    category: (metadata?['category'] as String?) ?? x['category'] as String?,
    allowed: allowed,
    authored: authored,
    additionalProperties: schema['additionalProperties'] != false,
  );
}

void _sortFields(List<FieldSpec> fields) {
  // Hinted orders first (ascending), then declaration order; a stable sort.
  fields.sort((a, b) {
    final ao = a.order, bo = b.order;
    if (ao != null && bo != null && ao != bo) return ao.compareTo(bo);
    if (ao != null && bo == null) return -1;
    if (ao == null && bo != null) return 1;
    return a.declarationIndex.compareTo(b.declarationIndex);
  });
}

FieldSpec _parseField(
  String name,
  Map<String, Object?> p,
  Map<String, Object?> hints, {
  required bool required,
  required int index,
  required Map<String, Object?> defs,
}) {
  var nullable = hints['nullable'] == true;
  var kind = FieldKind.any;
  var enumValues = const <String>[];
  int? fixedLength;
  FieldSpec? item;
  var children = const <FieldSpec>[];

  // The JSON type, with `null` split off.
  Object? t = p['type'];
  if (t is List) {
    final types = [for (final v in t) v as String];
    if (types.contains('null')) nullable = true;
    final rest = types.where((v) => v != 'null').toList();
    t = rest.length == 1 ? rest.single : null;
  }
  // `oneOf` / `anyOf` with an asset reference and null: an optional AssetRef.
  final variants = (p['oneOf'] ?? p['anyOf']) as List?;
  if (variants != null) {
    var ref = false;
    for (final v in variants) {
      final m = _map(v);
      if ((m[r'$ref'] as String? ?? '').endsWith(assetRefDef)) ref = true;
      if (m['type'] == 'null') nullable = true;
    }
    if (ref) kind = FieldKind.assetRef;
  }
  if ((p[r'$ref'] as String? ?? '').endsWith(assetRefDef)) kind = FieldKind.assetRef;

  if (kind == FieldKind.any) {
    switch (t) {
      case 'number':
        kind = FieldKind.number;
      case 'integer':
        kind = FieldKind.integer;
      case 'boolean':
        kind = FieldKind.boolean;
      case 'string':
        kind = FieldKind.string;
        final e = p['enum'];
        if (e is List) {
          kind = FieldKind.enumeration;
          enumValues = [
            for (final v in e)
              if (v != null) v.toString(),
          ];
        }
      case 'array':
        final items = _map(p['items']);
        final min = (p['minItems'] as num?)?.toInt();
        final max = (p['maxItems'] as num?)?.toInt();
        if (items['type'] == 'number' && min != null && min == max && min >= 2 && min <= 4) {
          fixedLength = min;
          kind = switch (min) {
            2 => FieldKind.vec2,
            3 => FieldKind.vec3,
            _ => FieldKind.vec4,
          };
          final desc = (p['description'] as String? ?? '').toLowerCase();
          if (min == 4 && desc.contains('quaternion')) kind = FieldKind.quaternion;
        } else {
          kind = FieldKind.array;
          item = _parseField('item', items, _map(items['x-fynkel']), required: false, index: 0, defs: defs);
        }
      case 'object':
        final props = _map(p['properties']);
        if (props.isNotEmpty) {
          kind = FieldKind.object;
          final req = {for (final r in p['required'] as List? ?? const []) r as String};
          var j = 0;
          final list = <FieldSpec>[
            for (final e in props.entries)
              _parseField(
                e.key,
                _map(e.value),
                _map(_map(e.value)['x-fynkel']),
                required: req.contains(e.key),
                index: j++,
                defs: defs,
              ),
          ];
          _sortFields(list);
          children = list;
        }
    }
  }

  // The generator's own type word (`vec3`, `quat`, `assetRef`, ...).
  switch (hints['type']) {
    case 'vec3':
      kind = FieldKind.vec3;
      fixedLength = 3;
    case 'quat':
      kind = FieldKind.quaternion;
      fixedLength = 4;
    case 'assetRef':
      kind = FieldKind.assetRef;
    case 'gameObject':
      kind = FieldKind.objectRef;
      nullable = true;
    case 'behaviour':
      kind = FieldKind.scriptRef;
      nullable = true;
    case 'number' when kind == FieldKind.any:
      kind = FieldKind.number;
    case 'integer' when kind == FieldKind.any:
      kind = FieldKind.integer;
    case 'boolean' when kind == FieldKind.any:
      kind = FieldKind.boolean;
    case 'string' when kind == FieldKind.any:
      kind = FieldKind.string;
  }

  // The `kind` hint.
  String? assetKind;
  var angle = false, slider = false, multiline = false;
  final k = hints['kind'] as String?;
  if (k != null) {
    final asset = RegExp(r'^asset\(([a-zA-Z]+)\)$').firstMatch(k);
    if (asset != null) {
      kind = FieldKind.assetRef;
      assetKind = asset.group(1);
    } else {
      switch (k) {
        case 'color':
          if (kind == FieldKind.vec3 || kind == FieldKind.vec4 || kind == FieldKind.quaternion) {
            kind = FieldKind.color;
            fixedLength ??= 3;
          }
        case 'entity':
          if (kind == FieldKind.string) kind = FieldKind.entity;
        case 'multiline':
          multiline = kind == FieldKind.string;
        case 'angle':
          angle = kind == FieldKind.number;
        case 'slider':
          slider = kind == FieldKind.number || kind == FieldKind.integer;
        case 'layerMask':
          if (kind == FieldKind.integer) kind = FieldKind.layerMask;
        case 'quaternion':
          if (kind == FieldKind.vec4) kind = FieldKind.quaternion;
      }
    }
  }

  final min = (hints['min'] as num?) ?? (p['minimum'] as num?) ?? (p['exclusiveMinimum'] as num?);
  final max = (hints['max'] as num?) ?? (p['maximum'] as num?) ?? (p['exclusiveMaximum'] as num?);
  if (slider && (min == null || max == null)) slider = false;
  return FieldSpec(
    name: name,
    label: (hints['label'] as String?) ?? humanize(name),
    kind: kind,
    tooltip: (hints['tooltip'] as String?) ?? p['description'] as String?,
    nullable: nullable,
    required: required,
    min: min,
    max: max,
    step: hints['step'] as num?,
    unit: hints['unit'] as String?,
    group: hints['group'] as String?,
    order: (hints['order'] as num?)?.toInt(),
    header: hints['header'] as String?,
    readOnly: hints['readOnly'] == true,
    hidden: hints['hidden'] == true,
    showIf: hints['showIf'] as String?,
    enumValues: enumValues,
    enumName: hints['enum'] is String ? hints['enum'] as String : null,
    assetKind: assetKind,
    angle: angle,
    slider: slider,
    multiline: multiline,
    hasDefault: p.containsKey('default') || hints.containsKey('default'),
    defaultValue: p.containsKey('default') ? p['default'] : hints['default'],
    item: item,
    fixedLength: fixedLength,
    children: children,
    declarationIndex: index,
    gizmo: GizmoHint.parse(hints['gizmo']),
    behaviourType: kind == FieldKind.scriptRef ? hints['behaviour'] as String? : null,
  );
}

Map<String, Object?> _map(Object? v) => v is Map ? {for (final e in v.entries) e.key as String: e.value} : const {};
