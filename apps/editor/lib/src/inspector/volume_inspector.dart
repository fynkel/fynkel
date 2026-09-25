// A volume profile's overrides on its Inspector page, edited in place as
// in Unity's Volume Profile Inspector: one card per override the profile
// holds (Ambient Occlusion, Bloom), Add Override for the others, and a
// checkbox in front of every field — ticked, the profile sets it; not
// ticked, it is inherited (the engine default shows greyed). Every change
// is one undo step, saved at once ([AssetEdits]); a label being dragged
// or a colour being picked shows in the Scene view before anything is
// written.
import 'package:flutter/material.dart';

import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'asset_edits.dart';
import 'field_widgets.dart';
import 'inspector_chrome.dart';

/// One field of an override: its key, label, engine default and range.
typedef _Field = ({String key, String label, Object fallback, num? min, num? max, String tooltip});

/// The overrides a profile can hold, in the order Add Override lists
/// them, with their fields (`schemas/volume.schema.json`).
const List<({String key, String title, List<_Field> fields})> volumeOverrides = [
  (
    key: 'ambientOcclusion',
    title: 'Ambient Occlusion',
    fields: [
      (
        key: 'intensity',
        label: 'Intensity',
        fallback: 0,
        min: 0,
        max: 8,
        tooltip: 'How dark the occlusion is: 0 is off, 1 is linear, larger is darker.',
      ),
      (
        key: 'radius',
        label: 'Radius',
        fallback: 0.6,
        min: 0.01,
        max: 10,
        tooltip: 'How far the occlusion reaches, in metres.',
      ),
    ],
  ),
  (
    key: 'bloom',
    title: 'Bloom',
    fields: [
      (
        key: 'intensity',
        label: 'Intensity',
        fallback: 0,
        min: 0,
        max: 10,
        tooltip: 'How much of the glow is added back: 0 is off.',
      ),
      (
        key: 'threshold',
        label: 'Threshold',
        fallback: 0.9,
        min: 0,
        max: 20,
        tooltip: 'The brightness after exposure (1 = white) above which a pixel glows.',
      ),
      (
        key: 'scatter',
        label: 'Scatter',
        fallback: 0.7,
        min: 0,
        max: 1,
        tooltip: 'How far the glow spreads: 0 keeps it tight, 1 carries it wide.',
      ),
      (
        key: 'tint',
        label: 'Tint',
        fallback: [1, 1, 1],
        min: null,
        max: null,
        tooltip: 'A colour multiplied into the glow.',
      ),
    ],
  ),
];

class VolumeInspector extends StatefulWidget {
  const VolumeInspector({super.key, required this.edits, required this.id});

  final AssetEdits edits;
  final String id;

  @override
  State<VolumeInspector> createState() => _VolumeInspectorState();
}

class _VolumeInspectorState extends State<VolumeInspector> {
  AssetEdits get edits => widget.edits;
  String get id => widget.id;

  @override
  void initState() {
    super.initState();
    edits.addListener(_changed);
    edits.shown = id;
  }

  @override
  void didUpdateWidget(covariant VolumeInspector old) {
    super.didUpdateWidget(old);
    if (old.edits != edits) {
      old.edits.removeListener(_changed);
      if (old.edits.shown == old.id) old.edits.shown = null;
      edits.addListener(_changed);
    }
    edits.shown = id;
  }

  @override
  void dispose() {
    edits.removeListener(_changed);
    if (edits.shown == id) edits.shown = null;
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final f = edits.fields(id);
    if (f == null) {
      return InspectorSection(
        title: 'Volume Profile',
        children: [InspectorValueRow('Error', edits.lastError ?? 'The profile could not be opened.')],
      );
    }
    final present = [
      for (final o in volumeOverrides)
        if (f[o.key] is Map) o,
    ];
    final absent = [
      for (final o in volumeOverrides)
        if (f[o.key] is! Map) o,
    ];
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (present.isEmpty)
          Padding(
            padding: EdgeInsets.fromLTRB(context.tokens.pad + 2, 12, context.tokens.pad + 2, 0),
            child: Text(
              'No overrides: a volume with this profile leaves the look as it is.',
              key: const Key('volume.none'),
              style: theme.textTheme.bodySmall?.copyWith(color: context.tokens.textSecondary),
            ),
          ),
        for (final o in present) _override(context, o, (f[o.key] as Map).cast<String, Object?>()),
        Padding(
          padding: EdgeInsets.fromLTRB(context.tokens.pad + 2, 10, context.tokens.pad + 2, 0),
          child: PopupMenuButton<String>(
            key: const Key('volume.addOverride'),
            popUpAnimationStyle: AnimationStyle.noAnimation,
            tooltip: '',
            enabled: edits.canEditAsset(id) && absent.isNotEmpty,
            onSelected: (key) => edits.edit(id, {'/$key': <String, Object?>{}}),
            itemBuilder: (_) => [
              for (final o in absent)
                PopupMenuItem(key: Key('volume.add.${o.key}'), value: o.key, height: 28, child: Text(o.title)),
            ],
            child: IgnorePointer(
              child: OutlinedButton.icon(
                onPressed: edits.canEditAsset(id) && absent.isNotEmpty ? () {} : null,
                icon: const Icon(FIcons.add),
                label: const Text('Add Override'),
              ),
            ),
          ),
        ),
        if (edits.lastError != null)
          Padding(
            padding: EdgeInsets.fromLTRB(context.tokens.pad + 2, 8, context.tokens.pad + 2, 0),
            child: Text(
              edits.lastError!,
              key: const Key('volume.error'),
              style: theme.textTheme.bodySmall?.copyWith(color: context.tokens.error),
            ),
          ),
      ],
    );
  }

  /// One override's card: All / None tick or untick every field, the menu
  /// removes the override.
  Widget _override(
    BuildContext context,
    ({String key, String title, List<_Field> fields}) o,
    Map<String, Object?> block,
  ) {
    final enabled = edits.canEditAsset(id);
    Widget small(String label, String key, VoidCallback? onPressed) => TextButton(
      key: Key(key),
      style: TextButton.styleFrom(
        visualDensity: VisualDensity.compact,
        padding: const EdgeInsets.symmetric(horizontal: 6),
        minimumSize: const Size(0, 20),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
      onPressed: onPressed,
      child: Text(label, style: const TextStyle(fontSize: 10)),
    );
    return KeyedSubtree(
      key: Key('volume.${o.key}'),
      child: InspectorSection(
        title: o.title,
        trailing: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            small(
              'All',
              'volume.${o.key}.all',
              enabled ? () => _set(o.key, {for (final x in o.fields) x.key: block[x.key] ?? x.fallback}) : null,
            ),
            small(
              'None',
              'volume.${o.key}.none',
              enabled ? () => edits.edit(id, {'/${o.key}': <String, Object?>{}}) : null,
            ),
            PopupMenuButton<String>(
              key: Key('volume.${o.key}.menu'),
              popUpAnimationStyle: AnimationStyle.noAnimation,
              tooltip: '',
              padding: EdgeInsets.zero,
              iconSize: 14,
              icon: Icon(Icons.adaptive.more, semanticLabel: '${o.title} options'),
              onSelected: (_) => edits.edit(id, {'/${o.key}': unset}),
              itemBuilder: (_) => [
                PopupMenuItem(
                  key: Key('volume.${o.key}.remove'),
                  value: 'remove',
                  enabled: enabled,
                  height: 28,
                  child: const Text('Remove'),
                ),
              ],
            ),
          ],
        ),
        children: [for (final x in o.fields) _field(context, o.key, x, block)],
      ),
    );
  }

  /// Sets [fields] of the override [block] (the others it holds stay).
  void _set(String block, Map<String, Object?> fields) =>
      edits.edit(id, {for (final e in fields.entries) '/$block/${e.key}': e.value});

  /// Draws [block] with [field] at [value] in the Scene view, nothing written.
  void _show(String block, Map<String, Object?> current, String field, Object? value) => edits.show(
    id,
    value == null
        ? null
        : {
            block: {...current, field: value},
          },
  );

  Widget _field(BuildContext context, String block, _Field x, Map<String, Object?> current) {
    final ticked = current.containsKey(x.key);
    final enabled = edits.canEditAsset(id) && ticked;
    final value = current[x.key] ?? x.fallback;
    final Widget field;
    if (value is List || x.fallback is List) {
      final rgb = value is List ? [for (final v in value) (v as num?) ?? 0] : (x.fallback as List).cast<num>();
      field = FieldRow(
        label: x.label,
        tooltip: x.tooltip,
        child: ColorField(
          value: rgb,
          enabled: enabled,
          onCommit: (c) => _set(block, {x.key: c}),
          onPreview: (c) => _show(block, current, x.key, c),
        ),
      );
    } else {
      field = NumberField(
        label: x.label,
        tooltip: x.tooltip,
        value: value is num ? value : x.fallback as num,
        min: x.min,
        max: x.max,
        step: 0.01,
        slider: true,
        enabled: enabled,
        onCommit: (v) => _set(block, {x.key: v}),
        onPreview: (v) => _show(block, current, x.key, v),
      );
    }
    return KeyedSubtree(
      key: Key('volume.$block.${x.key}'),
      child: Row(
        children: [
          SizedBox(
            width: 22,
            height: 18,
            child: Tooltip(
              message: ticked ? 'Overridden: untick to inherit' : 'Inherited: tick to override',
              waitDuration: const Duration(milliseconds: 500),
              child: Checkbox(
                key: Key('volume.$block.${x.key}.override'),
                value: ticked,
                visualDensity: VisualDensity.compact,
                materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                onChanged: edits.canEditAsset(id)
                    ? (v) => v == true ? _set(block, {x.key: value}) : edits.edit(id, {'/$block/${x.key}': unset})
                    : null,
              ),
            ),
          ),
          Expanded(child: field),
        ],
      ),
    );
  }
}
