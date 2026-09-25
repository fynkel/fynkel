// One Inspector field (item 4, ADR-0018): the widget for a [FieldSpec],
// wired to a [FieldTarget] — a component on the selected objects (the
// Inspector's cards) or a data asset's fields (its asset page, ADR-0027
// §7), so neither widget nor spec needs an entity.
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../picker/picker.dart';
import '../project/editor_project.dart';
import '../project_browser/asset_preview.dart';
import '../project_browser/project_browser_model.dart' show AssetPreview;
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'field_spec.dart';
import 'field_widgets.dart';
import 'inspector_math.dart';

/// What a [FieldEditor] reads and writes. Paths are the field's `data`
/// path: a top-level name (`speed`) or a pointer (`/points/2`).
abstract class FieldTarget {
  /// Stable per edited thing, for widget keys (`field.<key>.<path>`).
  String get key;
  bool get canEdit;

  /// The value shown (the first instance's when they differ).
  Object? shown(String path);

  /// Whether the edited instances differ at [path].
  bool isMixed(String path) => false;

  /// The last refusal of the top-level [field], or null.
  String? errorFor(String field);

  /// The running game's value while Play is on, to follow; null otherwise.
  ValueListenable<Object?>? live(String path) => null;

  /// Writes [value] at [path] on every instance, one undo step; [spec]
  /// runs the precheck (a vector's single number passes null).
  bool set(String path, Object? value, {FieldSpec? spec});

  /// Each edited instance's key (one for a single object or an asset).
  List<String> get instances;
  bool get single => instances.length <= 1;
  Object? shownFor(String instance, String path) => shown(path);

  /// One field, each instance its own value, one undo step.
  bool setEach(String path, Object? Function(String instance) valueFor) => set(path, valueFor(instances.first));

  /// A drag or a colour being picked: shown without a write; [endPreview]
  /// puts it back before the commit.
  void previewEach(String path, Object? Function(String instance) valueFor) {}
  void endPreview() {}

  // Prefab overrides (a component on a derived object).
  bool isOverridden(String path) => false;
  bool canRevert(String path) => false;
  void revert(String path) {}
  String? cannotApply(String path) => 'not an override';
  void apply(String path, {int depth = 0}) {}

  // Lookups.
  /// The assets [spec]'s asset field may take: its kind, and for a
  /// data-typed field ([FieldSpec.dataType]) that type only.
  List<AssetEntry> assetsFor(FieldSpec spec);
  String describeAsset(Map<String, Object?>? ref);
  AssetPreview Function(AssetEntry entry)? get assetPreview => null;
  void Function(String assetId)? get pingAsset => null;
  List<String> entityNames() => const [];
  List<String> layerNames() => const [];
  List<({String id, String name, String path})> objectsFor(FieldSpec spec) => const [];
  Map<String, Object?>? objectRef(FieldSpec spec, String entity) => null;
  String? describeObject(Map<String, Object?> ref) => null;
}

/// The assets of [all] a [spec]'s asset field may take (every one when it
/// names no kind): [AssetEntry.offeredKinds] holds the kind, and a
/// data-typed field's asset is of its type.
List<AssetEntry> assetsForSpec(List<AssetEntry> all, FieldSpec spec) {
  final kind = spec.assetKind;
  if (kind == null) return all;
  return [
    for (final a in all)
      if (a.offeredKinds.contains(kind) && (spec.dataType == null || a.dataType == spec.dataType)) a,
  ];
}

/// Picks the widget for a spec and wires it to its [target]. [path] is the
/// `data` path (`speed`, `inner/offset`, `/points/2`).
class FieldEditor extends StatelessWidget {
  const FieldEditor({super.key, required this.target, required this.spec, required this.path, this.indent = 0});

  final FieldTarget target;
  final FieldSpec spec;
  final String path;
  final double indent;

  bool get _enabled => target.canEdit && !spec.readOnly;
  String get _pointer => path.startsWith('/') ? path : '/$path';

  /// A field name at the top level writes by name (the crate adds a missing
  /// member); a nested path writes by pointer.
  String get _writePath => path.contains('/') ? _pointer : path;

  void _commit(Object? v) => target.set(_writePath, v, spec: spec);

  static List<num>? _quaternionOf(Object? v) =>
      v is List && v.length == 4 ? [for (final x in v) x is num ? x : 0] : null;

  /// Where [EulerHints] keeps the angles typed for instance [i]'s rotation.
  String _hintKey(String i) => '$i/$path';

  /// Instance [i]'s rotation in degrees as the Inspector shows it.
  Euler _eulerOf(String i) {
    final own = _quaternionOf(target.shownFor(i, path));
    return own == null ? Euler.zero : EulerHints.of(_hintKey(i), own);
  }

  /// Per axis, whether the selected objects' angles differ (to 0.001°).
  List<bool> _mixedAxes() {
    final eulers = [
      for (final i in target.instances)
        if (_quaternionOf(target.shownFor(i, path)) != null) _eulerOf(i),
    ];
    if (eulers.length < 2) return const [false, false, false];
    return [for (var a = 0; a < 3; a++) eulers.any((e) => (e[a] - eulers.first[a]).abs() > 1e-3)];
  }

  @override
  Widget build(BuildContext context) {
    final value = target.shown(path);
    final mixed = target.isMixed(path);
    final error = target.errorFor(path.startsWith('/') ? path.substring(1).split('/').first : path.split('/').first);
    final label = spec.label + (spec.readOnly ? '  🔒' : '');
    switch (spec.kind) {
      case FieldKind.number:
      case FieldKind.integer:
        final live = target.live(path);
        return NumberField(
          value: value is num ? value : null,
          live: live == null ? null : _Number(live),
          previous: value,
          mixed: mixed,
          label: label,
          tooltip: spec.tooltip,
          error: error,
          min: spec.min,
          max: spec.max,
          step: spec.step,
          unit: spec.unit,
          angle: spec.angle,
          slider: spec.slider,
          integer: spec.kind == FieldKind.integer,
          enabled: _enabled,
          overridden: _overridden,
          onRevert: _onRevert,
          onApply: _onApply,
          onCommit: _commit,
        );
      case FieldKind.boolean:
        return _row(
          label,
          error,
          BoolField(value: value is bool ? value : null, mixed: mixed, enabled: _enabled, onCommit: _commit),
        );
      case FieldKind.string:
        return _row(
          label,
          error,
          StringField(
            value: value?.toString(),
            mixed: mixed,
            multiline: spec.multiline,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.enumeration:
        return _row(
          label,
          error,
          EnumField(
            value: value?.toString(),
            values: spec.enumValues,
            mixed: mixed,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.vec2:
      case FieldKind.vec3:
      case FieldKind.vec4:
        final list = value is List ? [for (final v in value) v is num ? v : 0] : null;
        return _row(
          label,
          error,
          unit: spec.unit,
          VecField(
            value: list,
            live: target.live(path),
            components: spec.components,
            mixed: [for (var i = 0; i < spec.components; i++) target.isMixed('$path/$i')],
            enabled: _enabled,
            unit: spec.unit,
            step: spec.step,
            min: spec.min,
            max: spec.max,
            // A Transform's axis drag moves the object in the Scene view live.
            onPreviewComponent: (i, v) {
              if (v == null) return target.endPreview();
              target.previewEach(path, (inst) {
                final own = target.shownFor(inst, path);
                final list = own is List ? [for (final x in own) x is num ? x.toDouble() : 0.0] : null;
                if (list == null || i >= list.length) return own;
                return [...list]..[i] = v;
              });
            },
            onCommitComponent: (i, v) {
              if (list == null || list.length != spec.components) {
                // No vector yet: write the whole thing.
                final full = List<Object>.filled(spec.components, 0);
                full[i] = v;
                _commit(full);
              } else {
                target.set('$_pointer/$i', v);
              }
            },
          ),
        );
      case FieldKind.quaternion:
        final q = value is List && value.length == 4 ? [for (final v in value) v is num ? v : 0] : null;
        return _row(
          label,
          error,
          EulerField(
            quaternion: q,
            hintKey: _hintKey(target.instances.first),
            live: target.live(path),
            mixed: mixed,
            enabled: _enabled,
            onCommit: (quat) => _commit(_numbersLike(quat, q)),
            // Several objects: each keeps its own angles but the edited one.
            mixedAxes: target.single ? null : _mixedAxes(),
            onPreviewAxis: (axis, degrees) {
              if (degrees == null) return target.endPreview();
              target.previewEach(path, (i) => _eulerOf(i).withComponent(axis, degrees).toQuaternion());
            },
            onCommitAxis: target.single
                ? null
                : (axis, degrees) => target.setEach(_writePath, (i) {
                    final e = _eulerOf(i).withComponent(axis, degrees);
                    final q = e.toQuaternion();
                    EulerHints.remember(_hintKey(i), q, e);
                    return _numbersLike(q, _quaternionOf(target.shownFor(i, path)));
                  }),
          ),
        );
      case FieldKind.color:
        final rgb = value is List ? [for (final v in value) v is num ? v : 0] : null;
        return _row(
          label,
          error,
          ColorField(
            value: rgb,
            components: spec.components,
            mixed: mixed,
            enabled: _enabled,
            onCommit: (c) => _commit(_numbersLike(c, rgb)),
            // The Scene view follows the picker; one transaction on OK.
            onPreview: (c) => c == null ? target.endPreview() : target.previewEach(path, (_) => _numbersLike(c, rgb)),
          ),
        );
      case FieldKind.assetRef:
        return _row(
          label,
          error,
          AssetField(
            value: value is Map ? Map<String, Object?>.from(value) : null,
            kind: spec.assetKind,
            dataType: spec.dataType,
            nullable: spec.nullable,
            mixed: mixed,
            enabled: _enabled,
            subject: label,
            describe: target.describeAsset,
            candidates: () => target.assetsFor(spec),
            thumbnail: switch (target.assetPreview) {
              final preview? => (e, size) => AssetPreviewWidget(preview: preview(e), size: size),
              null => null,
            },
            onPing: target.pingAsset,
            onCommit: _commit,
          ),
        );
      case FieldKind.entity:
        return _row(
          label,
          error,
          EntityField(
            value: value?.toString(),
            subject: label,
            names: target.entityNames,
            mixed: mixed,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.objectRef:
      case FieldKind.scriptRef:
        return _row(
          label,
          error,
          ObjectField(
            value: value is Map ? Map<String, Object?>.from(value) : null,
            typeLabel: spec.kind == FieldKind.objectRef
                ? 'GameObject'
                : humanize(spec.behaviourType?.split('.').last ?? 'Script'),
            subject: label,
            describe: target.describeObject,
            candidates: () => [
              for (final o in target.objectsFor(spec))
                PickerItem(
                  value: o.id,
                  title: o.name,
                  icon: FIcons.cropSquare,
                  detail: o.path.isEmpty ? null : o.path,
                  keywords: o.path,
                  facts: [('Name', o.name), if (o.path.isNotEmpty) ('Parent', o.path)],
                ),
            ],
            refFor: (entity) => target.objectRef(spec, entity),
            mixed: mixed,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.layerMask:
        return _row(
          label,
          error,
          LayerMaskField(
            value: value is num ? value.toInt() : null,
            layers: target.layerNames(),
            mixed: mixed,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.array:
        return _array(context, label, error, value);
      case FieldKind.object:
        return _object(context, label, value);
      case FieldKind.any:
        return _row(label, error, JsonField(value: value, mixed: mixed, enabled: _enabled, onCommit: _commit));
    }
  }

  Widget _row(String label, String? error, Widget child, {String? unit}) => FieldRow(
    label: label,
    tooltip: spec.tooltip,
    error: error,
    indent: indent,
    unit: unit,
    overridden: _overridden,
    onRevert: _onRevert,
    onApply: _onApply,
    child: child,
  );

  /// Prefab mode: the field's top-level key was written by an override on
  /// a derived instance — bold, with Revert and (when the prefab can take
  /// it) Apply.
  bool get _overridden => target.isOverridden(path);
  VoidCallback? get _onRevert => _overridden && target.canRevert(path) ? () => target.revert(_writePath) : null;
  ApplyTo? get _onApply =>
      _overridden && target.cannotApply(path) == null ? (depth) => target.apply(_writePath, depth: depth) : null;

  Widget _array(BuildContext context, String label, String? error, Object? value) {
    final theme = Theme.of(context);
    final item = spec.item;
    final list = value is List ? value : const [];
    final mixed = target.isMixed(path);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        FieldRow(
          label: label,
          tooltip: spec.tooltip,
          error: error,
          indent: indent,
          overridden: _overridden,
          onRevert: _onRevert,
          onApply: _onApply,
          child: Row(
            children: [
              Expanded(
                child: Text(
                  mixed ? '—' : '${list.length} item${list.length == 1 ? '' : 's'}',
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
                ),
              ),
              if (_enabled && !mixed && item != null)
                IconButton(
                  key: ValueKey('array.add.$path'),
                  tooltip: 'Add item',
                  icon: const Icon(FIcons.add, size: 14),
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(minWidth: 22, minHeight: 22),
                  onPressed: () => _addItem(context, item, list),
                ),
            ],
          ),
        ),
        if (!mixed && item != null)
          for (var i = 0; i < list.length; i++)
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  child: FieldEditor(
                    key: ValueKey('field.${target.key}.$path.$i'),
                    target: target,
                    spec: FieldSpec(
                      name: '$i',
                      label: '${item.label} $i',
                      kind: item.kind,
                      tooltip: item.tooltip,
                      min: item.min,
                      max: item.max,
                      step: item.step,
                      unit: item.unit,
                      enumValues: item.enumValues,
                      assetKind: item.assetKind,
                      dataType: item.dataType,
                      angle: item.angle,
                      slider: item.slider,
                      multiline: item.multiline,
                      nullable: item.nullable,
                      item: item.item,
                      fixedLength: item.fixedLength,
                      children: item.children,
                    ),
                    path: '$_pointer/$i',
                    indent: indent + 10,
                  ),
                ),
                if (_enabled) ...[
                  for (final (d, icon, tip) in const [
                    (-1, FIcons.arrowUpward, 'Move up'),
                    (1, FIcons.arrowDownward, 'Move down'),
                  ])
                    IconButton(
                      key: ValueKey('array.move$d.$path.$i'),
                      tooltip: tip,
                      icon: Icon(icon, size: 12),
                      padding: EdgeInsets.zero,
                      constraints: const BoxConstraints(minWidth: 18, minHeight: 26),
                      onPressed: i + d < 0 || i + d >= list.length
                          ? null
                          : () => _commit(
                              [...list]
                                ..[i] = list[i + d]
                                ..[i + d] = list[i],
                            ),
                    ),
                  IconButton(
                    key: ValueKey('array.remove.$path.$i'),
                    tooltip: 'Remove item',
                    icon: const Icon(FIcons.remove, size: 14),
                    padding: EdgeInsets.zero,
                    constraints: const BoxConstraints(minWidth: 22, minHeight: 26),
                    onPressed: () {
                      final next = [
                        for (var j = 0; j < list.length; j++)
                          if (j != i) list[j],
                      ];
                      _commit(next);
                    },
                  ),
                ],
              ],
            ),
      ],
    );
  }

  /// Unity's `+`: a copy of the last item; into an empty list the item's
  /// default, or a fresh value — an asset item, which cannot be None, is
  /// picked first.
  Future<void> _addItem(BuildContext context, FieldSpec item, List<Object?> list) async {
    Object? value;
    if (list.isNotEmpty) {
      value = _copy(list.last);
    } else if (item.hasDefault) {
      value = item.defaultValue;
    } else if (item.kind == FieldKind.assetRef && !item.nullable) {
      final choice = await pickAssetRef(
        context,
        kind: item.assetKind,
        subject: '${spec.label} 0',
        candidates: () => target.assetsFor(item),
      );
      if (choice?.ref == null) return;
      value = choice!.ref;
    } else {
      value = _blank(item);
    }
    target.set('$_pointer/${list.length}', value);
  }

  static Object? _copy(Object? v) => switch (v) {
    final List<Object?> l => [for (final x in l) _copy(x)],
    final Map<Object?, Object?> m => {for (final e in m.entries) e.key as String: _copy(e.value)},
    _ => v,
  };

  /// A nested object — a script's `@FynkelRecord` — as a foldout (Unity's
  /// serialized class), open until folded.
  Widget _object(BuildContext context, String label, Object? value) {
    return _Foldout(
      key: ValueKey('foldout.${target.key}.$path'),
      label: label,
      indent: indent,
      children: [
        if (value == null && spec.nullable)
          Padding(
            padding: EdgeInsets.only(left: indent + 10),
            child: Align(
              alignment: Alignment.centerLeft,
              child: TextButton(onPressed: _enabled ? () => _commit(_blank(spec)) : null, child: const Text('Set…')),
            ),
          )
        else
          for (final child in spec.children)
            if (child.visibleWith(value is Map ? Map<String, Object?>.from(value) : const {}, siblings: spec.children))
              FieldEditor(
                key: ValueKey('field.${target.key}.$path.${child.name}'),
                target: target,
                spec: child,
                path: '$_pointer/${child.name}',
                indent: indent + 10,
              ),
      ],
    );
  }

  /// A fresh value of a spec (an added array item, a nullable object set).
  static Object? _blank(FieldSpec s) => switch (s.kind) {
    FieldKind.number => 0,
    FieldKind.integer || FieldKind.layerMask => 0,
    FieldKind.boolean => false,
    FieldKind.string || FieldKind.entity => '',
    FieldKind.enumeration => s.enumValues.isEmpty ? '' : s.enumValues.first,
    FieldKind.vec2 || FieldKind.vec3 || FieldKind.vec4 => List<Object>.filled(s.components, 0),
    FieldKind.quaternion => [0, 0, 0, 1],
    FieldKind.color => List<Object>.filled(s.components, 1),
    FieldKind.assetRef || FieldKind.objectRef || FieldKind.scriptRef => null,
    FieldKind.array => <Object?>[],
    FieldKind.object => {
      for (final c in s.children)
        if (c.hasDefault) c.name: c.defaultValue,
    },
    FieldKind.any => null,
  };
}

/// A caption with a chevron that folds [children] away.
class _Foldout extends StatefulWidget {
  const _Foldout({super.key, required this.label, required this.indent, required this.children});

  final String label;
  final double indent;
  final List<Widget> children;

  @override
  State<_Foldout> createState() => _FoldoutState();
}

class _FoldoutState extends State<_Foldout> {
  bool _open = true;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: EdgeInsets.only(left: widget.indent, top: 4, bottom: 1),
          child: Align(
            alignment: Alignment.centerLeft,
            child: InkWell(
              key: const ValueKey('foldout.toggle'),
              onTap: () => setState(() => _open = !_open),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(_open ? FIcons.expandMore : FIcons.chevronRight, size: 14, color: t.textSecondary),
                  const SizedBox(width: 2),
                  Text(widget.label, style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600)),
                ],
              ),
            ),
          ),
        ),
        if (_open) ...widget.children,
      ],
    );
  }
}

/// The list to write for [values] where the document held [previous]: each
/// slot keeps its int-ness (`[0, 0, 0, 1]` stays ints where it can).
List<Object> _numbersLike(List<double> values, List<num>? previous) => [
  for (var i = 0; i < values.length; i++)
    jsonNumber(values[i], previous: previous != null && i < previous.length ? previous[i] : null),
];

/// A followed field's value as a number (see [FieldTarget.live]).
final class _Number implements ValueListenable<num?> {
  const _Number(this.source);

  final ValueListenable<Object?> source;

  @override
  num? get value => switch (source.value) {
    final num n => n,
    _ => null,
  };

  @override
  void addListener(VoidCallback listener) => source.addListener(listener);
  @override
  void removeListener(VoidCallback listener) => source.removeListener(listener);

  @override
  bool operator ==(Object other) => other is _Number && identical(other.source, source);
  @override
  int get hashCode => identityHashCode(source);
}
