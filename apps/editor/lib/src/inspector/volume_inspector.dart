// A volume profile's overrides on its Inspector page, edited in place as
// in Unity's Volume Profile Inspector: one card per override the profile
// holds (Ambient Occlusion, Bloom, Color Grading), Add Override for the
// others, and a
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

/// How a field is edited: a number, a colour, three numbers (R, G, B) or
/// one of [_choices].
enum _Kind { number, color, rgb, choice }

/// One field of an override: its key, label, engine default and range.
typedef _Field = ({String key, String label, _Kind kind, Object fallback, num? min, num? max, String tooltip});

/// The values of a [_Kind.choice] field, and what each reads as.
const Map<String, Map<String, String>> _choices = {
  'tonemapper': {'aces': 'ACES', 'neutral': 'Neutral', 'none': 'None'},
};

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
        kind: _Kind.number,
        fallback: 0,
        min: 0,
        max: 8,
        tooltip: 'How dark the occlusion is: 0 is off, 1 is linear, larger is darker.',
      ),
      (
        key: 'radius',
        label: 'Radius',
        kind: _Kind.number,
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
        kind: _Kind.number,
        fallback: 0,
        min: 0,
        max: 10,
        tooltip: 'How much of the glow is added back: 0 is off.',
      ),
      (
        key: 'threshold',
        label: 'Threshold',
        kind: _Kind.number,
        fallback: 0.9,
        min: 0,
        max: 20,
        tooltip: 'The brightness after exposure (1 = white) above which a pixel glows.',
      ),
      (
        key: 'scatter',
        label: 'Scatter',
        kind: _Kind.number,
        fallback: 0.7,
        min: 0,
        max: 1,
        tooltip: 'How far the glow spreads: 0 keeps it tight, 1 carries it wide.',
      ),
      (
        key: 'tint',
        label: 'Tint',
        kind: _Kind.color,
        fallback: [1, 1, 1],
        min: null,
        max: null,
        tooltip: 'A colour multiplied into the glow.',
      ),
    ],
  ),
  (
    key: 'colorGrading',
    title: 'Color Grading',
    fields: [
      (
        key: 'tonemapper',
        label: 'Tonemapper',
        kind: _Kind.choice,
        fallback: 'aces',
        min: null,
        max: null,
        tooltip:
            'How the scene\'s light fits the screen: ACES is filmic, Neutral keeps the colours, None clamps at white.',
      ),
      (
        key: 'postExposure',
        label: 'Post Exposure',
        kind: _Kind.number,
        fallback: 0,
        min: -10,
        max: 10,
        tooltip: 'Stops added to the camera\'s exposure.',
      ),
      (
        key: 'temperature',
        label: 'Temperature',
        kind: _Kind.number,
        fallback: 0,
        min: -100,
        max: 100,
        tooltip: 'White balance: negative is cooler (bluer), positive warmer (yellower).',
      ),
      (
        key: 'tint',
        label: 'Tint',
        kind: _Kind.number,
        fallback: 0,
        min: -100,
        max: 100,
        tooltip: 'White balance: negative toward green, positive toward magenta.',
      ),
      (
        key: 'contrast',
        label: 'Contrast',
        kind: _Kind.number,
        fallback: 0,
        min: -100,
        max: 100,
        tooltip: 'Spreads (positive) or gathers (negative) the tones around middle grey.',
      ),
      (
        key: 'saturation',
        label: 'Saturation',
        kind: _Kind.number,
        fallback: 0,
        min: -100,
        max: 100,
        tooltip: '-100 is grey, 100 twice as vivid.',
      ),
      (
        key: 'hueShift',
        label: 'Hue Shift',
        kind: _Kind.number,
        fallback: 0,
        min: -180,
        max: 180,
        tooltip: 'Degrees every hue is turned round the colour wheel.',
      ),
      (
        key: 'colorFilter',
        label: 'Color Filter',
        kind: _Kind.color,
        fallback: [1, 1, 1],
        min: null,
        max: null,
        tooltip: 'A colour multiplied into the whole picture.',
      ),
      (
        key: 'lift',
        label: 'Lift',
        kind: _Kind.rgb,
        fallback: [0, 0, 0],
        min: -1,
        max: 1,
        tooltip: 'Per channel: raises (or lowers) the darks; white stays.',
      ),
      (
        key: 'gamma',
        label: 'Gamma',
        kind: _Kind.rgb,
        fallback: [1, 1, 1],
        min: 0.01,
        max: 10,
        tooltip: 'Per channel: above 1 brightens the midtones, below 1 darkens them.',
      ),
      (
        key: 'gain',
        label: 'Gain',
        kind: _Kind.rgb,
        fallback: [1, 1, 1],
        min: 0,
        max: 4,
        tooltip: 'Per channel: scales the brights.',
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
    final rgb = value is List && value.length == 3
        ? [for (final v in value) v is num ? v : 0]
        : (x.fallback is List ? (x.fallback as List).cast<num>() : const <num>[]);
    if (x.kind == _Kind.choice) {
      final names = _choices[x.key]!;
      field = FieldRow(
        label: x.label,
        tooltip: x.tooltip,
        child: EnumField(
          value: value is String ? value : x.fallback as String,
          values: names.keys.toList(),
          display: (v) => names[v] ?? v,
          enabled: enabled,
          onCommit: (v) => _set(block, {x.key: v}),
        ),
      );
    } else if (x.kind == _Kind.rgb) {
      field = FieldRow(
        label: x.label,
        tooltip: x.tooltip,
        child: VecField(
          value: rgb,
          components: 3,
          names: const ['R', 'G', 'B'],
          step: 0.01,
          min: x.min,
          max: x.max,
          enabled: enabled,
          onCommitComponent: (i, v) => _set(block, {
            x.key: [for (var c = 0; c < 3; c++) c == i ? v : rgb[c]],
          }),
          onPreviewComponent: (i, v) =>
              _show(block, current, x.key, v == null ? null : [for (var c = 0; c < 3; c++) c == i ? v : rgb[c]]),
        ),
      );
    } else if (x.kind == _Kind.color) {
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
        // A −100..100 control moves in whole steps.
        step: (x.max ?? 1) - (x.min ?? 0) > 20 ? 1 : 0.01,
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
