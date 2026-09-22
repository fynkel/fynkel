// The Asset panel (item 5): the Project browser's lead asset in full — its
// preview, file, a source's sidecar (the import settings edited here and
// written through the session, then reimported), its sub-assets (dragged
// one by one), a material's factors, and every document that references
// it (a click opens the document at the field).
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../env/tools.dart';
import '../inspector/field_widgets.dart' show AssetDragPayload, assetKindIcon;
import '../project/editor_project.dart';
import 'asset_preview.dart';
import 'project_browser_model.dart';

class AssetPanel extends StatefulWidget {
  const AssetPanel({
    super.key,
    required this.model,
    required this.onOpenDocument,
    required this.onOpenReference,
    this.run = runProcess,
  });

  final ProjectBrowserModel model;
  final void Function(String id) onOpenDocument;

  /// A "Referenced by" row: the referencing document's asset-root-relative
  /// path and the field's JSON pointer.
  final void Function(String documentPath, String? pointer) onOpenReference;
  final RunProcess run;

  @override
  State<AssetPanel> createState() => _AssetPanelState();
}

class _AssetPanelState extends State<AssetPanel> {
  ProjectBrowserModel get model => widget.model;
  String? _draftFor;
  Map<String, Object?> _draft = {};
  final _json = TextEditingController();
  String? _jsonError;
  OperationResult? _reimport;
  String? _reimportFor;

  @override
  void initState() {
    super.initState();
    model.addListener(_onModel);
  }

  @override
  void didUpdateWidget(covariant AssetPanel old) {
    super.didUpdateWidget(old);
    if (old.model != widget.model) {
      old.model.removeListener(_onModel);
      widget.model.addListener(_onModel);
    }
  }

  @override
  void dispose() {
    model.removeListener(_onModel);
    _json.dispose();
    super.dispose();
  }

  void _onModel() {
    if (mounted) setState(() {});
  }

  /// The settings being edited for [id]: the sidecar's until touched.
  Map<String, Object?> _draftOf(String id, AssetDetail d) {
    if (_draftFor != id) {
      _draftFor = id;
      _draft = Map.of(d.settings);
      _json.text = _pretty(_draft);
      _jsonError = null;
    }
    return _draft;
  }

  static String _pretty(Map<String, Object?> m) => const JsonEncoder.withIndent('  ').convert(m);

  bool _dirty(AssetDetail d) => _jsonError == null && _pretty(_draft) != _pretty(d.settings);

  void _set(String key, Object? value) {
    setState(() {
      if (value == null) {
        _draft.remove(key);
      } else {
        _draft[key] = value;
      }
      _json.text = _pretty(_draft);
      _jsonError = null;
    });
  }

  void _jsonChanged(String text) {
    setState(() {
      try {
        final v = jsonDecode(text);
        if (v is! Map) {
          _jsonError = 'The settings are a JSON object.';
          return;
        }
        _draft = {for (final e in v.entries) e.key as String: e.value};
        _jsonError = null;
      } on FormatException catch (e) {
        _jsonError = e.message;
      }
    });
  }

  void _apply(AssetDetail d) {
    if (model.setImportSettings(d.id, Map.of(_draft)) == null) return;
    _draftFor = null;
    _runReimport(d.id);
  }

  void _runReimport(String id) {
    final r = model.reimport(id);
    setState(() {
      _reimport = r;
      _reimportFor = id;
    });
  }

  Future<void> _reveal(String rel) async {
    final err = await revealInFileBrowser(model.project.assetFile(rel), run: widget.run);
    if (err != null && mounted) {
      ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text('Reveal in Finder failed: $err')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final e = model.leadEntry;
    if (e == null) return _empty(theme);
    final d = model.detail(e.id);
    if (d == null) return _empty(theme, message: '${e.path} left the database.');
    final draft = e.isSource ? _draftOf(e.id, d) : null;
    final isDoc = e.document == 'scene' || e.document == 'prefab';
    // Its own Material: the tiles below paint their ink on it, not on the panel frame.
    return Material(
      type: MaterialType.transparency,
      child: ListView(
        key: Key('asset.panel.${e.id}'),
        padding: const EdgeInsets.all(8),
        children: [
          _header(theme, e, d),
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            runSpacing: 4,
            children: [
              if (isDoc)
                FilledButton.tonalIcon(
                  key: const Key('asset.open'),
                  onPressed: () => widget.onOpenDocument(e.id),
                  icon: const Icon(Icons.open_in_new, size: 16),
                  label: const Text('Open'),
                ),
              OutlinedButton.icon(
                key: const Key('asset.reveal'),
                onPressed: () => _reveal(e.path),
                icon: const Icon(Icons.folder_open, size: 16),
                label: const Text('Reveal in Finder'),
              ),
              if (e.isSource)
                OutlinedButton.icon(
                  key: const Key('asset.reimport'),
                  onPressed: model.canEdit ? () => _runReimport(e.id) : null,
                  icon: const Icon(Icons.refresh, size: 16),
                  label: const Text('Reimport'),
                ),
              OutlinedButton.icon(
                key: const Key('asset.delete'),
                onPressed: model.canEdit ? () => model.delete(ids: [e.id]) : null,
                icon: const Icon(Icons.delete_outline, size: 16),
                label: const Text('Delete'),
              ),
            ],
          ),
          const SizedBox(height: 12),
          _section(theme, 'File', [
            _kv(theme, 'Path', 'assets/${e.path}'),
            if (d.size != null) _kv(theme, 'Size', _bytes(d.size!)),
            if (d.modified != null)
              _kv(
                theme,
                'Modified',
                DateTime.fromMillisecondsSinceEpoch((d.modified! * 1000).round()).toLocal().toString().split('.').first,
              ),
          ]),
          if (e.isSource && draft != null) ...[
            const SizedBox(height: 12),
            _section(theme, 'Import', [
              _kv(theme, 'Importer', '${d.importer ?? e.importer}  v${d.importerVersion ?? '?'}'),
              _kv(theme, 'Sidecar', d.sidecarPath == null ? '${e.path}.meta.json' : p.basename(d.sidecarPath!)),
              const SizedBox(height: 6),
              ..._settingsControls(theme, e, draft),
              ExpansionTile(
                key: const Key('asset.settings.json'),
                title: Text('Settings as JSON', style: theme.textTheme.bodySmall),
                tilePadding: EdgeInsets.zero,
                childrenPadding: const EdgeInsets.only(bottom: 6),
                dense: true,
                children: [
                  TextField(
                    key: const Key('asset.settings.jsonField'),
                    controller: _json,
                    maxLines: null,
                    enabled: model.canEdit,
                    style: theme.textTheme.bodySmall?.copyWith(fontFamily: 'monospace'),
                    decoration: InputDecoration(
                      isDense: true,
                      border: const OutlineInputBorder(),
                      errorText: _jsonError,
                    ),
                    onChanged: _jsonChanged,
                  ),
                ],
              ),
              Row(
                children: [
                  FilledButton.icon(
                    key: const Key('asset.applySettings'),
                    onPressed: model.canEdit && _dirty(d) ? () => _apply(d) : null,
                    icon: const Icon(Icons.check, size: 16),
                    label: const Text('Apply and Reimport'),
                  ),
                  const SizedBox(width: 6),
                  if (_dirty(d))
                    TextButton(
                      key: const Key('asset.revertSettings'),
                      onPressed: () => setState(() => _draftFor = null),
                      child: const Text('Revert'),
                    ),
                ],
              ),
              if (_reimport != null && _reimportFor == e.id) _reimportResult(theme, _reimport!),
            ]),
            const SizedBox(height: 12),
            _section(theme, 'Sub-assets (${d.subAssets.length})', [
              if (d.subAssets.isEmpty)
                Text('None — this source imports as one asset.', style: theme.textTheme.bodySmall),
              for (final s in d.subAssets.entries) _subAssetRow(theme, e, s.key, s.value),
            ]),
          ],
          if (d.material != null) ...[
            const SizedBox(height: 12),
            _section(theme, 'Material', [
              _kv(theme, 'Shader', '${d.material!['shader']}'),
              _kv(
                theme,
                'Alpha',
                '${d.material!['alphaMode']}${d.material!['alphaMode'] == 'mask' ? ' at ${d.material!['alphaCutoff']}' : ''}',
              ),
              _kv(theme, 'Double-sided', d.material!['doubleSided'] == true ? 'yes' : 'no'),
              if (d.material!['fields'] is Map)
                for (final f in (d.material!['fields'] as Map).entries) _kv(theme, '${f.key}', _value(f.value)),
            ]),
          ],
          if (d.entities != null) ...[
            const SizedBox(height: 12),
            _section(theme, '${e.document![0].toUpperCase()}${e.document!.substring(1)}', [
              _kv(theme, 'Entities', '${d.entities}'),
              if (!e.isSource && e.name.isNotEmpty) _kv(theme, 'Name', e.name),
            ]),
          ],
          if (d.fields != null && d.material == null && d.entities == null) ...[
            const SizedBox(height: 12),
            _section(theme, 'Document', [
              for (final f in d.fields!.entries)
                if (f.key != 'id' && f.key != 'formatVersion' && f.key != 'kind') _kv(theme, f.key, _value(f.value)),
            ]),
          ],
          const SizedBox(height: 12),
          _dependents(theme, e, d),
        ],
      ),
    );
  }

  Widget _empty(ThemeData theme, {String? message}) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.inventory_2_outlined, size: 40, color: theme.disabledColor),
            const SizedBox(height: 12),
            Text('No asset selected', style: theme.textTheme.titleMedium),
            const SizedBox(height: 4),
            Text(
              message ?? 'Select an asset in the Project panel to see its file, import settings, sub-assets and uses.',
              key: const Key('asset.empty'),
              textAlign: TextAlign.center,
              style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
            ),
          ],
        ),
      ),
    );
  }

  Widget _header(ThemeData theme, AssetEntry e, AssetDetail d) {
    final kind = browserKindOf(e);
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        AssetPreviewWidget(preview: model.preview(e), size: 48),
        const SizedBox(width: 10),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(p.basename(e.path), key: const Key('asset.title'), style: theme.textTheme.titleMedium),
              Text(
                e.isSource
                    ? '$kind · imported by ${e.importer}'
                    : '$kind document${e.name.isNotEmpty && e.name != ProjectBrowserModel.editableName(e) ? ' · “${e.name}”' : ''}',
                style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
              ),
              Row(
                children: [
                  Flexible(
                    child: Text(
                      e.id,
                      style: theme.textTheme.bodySmall?.copyWith(fontFamily: 'monospace', color: theme.hintColor),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  IconButton(
                    key: const Key('asset.copyId'),
                    tooltip: 'Copy id',
                    icon: const Icon(Icons.copy, size: 14),
                    visualDensity: VisualDensity.compact,
                    onPressed: () => Clipboard.setData(ClipboardData(text: e.id)),
                  ),
                ],
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _section(ThemeData theme, String title, List<Widget> children) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(title, style: theme.textTheme.labelLarge),
        const Divider(height: 8),
        ...children,
      ],
    );
  }

  Widget _kv(ThemeData theme, String key, String value) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 110,
            child: Text(key, style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor)),
          ),
          Expanded(child: SelectableText(value, style: theme.textTheme.bodySmall)),
        ],
      ),
    );
  }

  static String _value(Object? v) => v is Map || v is List ? jsonEncode(v) : '$v';

  static String _bytes(int n) {
    if (n < 1024) return '$n B';
    if (n < 1024 * 1024) return '${(n / 1024).toStringAsFixed(1)} KB';
    return '${(n / (1024 * 1024)).toStringAsFixed(2)} MB';
  }

  /// The importer's known settings as controls; the JSON field below
  /// covers everything else.
  List<Widget> _settingsControls(ThemeData theme, AssetEntry e, Map<String, Object?> draft) {
    final enabled = model.canEdit;
    switch (e.importer) {
      case 'fynkel.image':
        final v = draft['colorSpace'];
        return [
          Row(
            children: [
              SizedBox(
                width: 110,
                child: Text('Colour space', style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor)),
              ),
              Expanded(
                child: DropdownButton<String>(
                  key: const Key('asset.settings.colorSpace'),
                  value: v == 'linear' ? 'linear' : 'srgb',
                  isDense: true,
                  isExpanded: true,
                  items: const [
                    DropdownMenuItem(
                      value: 'srgb',
                      child: Text('sRGB (colour)', overflow: TextOverflow.ellipsis),
                    ),
                    DropdownMenuItem(
                      value: 'linear',
                      child: Text('linear (data)', overflow: TextOverflow.ellipsis),
                    ),
                  ],
                  onChanged: enabled ? (s) => _set('colorSpace', s == 'srgb' ? null : s) : null,
                ),
              ),
            ],
          ),
          Text(
            v == 'linear'
                ? 'Data textures: normal maps, roughness, metallic, masks.'
                : 'Colour textures: base colour, emissive. The default.',
            style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor),
          ),
        ];
      case 'fynkel.audio':
        return [
          SwitchListTile(
            key: const Key('asset.settings.stream'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            title: Text('Stream from disk', style: theme.textTheme.bodySmall),
            subtitle: Text(
              'Music and long sounds; short effects stay decoded in memory.',
              style: theme.textTheme.labelSmall,
            ),
            value: draft['stream'] == true,
            onChanged: enabled ? (b) => _set('stream', b ? true : null) : null,
          ),
        ];
      case 'fynkel.gltf' || 'fynkel.fbx':
        return [
          SwitchListTile(
            key: const Key('asset.settings.allowExternalFiles'),
            dense: true,
            contentPadding: EdgeInsets.zero,
            title: Text('Allow external files', style: theme.textTheme.bodySmall),
            subtitle: Text(
              'Textures and buffers next to the file by plain relative paths; off, the file must be self-contained.',
              style: theme.textTheme.labelSmall,
            ),
            value: draft['allowExternalFiles'] == true,
            onChanged: enabled ? (b) => _set('allowExternalFiles', b ? true : null) : null,
          ),
        ];
      default:
        return [
          Text(
            '${e.importer} has no settings the editor knows; edit the JSON below.',
            style: theme.textTheme.bodySmall,
          ),
        ];
    }
  }

  Widget _reimportResult(ThemeData theme, OperationResult r) {
    final color = r.ok ? theme.colorScheme.primaryContainer : theme.colorScheme.errorContainer;
    final on = r.ok ? theme.colorScheme.onPrimaryContainer : theme.colorScheme.onErrorContainer;
    final summary = r.json['summary'];
    final lines = <String>[];
    if (r.ok) {
      lines.add('Reimported${r.json['sidecarWritten'] == true ? ' — the sidecar gained new sub-assets' : ''}.');
      if (summary is Map) {
        for (final k in ['meshes', 'materials', 'textures', 'animations', 'skins', 'nodes']) {
          final v = summary[k];
          if (v is List && v.isNotEmpty) lines.add('${v.length} $k');
          if (v is num) lines.add('$v $k');
        }
        for (final k in ['width', 'height', 'colorSpace', 'channels', 'sampleRate', 'durationSeconds', 'stream']) {
          if (summary[k] != null) lines.add('$k: ${summary[k]}');
        }
      }
    } else {
      lines.add('The importer refused — the sidecar is unchanged:');
      lines.addAll(r.diagnostics.map((d) => '${d.code}: ${d.message}${d.pointer != null ? ' (${d.pointer})' : ''}'));
    }
    return Container(
      key: const Key('asset.reimportResult'),
      margin: const EdgeInsets.only(top: 6),
      padding: const EdgeInsets.all(6),
      decoration: BoxDecoration(color: color, borderRadius: BorderRadius.circular(4)),
      child: Text(lines.join('\n'), style: theme.textTheme.bodySmall?.copyWith(color: on)),
    );
  }

  Widget _subAssetRow(ThemeData theme, AssetEntry e, String sub, String uuid) {
    final kind = AssetEntry.subKind(sub);
    final payload = AssetDragPayload(id: e.id, sub: sub, kind: kind, name: '${p.basename(e.path)}#$sub');
    final row = Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Row(
        children: [
          Icon(assetKindIcon(kind), size: 14, color: theme.hintColor),
          const SizedBox(width: 6),
          Expanded(child: Text(sub, style: theme.textTheme.bodySmall)),
          Text(
            '${uuid.length > 8 ? uuid.substring(0, 8) : uuid}…',
            style: theme.textTheme.labelSmall?.copyWith(fontFamily: 'monospace', color: theme.hintColor),
          ),
          if (kind == 'prefab')
            IconButton(
              key: Key('asset.sub.open.$sub'),
              tooltip: 'Open the imported hierarchy in a tab (read-only; Prefab mode)',
              icon: const Icon(Icons.open_in_new, size: 12),
              visualDensity: VisualDensity.compact,
              onPressed: () => widget.onOpenDocument('${e.id}#$sub'),
            ),
          IconButton(
            tooltip: 'Copy the sub-asset uuid',
            icon: const Icon(Icons.copy, size: 12),
            visualDensity: VisualDensity.compact,
            onPressed: () => Clipboard.setData(ClipboardData(text: uuid)),
          ),
        ],
      ),
    );
    return Draggable<AssetDragPayload>(
      key: Key('asset.sub.$sub'),
      data: payload,
      dragAnchorStrategy: pointerDragAnchorStrategy,
      feedback: Material(
        elevation: 4,
        borderRadius: BorderRadius.circular(4),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
          child: Text(payload.name, style: theme.textTheme.bodySmall),
        ),
      ),
      child: Tooltip(message: 'Drag onto a $kind field', child: row),
    );
  }

  Widget _dependents(ThemeData theme, AssetEntry e, AssetDetail d) {
    final byDoc = <String, List<Map<String, Object?>>>{};
    for (final r in d.dependents) {
      byDoc.putIfAbsent(r['path'] as String? ?? '?', () => []).add(r);
    }
    return _section(theme, 'Referenced by (${d.dependents.length})', [
      if (d.dependents.isEmpty)
        Text(
          'Nothing references this asset — it can be deleted without leaving a dangling reference.',
          key: const Key('asset.noDependents'),
          style: theme.textTheme.bodySmall,
        ),
      for (final doc in byDoc.entries) ...[
        Padding(
          padding: const EdgeInsets.only(top: 4),
          child: Text(doc.key, style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600)),
        ),
        for (final r in doc.value)
          InkWell(
            key: Key('asset.dependent.${doc.key}${r['pointer']}'),
            onTap: () => widget.onOpenReference(doc.key, r['pointer'] as String?),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(12, 2, 0, 2),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      '${r['pointer'] ?? ''}${r['sub'] is String && (r['sub'] as String).isNotEmpty ? '  #${r['sub']}' : ''}',
                      style: theme.textTheme.bodySmall?.copyWith(fontFamily: 'monospace'),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  Icon(Icons.arrow_forward, size: 12, color: theme.hintColor),
                ],
              ),
            ),
          ),
      ],
    ]);
  }
}
