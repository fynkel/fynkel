// A `.shader` asset's sections on its Inspector page (ADR-0031 § 7), as
// Unity's shader Inspector: what the header says (the menu name, lighting,
// the alpha modes, shadows), *Compiled* or the errors — a click opens the
// IDE at the line — the properties a material shows, and Create Material.
// The code itself is edited in the IDE (Open, or a double-click in the
// Project); saving it recooks, and the page reads the file again.
import 'package:flutter/material.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../project/editor_project.dart';
import '../project_browser/project_browser_model.dart';
import '../shell/notices.dart';
import '../theme/icons.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'inspector_chrome.dart';

/// Opens a file in the IDE, at a line when given; the error, if any.
typedef OpenInIde = Future<String?> Function(String file, {int? line, int? column});

/// Unity's names for the alpha modes.
const alphaModeNames = {'opaque': 'Opaque', 'mask': 'Cutout', 'blend': 'Transparent'};

/// What a property's type and default read as: `Float = 0.5  [0, 1]`.
String describeProperty(ShaderProperty p) {
  String number(Object? v) => v is num ? (v == v.roundToDouble() ? '${v.toInt()}' : '$v') : '$v';
  final type = switch (p.type) {
    'color3' => 'Color (RGB)',
    'color' => 'Color',
    'texture' => 'Texture',
    'vec2' || 'vec3' || 'vec4' => 'Vector${p.type.substring(3)}',
    final t => '${t[0].toUpperCase()}${t.substring(1)}',
  };
  final d = p.defaultValue;
  final shown = d is List ? '(${d.map(number).join(', ')})' : number(d);
  final range = p.min != null || p.max != null ? '  [${number(p.min ?? '')}, ${number(p.max ?? '')}]' : '';
  final extra = p.isTexture ? '${p.colorSpace == 'linear' ? ', linear' : ''}${p.required ? ', required' : ''}' : '';
  return '$type = $shown$range$extra';
}

class ShaderSections extends StatelessWidget {
  const ShaderSections({super.key, required this.model, required this.entry, this.openInIde});

  final ProjectBrowserModel model;
  final AssetEntry entry;
  final OpenInIde? openInIde;

  String get _file => entry.file ?? model.project.assetFile(entry.path);

  Future<void> _open(BuildContext context, {int? line, int? column}) async {
    final notices = Notices.maybeOf(context);
    final err = await openInIde?.call(_file, line: line, column: column);
    if (err != null) notices?.show('Open in IDE failed: $err');
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final info = model.project.shaders().where((s) => s.shader == entry.id).firstOrNull;
    if (info == null) {
      return const InspectorSection(title: 'Shader', children: [InspectorValueRow('Status', 'Not read yet')]);
    }
    final small = theme.textTheme.bodySmall;
    Widget button(String key, String label, VoidCallback? onPressed) => OutlinedButton(
      key: Key(key),
      style: OutlinedButton.styleFrom(
        visualDensity: VisualDensity.compact,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        minimumSize: Size(0, t.row + 2),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
      onPressed: onPressed,
      child: Text(label),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InspectorSection(
          title: 'Shader',
          children: [
            InspectorValueRow('Name', info.name, key: const Key('shader.name')),
            if (info.ok) ...[
              InspectorValueRow('Lighting', info.lighting == 'unlit' ? 'Unlit' : 'Lit'),
              InspectorValueRow('Alpha', info.alpha.map((a) => alphaModeNames[a] ?? a).join(', ')),
              InspectorValueRow('Shadows', info.shadows ? 'Casts' : 'Off'),
              if (info.vertex) const InspectorValueRow('Vertices', 'Moved by vertex()'),
            ],
            if (info.package != null) InspectorValueRow('Package', info.package!),
            Padding(
              padding: const EdgeInsets.only(top: 4, bottom: 2),
              child: Row(
                key: const Key('shader.status'),
                children: [
                  Icon(info.ok ? FIcons.codegenOk : FIcons.errorOutline, size: 14, color: info.ok ? t.ok : t.error),
                  const SizedBox(width: 6),
                  Expanded(
                    child: Text(
                      info.ok
                          ? 'Compiled'
                          : '${info.diagnostics.length} error${info.diagnostics.length == 1 ? '' : 's'}: '
                                'its materials draw magenta',
                      style: small?.copyWith(color: info.ok ? t.text : t.error),
                    ),
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Wrap(
                spacing: 6,
                runSpacing: 4,
                children: [
                  if (openInIde != null) button('shader.open', 'Open in IDE', () => _open(context)),
                  button(
                    'shader.createMaterial',
                    'Create Material',
                    model.canEdit && info.ok && entry.package == null ? () => model.newMaterialFor(entry) : null,
                  ),
                ],
              ),
            ),
          ],
        ),
        if (!info.ok)
          InspectorSection(
            title: 'Errors',
            children: [
              for (final (i, d) in info.diagnostics.indexed)
                InkWell(
                  key: Key('shader.error.$i'),
                  onTap: openInIde == null ? null : () => _open(context, line: d.line, column: d.column),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 3),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        SizedBox(
                          width: 64,
                          child: Text(
                            d.line == null ? '' : 'line ${d.line}',
                            style: monoStyle(context, color: t.textSecondary),
                          ),
                        ),
                        Expanded(
                          child: Text(
                            d.suggestion == null ? d.message : '${d.message}\n${d.suggestion}',
                            style: small?.copyWith(color: t.error),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
            ],
          ),
        if (info.ok)
          InspectorSection(
            title: 'Properties',
            children: [
              if (info.properties.isEmpty)
                Text('None: its materials have no fields.', style: small?.copyWith(color: t.textDisabled)),
              for (final p in info.properties)
                Tooltip(
                  message: p.tooltip ?? '',
                  waitDuration: const Duration(milliseconds: 500),
                  child: InspectorValueRow(p.label, describeProperty(p), key: Key('shader.property.${p.name}')),
                ),
            ],
          ),
      ],
    );
  }
}
