// A `fynkel.pbr@1` material's fields on its Inspector page, edited in
// place as in Unity's material Inspector: the surface (rendering mode,
// cutoff, double-sided), the maps with their factors, and the emission
// as an HDR colour (a colour and an intensity). Every change is one undo
// step, saved at once ([MaterialEdits]); a colour being picked or a label
// being dragged shows in the Scene view before anything is written.
import 'package:flutter/material.dart';

import '../project/editor_project.dart';
import '../project_browser/asset_preview.dart';
import '../project_browser/project_browser_model.dart';
import '../theme/tokens.dart';
import 'field_widgets.dart';
import 'inspector_chrome.dart';
import 'material_edits.dart';

/// Unity's names for the three alpha modes.
const _renderingModes = {'opaque': 'Opaque', 'mask': 'Cutout', 'blend': 'Transparent'};

class MaterialInspector extends StatefulWidget {
  const MaterialInspector({super.key, required this.edits, required this.browser, required this.id});

  final MaterialEdits edits;
  final ProjectBrowserModel browser;
  final String id;

  @override
  State<MaterialInspector> createState() => _MaterialInspectorState();
}

class _MaterialInspectorState extends State<MaterialInspector> {
  MaterialEdits get edits => widget.edits;
  String get id => widget.id;

  @override
  void initState() {
    super.initState();
    edits.addListener(_changed);
    edits.shown = id;
  }

  @override
  void didUpdateWidget(covariant MaterialInspector old) {
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

  void _set(String field, Object? value) => edits.set(id, {field: value});

  @override
  Widget build(BuildContext context) {
    final f = edits.fields(id);
    if (f == null) {
      return InspectorSection(
        title: 'Material',
        children: [InspectorValueRow('Error', edits.lastError ?? 'The material could not be opened.')],
      );
    }
    final enabled = edits.canEdit;
    num number(String k, num fallback) => f[k] is num ? f[k] as num : fallback;
    List<num> colour(String k, List<num> fallback) =>
        f[k] is List ? [for (final v in f[k] as List) (v as num?) ?? 0] : fallback;
    final mode = f['alpha_mode'] is String ? f['alpha_mode'] as String : 'opaque';

    Widget slider(String field, String label, num fallback, {num min = 0, num? max = 1, bool slider = true}) =>
        KeyedSubtree(
          key: Key('material.$field'),
          child: NumberField(
            label: label,
            value: number(field, fallback),
            min: min,
            max: max,
            step: 0.01,
            slider: slider,
            enabled: enabled,
            onCommit: (v) => _set(field, v),
            onPreview: (v) => edits.show(id, v == null ? null : {field: v}),
          ),
        );

    Widget colourRow(String field, String label, List<num> fallback, int components) => KeyedSubtree(
      key: Key('material.$field'),
      child: FieldRow(
        label: label,
        child: ColorField(
          value: colour(field, fallback),
          components: components,
          enabled: enabled,
          onCommit: (rgb) => _set(field, rgb),
          onPreview: (rgb) => edits.show(id, rgb == null ? null : {field: rgb}),
        ),
      ),
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InspectorSection(
          title: 'Surface Options',
          children: [
            KeyedSubtree(
              key: const Key('material.alpha_mode'),
              child: FieldRow(
                label: 'Rendering Mode',
                tooltip: 'Opaque draws solid; Cutout drops what is under the cutoff; Transparent blends.',
                child: EnumField(
                  value: mode,
                  values: _renderingModes.keys.toList(),
                  display: (v) => _renderingModes[v] ?? v,
                  enabled: enabled,
                  onCommit: (v) => _set('alpha_mode', v),
                ),
              ),
            ),
            if (mode == 'mask') slider('alpha_cutoff', 'Alpha Cutoff', 0.5),
            KeyedSubtree(
              key: const Key('material.double_sided'),
              child: FieldRow(
                label: 'Double-Sided',
                tooltip: 'Draws the back faces too (Unity\'s Render Face: Both).',
                child: BoolField(
                  value: f['double_sided'] == true,
                  enabled: enabled,
                  onCommit: (v) => _set('double_sided', v),
                ),
              ),
            ),
          ],
        ),
        InspectorSection(
          title: 'Surface Inputs',
          children: [
            _map('base_color_texture', 'Base Map', f, enabled),
            colourRow('base_color', 'Base Color', const [1, 1, 1, 1], 4),
            _map('metallic_roughness_texture', 'Metallic Map', f, enabled),
            slider('metallic', 'Metallic', 0),
            slider('roughness', 'Roughness', 0.5),
            _map('normal_texture', 'Normal Map', f, enabled),
            slider('normal_scale', 'Normal Scale', 1, max: null, slider: false),
            _map('occlusion_texture', 'Occlusion Map', f, enabled),
            slider('occlusion_strength', 'Occlusion', 1),
          ],
        ),
        InspectorSection(
          title: 'Emission',
          children: [
            _map('emissive_texture', 'Emission Map', f, enabled),
            colourRow('emissive', 'Emission Color', const [0, 0, 0], 3),
            slider('emissive_strength', 'Intensity', 1, max: null, slider: false),
          ],
        ),
        if (edits.lastError != null)
          Padding(
            padding: EdgeInsets.fromLTRB(context.tokens.pad + 2, 8, context.tokens.pad + 2, 0),
            child: Text(
              edits.lastError!,
              key: const Key('material.error'),
              style: Theme.of(context).textTheme.bodySmall?.copyWith(color: context.tokens.error),
            ),
          ),
      ],
    );
  }

  /// A texture slot: the Project's textures in the picker, a drop from the
  /// Project browser, None to unbind.
  Widget _map(String field, String label, Map<String, Object?> f, bool enabled) {
    final value = f[field];
    final project = widget.browser.project;
    return KeyedSubtree(
      key: Key('material.$field'),
      child: FieldRow(
        label: label,
        child: AssetField(
          value: value is Map ? value.cast<String, Object?>() : null,
          kind: 'texture',
          nullable: true,
          enabled: enabled,
          subject: label,
          describe: (ref) => describeAssetRef(project, ref),
          candidates: () => [
            for (final a in project.assets())
              if (a.offeredKinds.contains('texture')) a,
          ],
          thumbnail: (e, size) => AssetPreviewWidget(preview: widget.browser.preview(e), size: size),
          onPing: widget.browser.ping,
          onCommit: (ref) => _set(field, ref),
        ),
      ),
    );
  }
}

/// An asset reference's name as a field shows it (`name`, `name › sub`).
String describeAssetRef(EditorProject project, Map<String, Object?>? ref) {
  if (ref == null) return 'None';
  final id = ref['asset'] as String? ?? '';
  final sub = ref['sub'] as String?;
  for (final a in project.assets()) {
    if (a.id == id) return sub == null || sub.isEmpty ? a.name : '${a.name} › $sub';
  }
  return sub == null || sub.isEmpty ? id : '$id#$sub';
}
