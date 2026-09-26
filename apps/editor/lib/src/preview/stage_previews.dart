// The editor's 3D previews (ADR-0030 §5), each a [PreviewContext.stage]
// with its toolbar and info line, as Unity's:
// - a material on a shape (Sphere by default; Cube, Cylinder, Capsule,
//   Plane from the toolbar), or on any mesh dropped onto the picture from
//   the Project;
// - a model (or a rig) whole, or one of its meshes from the toolbar;
// - a prefab as it is.
// The info line counts what is drawn: meshes, vertices, triangles, the
// skinned ones, and the bounds' size.
import 'package:flutter/material.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../inspector/field_widgets.dart';
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'preview_provider.dart';
import 'stage_view.dart';

const _shapes = ['sphere', 'cube', 'cylinder', 'capsule', 'plane'];

String _title(String word) => word.isEmpty ? word : '${word[0].toUpperCase()}${word.substring(1)}';

/// `1,234`.
String _count(int n) {
  final s = '$n';
  final out = StringBuffer();
  for (var i = 0; i < s.length; i++) {
    if (i > 0 && (s.length - i) % 3 == 0) out.write(',');
    out.write(s[i]);
  }
  return out.toString();
}

String _metres(double m) => m >= 10 ? m.toStringAsFixed(1) : m.toStringAsFixed(2);

/// The info line of a stage: `2 meshes  1 skinned  1,024 verts  2,040 tris  1.00 × 2.00 × 0.50 m`.
String stageInfo(PreviewStageStats? s, {String? lead}) {
  if (s == null) return lead ?? '';
  return [
    ?lead,
    if (s.meshes != 1 || lead == null) '${s.meshes} ${s.meshes == 1 ? 'mesh' : 'meshes'}',
    if (s.skinned > 0) '${s.skinned} skinned',
    '${_count(s.vertices)} verts',
    '${_count(s.triangles)} tris',
    if (s.size case final size?) '${size.map(_metres).join(' × ')} m',
  ].join('  ');
}

/// A material's shape: a built-in's name, or `{asset, sub, name}` for a
/// dropped mesh.
Object _shapeOf(PreviewContext preview) => preview.value<Object>('shape', 'sphere');

String _shapeName(Object shape) => shape is Map ? '${shape['name'] ?? 'Mesh'}' : _title('$shape');

final materialPreview = PreviewProvider(
  package: PreviewProvider.editorPackage,
  kind: 'material',
  build: (context, preview) {
    final shape = _shapeOf(preview);
    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) => d.data.kind == 'mesh',
      onAcceptWithDetails: (d) => preview.set('shape', {'asset': d.data.id, 'sub': d.data.sub, 'name': d.data.name}),
      builder: (context, over, _) => Stack(
        fit: StackFit.expand,
        children: [
          preview.stage(
            shape: shape is Map ? {'asset': shape['asset'], 'sub': shape['sub']} : shape,
            onStats: (s) => preview.set('stats', s),
          ),
          if (over.isNotEmpty)
            IgnorePointer(
              child: DecoratedBox(
                decoration: BoxDecoration(border: Border.all(color: context.tokens.accent, width: 2)),
              ),
            ),
        ],
      ),
    );
  },
  toolbar: (context, preview) {
    final shape = _shapeOf(preview);
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall;
    return _Menu(
      key: const Key('preview.shape'),
      label: _shapeName(shape),
      tooltip: 'Preview shape (or drop a mesh on the picture)',
      items: [for (final s in _shapes) (key: 'preview.shape.$s', value: s, label: _title(s))],
      selected: shape is String ? shape : null,
      onSelected: (s) => preview.set('shape', s),
      style: small?.copyWith(color: t.textSecondary),
    );
  },
  info: (preview) => stageInfo(preview.value<PreviewStageStats?>('stats', null), lead: _shapeName(_shapeOf(preview))),
);

/// A model's meshes (`mesh/Body`), from its asset detail.
List<String> modelMeshes(BuildContext context, String asset) {
  final detail = PreviewStageScope.maybeOf(context)?.project.assetDetail(asset);
  return [
    for (final sub in detail?.subAssets.keys ?? const <String>[])
      if (sub.startsWith('mesh/')) sub,
  ];
}

Widget _modelToolbar(BuildContext context, PreviewContext preview) {
  final meshes = modelMeshes(context, preview.asset.id);
  if (meshes.isEmpty) return const SizedBox.shrink();
  final part = preview.value<String?>('part', null);
  final t = context.tokens;
  return _Menu(
    key: const Key('preview.part'),
    label: part == null ? 'Whole' : part.substring('mesh/'.length),
    tooltip: 'Show the whole model or one mesh',
    items: [
      (key: 'preview.part.whole', value: '', label: 'Whole'),
      for (final m in meshes) (key: 'preview.part.$m', value: m, label: m.substring('mesh/'.length)),
    ],
    selected: part ?? '',
    onSelected: (m) => preview.set('part', m.isEmpty ? null : m),
    style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.textSecondary),
  );
}

Widget _hierarchy(BuildContext context, PreviewContext preview) =>
    preview.stage(sub: preview.value<String?>('part', null), onStats: (s) => preview.set('stats', s));

String _hierarchyInfo(PreviewContext preview) => stageInfo(preview.value<PreviewStageStats?>('stats', null));

final modelPreview = PreviewProvider(
  package: PreviewProvider.editorPackage,
  kind: 'model',
  build: _hierarchy,
  toolbar: _modelToolbar,
  info: _hierarchyInfo,
);

final rigPreview = PreviewProvider(
  package: PreviewProvider.editorPackage,
  kind: 'rig',
  build: _hierarchy,
  toolbar: _modelToolbar,
  info: _hierarchyInfo,
);

final prefabPreview = PreviewProvider(
  package: PreviewProvider.editorPackage,
  kind: 'prefab',
  build: _hierarchy,
  info: _hierarchyInfo,
);

typedef _Item = ({String key, String value, String label});

/// A toolbar's drop-down: its value's name and a chevron.
class _Menu extends StatelessWidget {
  const _Menu({
    super.key,
    required this.label,
    required this.tooltip,
    required this.items,
    required this.selected,
    required this.onSelected,
    this.style,
  });

  final String label, tooltip;
  final List<_Item> items;
  final String? selected;
  final void Function(String value) onSelected;
  final TextStyle? style;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return PopupMenuButton<String>(
      popUpAnimationStyle: AnimationStyle.noAnimation,
      tooltip: tooltip,
      padding: EdgeInsets.zero,
      onSelected: onSelected,
      itemBuilder: (_) => [
        for (final i in items)
          PopupMenuItem(
            key: Key(i.key),
            value: i.value,
            height: 26,
            child: Row(
              children: [
                SizedBox(width: 16, child: i.value == selected ? Icon(FIcons.check, size: 12, color: t.text) : null),
                Text(i.label, style: style?.copyWith(color: t.text)),
              ],
            ),
          ),
      ],
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(label, style: style),
            Icon(FIcons.arrowDropDown, size: 12, color: t.textSecondary),
          ],
        ),
      ),
    );
  }
}
