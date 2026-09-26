// A material's fields on its Inspector page, edited in place as in
// Unity's material Inspector. The Shader dropdown at the top picks the
// built-in Lit or a `.shader` asset (ADR-0031; slashes in its name make
// submenus) and keeps the values the two share. A Lit material shows the
// surface (rendering mode, cutoff, double-sided), the maps with their
// factors, and the emission as an HDR colour (a colour and an intensity);
// a shader's material shows the shader's properties as fields — a slider
// for a ranged number, the colour picker, X Y Z W, a toggle, a texture
// field. Every change is one undo step, saved at once ([AssetEdits]); a
// colour being picked or a label being dragged shows in the Scene view
// before anything is written.
import 'package:flutter/material.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../project/editor_project.dart';
import '../project_browser/asset_preview.dart';
import '../project_browser/project_browser_model.dart';
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'field_widgets.dart';
import 'inspector_chrome.dart';
import 'asset_edits.dart';
import 'shader_switch.dart';

/// Unity's names for the three alpha modes.
const _renderingModes = {'opaque': 'Opaque', 'mask': 'Cutout', 'blend': 'Transparent'};

class MaterialInspector extends StatefulWidget {
  const MaterialInspector({super.key, required this.edits, required this.browser, required this.id});

  final AssetEdits edits;
  final ProjectBrowserModel browser;
  final String id;

  @override
  State<MaterialInspector> createState() => _MaterialInspectorState();
}

class _MaterialInspectorState extends State<MaterialInspector> {
  AssetEdits get edits => widget.edits;
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

  /// The Shader dropdown: [to]'s material, the shared values kept.
  void _switchShader(Map<String, Object?> f, ShaderInfo to) => edits.edit(id, switchShader(f, to));

  @override
  Widget build(BuildContext context) {
    final f = edits.fields(id);
    if (f == null) {
      return InspectorSection(
        title: 'Material',
        children: [InspectorValueRow('Error', edits.lastError ?? 'The material could not be opened.')],
      );
    }
    final enabled = edits.canEditAsset(id);
    num number(String k, num fallback) => f[k] is num ? f[k] as num : fallback;
    List<num> colour(String k, List<num> fallback) =>
        f[k] is List ? [for (final v in f[k] as List) (v as num?) ?? 0] : fallback;
    final mode = f['alpha_mode'] is String ? f['alpha_mode'] as String : 'opaque';
    final shaders = widget.browser.project.shaders();
    final shaderId = shaderOf(f) ?? pbrShader;
    final shader = shaders.where((s) => s.shader == shaderId).firstOrNull;
    final picker = KeyedSubtree(
      key: const Key('material.shader'),
      child: Padding(
        padding: EdgeInsets.fromLTRB(context.tokens.pad + 2, 6, context.tokens.pad + 2, 2),
        child: FieldRow(
          label: 'Shader',
          tooltip: 'What draws the material: the built-in Lit, or a .shader in the project',
          child: ShaderField(
            current: shaderId,
            shaders: shaders,
            enabled: enabled,
            onPick: (to) => to.shader == shaderId ? null : _switchShader(f, to),
          ),
        ),
      ),
    );
    if (shaderId != pbrShader) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [picker, ..._custom(context, f, shader, shaderId, enabled), _error(context)],
      );
    }

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
        picker,
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
        _error(context),
      ],
    );
  }

  Widget _error(BuildContext context) => edits.lastError == null
      ? const SizedBox.shrink()
      : Padding(
          padding: EdgeInsets.fromLTRB(context.tokens.pad + 2, 8, context.tokens.pad + 2, 0),
          child: Text(
            edits.lastError!,
            key: const Key('material.error'),
            style: Theme.of(context).textTheme.bodySmall?.copyWith(color: context.tokens.error),
          ),
        );

  /// A shader asset's material: the surface options its shader allows,
  /// then one field per property.
  List<Widget> _custom(
    BuildContext context,
    Map<String, Object?> f,
    ShaderInfo? shader,
    String shaderId,
    bool enabled,
  ) {
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall;
    if (shader == null || !shader.ok) {
      return [
        InspectorSection(
          title: 'Surface Inputs',
          children: [
            Text(
              shader == null
                  ? 'The shader is missing ($shaderId): the material draws magenta.'
                  : 'The shader ${shader.name} does not compile (see its page): the material draws magenta.',
              key: const Key('material.shaderProblem'),
              style: small?.copyWith(color: t.error),
            ),
          ],
        ),
      ];
    }
    final mode = f['alpha_mode'] is String ? f['alpha_mode'] as String : 'opaque';
    final params = f['parameters'] is Map ? Map<String, Object?>.from(f['parameters'] as Map) : <String, Object?>{};
    final textures = f['textures'] is Map ? Map<String, Object?>.from(f['textures'] as Map) : <String, Object?>{};
    void setParam(String name, Object value) => _set('parameters', {...params, name: value});
    void showParam(String name, Object? value) => edits.show(
      id,
      value == null
          ? null
          : {
              'parameters': {...params, name: value},
            },
    );

    Widget field(ShaderProperty p) {
      final value = params[p.name] ?? p.defaultValue;
      final key = Key('material.param.${p.name}');
      switch (p.type) {
        case 'float' || 'int':
          final integer = p.type == 'int';
          return KeyedSubtree(
            key: key,
            child: NumberField(
              label: p.label,
              tooltip: p.tooltip,
              value: value is num ? value : 0,
              min: p.min,
              max: p.max,
              step: integer ? 1 : 0.01,
              integer: integer,
              slider: p.min != null && p.max != null,
              enabled: enabled,
              onCommit: (v) => setParam(p.name, v),
              onPreview: (v) => showParam(p.name, v),
            ),
          );
        case 'bool':
          return KeyedSubtree(
            key: key,
            child: FieldRow(
              label: p.label,
              tooltip: p.tooltip,
              child: BoolField(
                value: value == true || value == 1,
                enabled: enabled,
                onCommit: (v) => setParam(p.name, v),
              ),
            ),
          );
        case 'color3' || 'color':
          return KeyedSubtree(
            key: key,
            child: FieldRow(
              label: p.label,
              tooltip: p.tooltip,
              child: ColorField(
                value: value is List ? [for (final v in value) (v as num?) ?? 0] : null,
                components: p.components,
                enabled: enabled,
                onCommit: (rgb) => setParam(p.name, rgb),
                onPreview: (rgb) => showParam(p.name, rgb),
              ),
            ),
          );
        case 'vec2' || 'vec3' || 'vec4':
          final list = value is List ? [for (final v in value) (v as num?) ?? 0] : List<num>.filled(p.components, 0);
          List<num> withComponent(int i, num v) => [...list]..[i] = v;
          return KeyedSubtree(
            key: key,
            child: FieldRow(
              label: p.label,
              tooltip: p.tooltip,
              child: VecField(
                value: list,
                components: p.components,
                min: p.min,
                max: p.max,
                step: 0.01,
                enabled: enabled,
                onCommitComponent: (i, v) => setParam(p.name, withComponent(i, v as num)),
                onPreviewComponent: (i, v) => showParam(p.name, v == null ? null : withComponent(i, v)),
              ),
            ),
          );
        case 'texture':
          final binding = textures[p.name];
          final ref = binding is Map && binding['texture'] is Map
              ? Map<String, Object?>.from(binding['texture'] as Map)
              : null;
          final project = widget.browser.project;
          return KeyedSubtree(
            key: key,
            child: FieldRow(
              label: p.label,
              tooltip: p.tooltip,
              child: AssetField(
                value: ref,
                kind: 'texture',
                nullable: true,
                enabled: enabled,
                subject: p.label,
                describe: (r) => r == null ? 'None (${p.defaultValue})' : describeAssetRef(project, r),
                candidates: () => [
                  for (final a in project.assets())
                    if (a.offeredKinds.contains('texture')) a,
                ],
                thumbnail: (e, size) => AssetPreviewWidget(preview: widget.browser.preview(e), size: size),
                onPing: widget.browser.ping,
                onCommit: (r) {
                  final next = {...textures};
                  if (r == null) {
                    next.remove(p.name);
                  } else {
                    next[p.name] = {...binding is Map ? Map<String, Object?>.from(binding) : const {}, 'texture': r};
                  }
                  next.isEmpty ? edits.edit(id, {'/textures': unset}) : _set('textures', next);
                },
              ),
            ),
          );
      }
      return InspectorValueRow(p.label, '$value');
    }

    return [
      InspectorSection(
        title: 'Surface Options',
        children: [
          if (shader.alpha.length > 1)
            KeyedSubtree(
              key: const Key('material.alpha_mode'),
              child: FieldRow(
                label: 'Rendering Mode',
                tooltip:
                    'The modes ${shader.name} allows: Opaque draws solid; Cutout drops what is under the cutoff; Transparent blends.',
                child: EnumField(
                  value: mode,
                  values: shader.alpha,
                  display: (v) => _renderingModes[v] ?? v,
                  enabled: enabled,
                  onCommit: (v) => _set('alpha_mode', v),
                ),
              ),
            ),
          if (mode == 'mask')
            KeyedSubtree(
              key: const Key('material.alpha_cutoff'),
              child: NumberField(
                label: 'Alpha Cutoff',
                value: f['alpha_cutoff'] is num ? f['alpha_cutoff'] as num : 0.5,
                min: 0,
                max: 1,
                step: 0.01,
                slider: true,
                enabled: enabled,
                onCommit: (v) => _set('alpha_cutoff', v),
                onPreview: (v) => edits.show(id, v == null ? null : {'alpha_cutoff': v}),
              ),
            ),
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
          if (shader.properties.isEmpty)
            Text('${shader.name} has no properties.', style: small?.copyWith(color: t.textDisabled)),
          for (final p in shader.properties) field(p),
        ],
      ),
    ];
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

/// The material's Shader dropdown (ADR-0031 § 7): the built-in Lit first,
/// then the project's shaders, then the packages'; a `/` in a shader's name
/// makes a submenu, as in Unity. A shader that does not compile is listed
/// with its errors noted, and picking it draws the material magenta.
class ShaderField extends StatelessWidget {
  const ShaderField({
    super.key,
    required this.current,
    required this.shaders,
    required this.onPick,
    this.enabled = true,
  });

  /// What the material's `shader` names: `fynkel.pbr@1` or an asset id.
  final String current;
  final List<ShaderInfo> shaders;
  final void Function(ShaderInfo shader) onPick;
  final bool enabled;

  String _shown() {
    final s = shaders.where((s) => s.shader == current).firstOrNull;
    if (s != null) return s.ok ? s.name : '${s.name} (errors)';
    return current == pbrShader ? 'Lit' : 'Missing shader';
  }

  List<Widget> _items(BuildContext context, _MenuNode node) {
    final style = Theme.of(context).textTheme.bodySmall;
    final out = <Widget>[];
    for (final e in node.children.entries) {
      final n = e.value;
      for (final s in n.leaves) {
        out.add(
          MenuItemButton(
            key: Key('material.shader.${s.shader}'),
            leadingIcon: SizedBox(width: 16, child: s.shader == current ? const Icon(FIcons.check, size: 14) : null),
            onPressed: () => onPick(s),
            child: Text(s.ok ? e.key : '${e.key} (errors)', style: style),
          ),
        );
      }
      if (n.children.isNotEmpty) {
        out.add(
          SubmenuButton(
            menuChildren: _items(context, n),
            child: Text(e.key, style: style),
          ),
        );
      }
    }
    return out;
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final builtin = shaders.where((s) => s.builtin).toList();
    final own = _MenuNode.of(shaders.where((s) => !s.builtin && s.package == null));
    final packaged = _MenuNode.of(shaders.where((s) => !s.builtin && s.package != null));
    final lit = _MenuNode()..children.addAll({for (final s in builtin) s.name: _MenuNode()..leaves.add(s)});
    return MenuAnchor(
      style: const MenuStyle(visualDensity: VisualDensity.compact),
      menuChildren: [
        ..._items(context, lit),
        if (own.children.isNotEmpty) ...[const Divider(height: 8), ..._items(context, own)],
        if (packaged.children.isNotEmpty) ...[const Divider(height: 8), ..._items(context, packaged)],
      ],
      builder: (context, controller, _) => SizedBox(
        height: t.fieldHeight,
        child: InkWell(
          key: const Key('material.shader.button'),
          onTap: enabled ? () => controller.isOpen ? controller.close() : controller.open() : null,
          child: FieldBox(
            padding: const EdgeInsets.only(left: 6, right: 4),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    _shown(),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(color: enabled ? t.text : t.textDisabled),
                  ),
                ),
                Icon(FIcons.chevronDown, size: 12, color: t.textDisabled),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// A level of the Shader menu: its submenus by name and the shaders at it.
final class _MenuNode {
  final children = <String, _MenuNode>{};
  final leaves = <ShaderInfo>[];

  /// [shaders] by the `/`-separated parts of their names, sorted.
  static _MenuNode of(Iterable<ShaderInfo> shaders) {
    final root = _MenuNode();
    final sorted = shaders.toList()..sort((a, b) => a.name.toLowerCase().compareTo(b.name.toLowerCase()));
    for (final s in sorted) {
      final parts = s.name.split('/').where((p) => p.isNotEmpty).toList();
      if (parts.isEmpty) parts.add(s.name);
      var node = root;
      for (final part in parts.take(parts.length - 1)) {
        node = node.children.putIfAbsent(part, _MenuNode.new);
      }
      node.children.putIfAbsent(parts.last, _MenuNode.new).leaves.add(s);
    }
    return root;
  }
}
