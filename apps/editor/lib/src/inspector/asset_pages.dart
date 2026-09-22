// The Inspector's pages for the Project browser's things (item 9c, "one
// Inspector" — what the Asset panel of item 5 showed, moved here):
//
// * an asset: its picture, what it is, the facts row, Open / Reveal /
//   Reimport / Delete; a source's import settings (edited here, written
//   through the session with Apply and Reimport on the bar pinned to the
//   bottom); its parts (click one to inspect it, drag it onto a field); a
//   material's factors; a document's summary; and every document that uses
//   it (a click opens the document at the field);
// * a part of a source (`mesh/0`, `anim/1`): what it is, the file it comes
//   from (a link back), its uuid, a drag handle and the uses of that part;
// * a folder: what is inside it by kind, Open and Reveal.
//
// What an asset page says about a document's own content — a material's
// factors, a scene's entity count — comes from [AssetDocumentSections],
// registered by document kind, so a new kind of document (the packages
// stream's `@DataAsset`) registers a section here instead of adding a
// panel or editing the page.
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../env/tools.dart';
import '../project/editor_project.dart';
import '../project_browser/asset_preview.dart';
import '../project_browser/project_browser_model.dart';
import '../shell/editor_selection.dart';
import '../shell/notices.dart';
import '../theme/icons.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'field_widgets.dart';
import 'inspector_chrome.dart';

String _date(double seconds) =>
    DateTime.fromMillisecondsSinceEpoch((seconds * 1000).round()).toLocal().toString().split('.').first;

String _value(Object? v) => v is Map || v is List ? jsonEncode(v) : '$v';

String _capital(String s) => s.isEmpty ? s : '${s[0].toUpperCase()}${s.substring(1)}';

Future<void> _reveal(BuildContext context, ProjectBrowserModel model, String rel, RunProcess run) async {
  final notices = Notices.maybeOf(context);
  final err = await revealInFileBrowser(rel.isEmpty ? model.project.assetsDir : model.project.assetFile(rel), run: run);
  if (err != null) notices?.show('Reveal in Finder failed: $err');
}

/// A square picture box for a page header.
Widget _pictureBox(BuildContext context, Widget child) {
  final t = context.tokens;
  return Container(
    width: 40,
    height: 40,
    decoration: BoxDecoration(
      color: t.field,
      border: Border.all(color: t.line),
      borderRadius: BorderRadius.circular(t.radiusPanel),
    ),
    clipBehavior: Clip.antiAlias,
    alignment: Alignment.center,
    child: child,
  );
}

Widget _idRow(BuildContext context, String id, {Key? copyKey, String tooltip = 'Copy id'}) {
  final t = context.tokens;
  return Row(
    children: [
      Flexible(
        child: Text(
          id,
          style: monoStyle(context, size: 10, color: t.textDisabled),
          overflow: TextOverflow.ellipsis,
        ),
      ),
      IconButton(
        key: copyKey,
        tooltip: tooltip,
        icon: const Icon(FIcons.copy, size: 11),
        color: t.textDisabled,
        padding: EdgeInsets.zero,
        constraints: const BoxConstraints.tightFor(width: 20, height: 18),
        onPressed: () => Clipboard.setData(ClipboardData(text: id)),
      ),
    ],
  );
}

/// The documents that reference an asset (or one part of it), grouped by
/// document; a row opens the document at the field.
class _UsedBy extends StatelessWidget {
  const _UsedBy({required this.dependents, required this.onOpenReference, this.empty});

  final List<Map<String, Object?>> dependents;
  final void Function(String documentPath, String? pointer) onOpenReference;
  final String? empty;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final byDoc = <String, List<Map<String, Object?>>>{};
    for (final r in dependents) {
      byDoc.putIfAbsent(r['path'] as String? ?? '?', () => []).add(r);
    }
    return InspectorSection(
      title: 'Used by · ${dependents.length}',
      children: [
        if (dependents.isEmpty)
          Text(
            empty ?? 'Nothing uses this asset — it can be deleted without leaving a dangling reference.',
            key: const Key('asset.noDependents'),
            style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
          ),
        for (final doc in byDoc.entries) ...[
          Padding(
            padding: const EdgeInsets.only(top: 4, bottom: 1),
            child: Row(
              children: [
                Icon(assetKindIcon(_documentKindOf(doc.key)), size: 12, color: t.textSecondary),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    doc.key,
                    overflow: TextOverflow.ellipsis,
                    style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600, color: t.text),
                  ),
                ),
              ],
            ),
          ),
          for (final r in doc.value)
            _HoverRow(
              key: Key('asset.dependent.${doc.key}${r['pointer']}'),
              onTap: () => onOpenReference(doc.key, r['pointer'] as String?),
              child: Row(
                children: [
                  const SizedBox(width: 18),
                  Expanded(
                    child: Text(
                      '${r['pointer'] ?? ''}'
                      '${r['sub'] is String && (r['sub'] as String).isNotEmpty ? '  › ${r['sub']}' : ''}',
                      style: monoStyle(context, color: t.textSecondary),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  Icon(FIcons.arrowForward, size: 12, color: t.textDisabled),
                ],
              ),
            ),
        ],
      ],
    );
  }

  static String? _documentKindOf(String path) {
    for (final k in ['scene', 'prefab', 'material', 'rig', 'blend', 'volume']) {
      if (path.endsWith('.$k.json')) return k;
    }
    return null;
  }
}

/// A clickable row that lights up under the pointer.
class _HoverRow extends StatefulWidget {
  const _HoverRow({super.key, required this.child, this.onTap});

  final Widget child;
  final VoidCallback? onTap;

  @override
  State<_HoverRow> createState() => _HoverRowState();
}

class _HoverRowState extends State<_HoverRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return MouseRegion(
      cursor: widget.onTap == null ? MouseCursor.defer : SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          constraints: BoxConstraints(minHeight: t.row + 4),
          padding: const EdgeInsets.symmetric(horizontal: 4),
          decoration: BoxDecoration(color: _hover ? t.hover : null, borderRadius: t.cornerControl),
          alignment: Alignment.centerLeft,
          child: widget.child,
        ),
      ),
    );
  }
}

// ── Asset ──────────────────────────────────────────────────────────────────

/// Draws what an asset page shows about one document kind's content.
typedef AssetSectionBuilder = List<Widget> Function(BuildContext context, AssetEntry entry, AssetDetail detail);

/// The asset page's sections per document kind (item 9c d). A kind with
/// nothing registered shows the generic summary: the entity count of a
/// scene-like document, or the document's top-level fields.
abstract final class AssetDocumentSections {
  static final Map<String, AssetSectionBuilder> _byKind = {'material': _materialSections};

  /// Shows [builder]'s sections on the page of every document of [kind],
  /// in place of the generic summary. A second registration replaces the
  /// first.
  static void register(String kind, AssetSectionBuilder builder) => _byKind[kind] = builder;

  /// Takes [kind] back to the generic summary (a test's tear-down).
  static void unregister(String kind) => _byKind.remove(kind);

  /// The kinds that have their own sections.
  static Iterable<String> get kinds => _byKind.keys;

  static List<Widget> of(BuildContext context, AssetEntry e, AssetDetail d) {
    final builder = e.document == null ? null : _byKind[e.document];
    return builder != null ? builder(context, e, d) : _generic(context, e, d);
  }

  static List<Widget> _generic(BuildContext context, AssetEntry e, AssetDetail d) => [
    if (d.entities != null)
      InspectorSection(
        title: _capital(e.document ?? 'document'),
        children: [
          InspectorValueRow('Entities', '${d.entities}'),
          if (!e.isSource && e.name.isNotEmpty) InspectorValueRow('Name', e.name),
        ],
      )
    else if (d.fields != null && d.material == null)
      InspectorSection(
        title: _capital(e.document ?? 'document'),
        children: [
          for (final f in d.fields!.entries)
            if (f.key != 'id' && f.key != 'formatVersion' && f.key != 'kind') InspectorValueRow(f.key, _value(f.value)),
        ],
      ),
  ];

  static List<Widget> _materialSections(BuildContext context, AssetEntry e, AssetDetail d) {
    final m = d.material;
    if (m == null) return _generic(context, e, d);
    return [
      InspectorSection(
        title: 'Material',
        children: [
          InspectorValueRow('Shader', '${m['shader']}', mono: true),
          InspectorValueRow('Alpha', '${m['alphaMode']}${m['alphaMode'] == 'mask' ? ' at ${m['alphaCutoff']}' : ''}'),
          InspectorValueRow('Double-sided', m['doubleSided'] == true ? 'yes' : 'no'),
          if (m['fields'] is Map)
            for (final f in (m['fields'] as Map).entries) InspectorValueRow('${f.key}', _value(f.value), mono: true),
        ],
      ),
    ];
  }
}

class AssetPage extends StatefulWidget {
  const AssetPage({
    super.key,
    required this.model,
    required this.id,
    required this.onOpenDocument,
    required this.onOpenReference,
    this.selection,
    this.run = runProcess,
  });

  final ProjectBrowserModel model;
  final String id;

  /// A scene or prefab (or an imported prefab part, `id#prefab`) into a tab.
  final void Function(String id) onOpenDocument;

  /// A "Used by" row: the referencing document's asset-root-relative path
  /// and the field's JSON pointer.
  final void Function(String documentPath, String? pointer) onOpenReference;
  final EditorSelection? selection;
  final RunProcess run;

  @override
  State<AssetPage> createState() => _AssetPageState();
}

class _AssetPageState extends State<AssetPage> {
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
  void didUpdateWidget(covariant AssetPage old) {
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

  @override
  Widget build(BuildContext context) {
    final e = model.entry(widget.id);
    if (e == null) {
      return const InspectorEmpty(
        key: Key('asset.empty'),
        icon: FIcons.inventory2Outlined,
        title: 'This asset is gone',
        message: 'It left the asset database (deleted or moved outside the project).',
      );
    }
    final d = model.detail(e.id);
    if (d == null) {
      return InspectorEmpty(
        key: const Key('asset.empty'),
        icon: FIcons.inventory2Outlined,
        title: 'Nothing to show',
        message: '${e.path} could not be read.',
      );
    }
    final draft = e.isSource ? _draftOf(e.id, d) : null;
    final pending = draft != null && (_dirty(d) || _jsonError != null);
    return Material(
      type: MaterialType.transparency,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _header(context, e, d),
          Expanded(
            child: ListView(
              key: Key('asset.panel.${e.id}'),
              padding: const EdgeInsets.only(bottom: 16),
              children: [
                FactsRow([
                  ('Kind', browserKindOf(e)),
                  ('Size', d.size == null ? '—' : formatBytes(d.size!)),
                  if (e.isSource)
                    ('Parts', '${d.subAssets.length}')
                  else if (d.entities != null)
                    ('Entities', '${d.entities}')
                  else
                    ('Modified', d.modified == null ? '—' : _date(d.modified!).split(' ').first),
                ]),
                _actions(context, e),
                InspectorSection(
                  title: 'File',
                  children: [
                    InspectorValueRow('Path', 'assets/${e.path}', mono: true),
                    if (d.modified != null) InspectorValueRow('Modified', _date(d.modified!)),
                    if (e.isSource)
                      InspectorValueRow(
                        'Sidecar',
                        d.sidecarPath == null ? '${p.basename(e.path)}.meta.json' : p.basename(d.sidecarPath!),
                        mono: true,
                      ),
                  ],
                ),
                if (draft != null) _importSettings(context, e, d, draft),
                if (e.isSource) _parts(context, e, d),
                ...AssetDocumentSections.of(context, e, d),
                _UsedBy(dependents: d.dependents, onOpenReference: widget.onOpenReference),
              ],
            ),
          ),
          if (pending)
            ApplyBar(
              message: _jsonError != null
                  ? 'The settings JSON does not parse: $_jsonError'
                  : 'Import settings changed. Apply writes the sidecar and reimports.',
              applyLabel: 'Apply and Reimport',
              applyKey: const Key('asset.applySettings'),
              revertKey: const Key('asset.revertSettings'),
              onApply: model.canEdit && _dirty(d) ? () => _apply(d) : null,
              onRevert: () => setState(() => _draftFor = null),
            ),
        ],
      ),
    );
  }

  Widget _header(BuildContext context, AssetEntry e, AssetDetail d) {
    final kind = browserKindOf(e);
    return InspectorPageHeader(
      leading: _pictureBox(context, AssetPreviewWidget(preview: model.preview(e), size: 40)),
      titleKey: const Key('asset.title'),
      title: Text(p.basename(e.path)),
      subtitle: Text(
        e.isSource
            ? '$kind · imported by ${e.importer}${d.importerVersion == null ? '' : ' v${d.importerVersion}'}'
            : '$kind document${e.name.isNotEmpty && e.name != ProjectBrowserModel.editableName(e) ? ' · “${e.name}”' : ''}',
      ),
      below: _idRow(context, e.id, copyKey: const Key('asset.copyId')),
      selection: widget.selection,
    );
  }

  Widget _actions(BuildContext context, AssetEntry e) {
    final t = context.tokens;
    final isDoc = e.document == 'scene' || e.document == 'prefab';
    return Padding(
      padding: EdgeInsets.fromLTRB(t.pad + 2, 8, t.pad + 2, 0),
      child: Wrap(
        spacing: 6,
        runSpacing: 4,
        children: [
          if (isDoc)
            PageAction(
              key: const Key('asset.open'),
              primary: true,
              icon: FIcons.openInNew,
              label: 'Open',
              onPressed: () => widget.onOpenDocument(e.id),
            ),
          PageAction(
            key: const Key('asset.reveal'),
            icon: FIcons.folderOpen,
            label: 'Reveal in Finder',
            onPressed: () => _reveal(context, model, e.path, widget.run),
          ),
          if (e.isSource)
            PageAction(
              key: const Key('asset.reimport'),
              icon: FIcons.refresh,
              label: 'Reimport',
              onPressed: model.canEdit ? () => _runReimport(e.id) : null,
            ),
          PageAction(
            key: const Key('asset.delete'),
            icon: FIcons.deleteOutline,
            label: 'Delete',
            onPressed: model.canEdit ? () => model.delete(ids: [e.id]) : null,
          ),
        ],
      ),
    );
  }

  Widget _help(BuildContext context, String text) {
    final t = context.tokens;
    return LayoutBuilder(
      builder: (context, c) => Padding(
        padding: EdgeInsets.only(left: labelColumnWidth(c.maxWidth) + 10, bottom: 4),
        child: Text(text, style: Theme.of(context).textTheme.labelSmall?.copyWith(color: t.textDisabled)),
      ),
    );
  }

  Widget _importSettings(BuildContext context, AssetEntry e, AssetDetail d, Map<String, Object?> draft) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final enabled = model.canEdit;
    final controls = <Widget>[
      InspectorValueRow('Importer', '${d.importer ?? e.importer}  v${d.importerVersion ?? '?'}'),
    ];
    switch (e.importer) {
      case 'fynkel.image':
        final linear = draft['colorSpace'] == 'linear';
        controls.addAll([
          FieldRow(
            label: 'Colour space',
            child: _Choice(
              key: const Key('asset.settings.colorSpace'),
              value: linear ? 'linear' : 'srgb',
              options: const {'srgb': 'sRGB (colour)', 'linear': 'linear (data)'},
              enabled: enabled,
              onChanged: (s) => _set('colorSpace', s == 'srgb' ? null : s),
            ),
          ),
          _help(
            context,
            linear
                ? 'Data textures: normal maps, roughness, metallic, masks.'
                : 'Colour textures: base colour, emissive. The default.',
          ),
        ]);
      case 'fynkel.audio':
        controls.addAll([
          FieldRow(
            label: 'Stream from disk',
            child: BoolField(
              key: const Key('asset.settings.stream'),
              value: draft['stream'] == true,
              enabled: enabled,
              onCommit: (b) => _set('stream', b ? true : null),
            ),
          ),
          _help(context, 'Music and long sounds; short effects stay decoded in memory.'),
        ]);
      case 'fynkel.gltf' || 'fynkel.fbx':
        controls.addAll([
          FieldRow(
            label: 'External files',
            child: BoolField(
              key: const Key('asset.settings.allowExternalFiles'),
              value: draft['allowExternalFiles'] == true,
              enabled: enabled,
              onCommit: (b) => _set('allowExternalFiles', b ? true : null),
            ),
          ),
          _help(
            context,
            'Textures and buffers next to the file by plain relative paths; off, the file must be self-contained.',
          ),
        ]);
      default:
        controls.add(
          Text(
            '${e.importer} has no settings the editor knows; edit the JSON below.',
            style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
          ),
        );
    }
    return InspectorSection(
      title: 'Import settings',
      children: [
        ...controls,
        Theme(
          data: theme.copyWith(dividerColor: Colors.transparent),
          child: ExpansionTile(
            key: const Key('asset.settings.json'),
            title: Text('Settings as JSON', style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary)),
            tilePadding: EdgeInsets.zero,
            childrenPadding: const EdgeInsets.only(bottom: 6),
            dense: true,
            visualDensity: VisualDensity.compact,
            children: [
              TextField(
                key: const Key('asset.settings.jsonField'),
                controller: _json,
                maxLines: null,
                enabled: enabled,
                style: monoStyle(context, color: t.text),
                decoration: InputDecoration(isDense: true, errorText: _jsonError),
                onChanged: _jsonChanged,
              ),
            ],
          ),
        ),
        if (_reimport != null && _reimportFor == e.id) _reimportResult(context, _reimport!),
      ],
    );
  }

  Widget _reimportResult(BuildContext context, OperationResult r) {
    final t = context.tokens;
    final color = t.surfaceOf(r.ok ? t.ok : t.error, .2);
    final on = r.ok ? t.ok : t.error;
    final summary = r.json['summary'];
    final lines = <String>[];
    if (r.ok) {
      lines.add('Reimported${r.json['sidecarWritten'] == true ? ' — the sidecar gained new parts' : ''}.');
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
      decoration: BoxDecoration(color: color, borderRadius: t.cornerControl),
      child: SelectableText(lines.join('\n'), style: Theme.of(context).textTheme.bodySmall?.copyWith(color: on)),
    );
  }

  Widget _parts(BuildContext context, AssetEntry e, AssetDetail d) {
    final t = context.tokens;
    return InspectorSection(
      title: 'Parts · ${d.subAssets.length}',
      children: [
        if (d.subAssets.isEmpty)
          Text(
            'None — this file imports as one asset.',
            style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.textDisabled),
          ),
        for (final s in d.subAssets.entries) _partRow(context, e, s.key, s.value),
      ],
    );
  }

  Widget _partRow(BuildContext context, AssetEntry e, String sub, String uuid) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final kind = AssetEntry.subKind(sub);
    final payload = AssetDragPayload(id: e.id, sub: sub, kind: kind, name: '${p.basename(e.path)} › $sub');
    final row = _HoverRow(
      key: Key('asset.part.$sub'),
      onTap: widget.selection == null ? null : () => widget.selection!.inspect(InspectAsset(e.id, sub: sub)),
      child: Row(
        children: [
          Icon(assetKindIcon(kind), size: 13, color: t.textSecondary),
          const SizedBox(width: 6),
          Expanded(
            child: Text(sub, style: theme.textTheme.bodySmall?.copyWith(color: t.text)),
          ),
          Text(kind, style: theme.textTheme.labelSmall?.copyWith(color: t.textDisabled)),
          if (kind == 'prefab')
            IconButton(
              key: Key('asset.sub.open.$sub'),
              tooltip: 'Open the imported hierarchy in a tab (read-only; Prefab mode)',
              icon: const Icon(FIcons.openInNew, size: 12),
              color: t.textSecondary,
              padding: EdgeInsets.zero,
              constraints: const BoxConstraints.tightFor(width: 22, height: 20),
              onPressed: () => widget.onOpenDocument('${e.id}#$sub'),
            ),
          IconButton(
            tooltip: 'Copy the part’s uuid',
            icon: const Icon(FIcons.copy, size: 11),
            color: t.textDisabled,
            padding: EdgeInsets.zero,
            constraints: const BoxConstraints.tightFor(width: 22, height: 20),
            onPressed: () => Clipboard.setData(ClipboardData(text: uuid)),
          ),
        ],
      ),
    );
    return Draggable<AssetDragPayload>(
      key: Key('asset.sub.$sub'),
      data: payload,
      dragAnchorStrategy: pointerDragAnchorStrategy,
      feedback: _dragFeedback(context, kind, payload.name),
      child: Tooltip(
        message: 'Click to inspect · drag onto a $kind field',
        waitDuration: const Duration(seconds: 1),
        child: row,
      ),
    );
  }
}

Widget _dragFeedback(BuildContext context, String kind, String name) {
  final t = context.tokens;
  return Material(
    color: t.popup,
    elevation: 4,
    borderRadius: t.cornerControl,
    child: Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(assetKindIcon(kind), size: 13, color: t.textSecondary),
          const SizedBox(width: 6),
          Text(name, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.text)),
        ],
      ),
    ),
  );
}

/// A labelled choice in a field box (the colour space).
class _Choice extends StatelessWidget {
  const _Choice({super.key, required this.value, required this.options, required this.onChanged, this.enabled = true});

  final String value;
  final Map<String, String> options;
  final void Function(String value) onChanged;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    return SizedBox(
      height: t.fieldHeight,
      child: PopupMenuButton<String>(
        popUpAnimationStyle: AnimationStyle.noAnimation,
        enabled: enabled,
        tooltip: '',
        initialValue: value,
        onSelected: onChanged,
        itemBuilder: (_) => [
          for (final o in options.entries)
            PopupMenuItem(
              value: o.key,
              height: 26,
              child: Text(o.value, style: theme.textTheme.bodySmall),
            ),
        ],
        child: FieldBox(
          padding: const EdgeInsets.only(left: 6, right: 4),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  options[value] ?? value,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodySmall?.copyWith(color: t.text),
                ),
              ),
              Icon(FIcons.chevronDown, size: 12, color: t.textDisabled),
            ],
          ),
        ),
      ),
    );
  }
}

// ── A part of a source ─────────────────────────────────────────────────────

class SubAssetPage extends StatelessWidget {
  const SubAssetPage({
    super.key,
    required this.model,
    required this.id,
    required this.sub,
    required this.onOpenDocument,
    required this.onOpenReference,
    this.selection,
  });

  final ProjectBrowserModel model;
  final String id;
  final String sub;
  final void Function(String id) onOpenDocument;
  final void Function(String documentPath, String? pointer) onOpenReference;
  final EditorSelection? selection;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: model,
      builder: (context, _) {
        final t = context.tokens;
        final theme = Theme.of(context);
        final e = model.entry(id);
        final d = e == null ? null : model.detail(id);
        final uuid = d?.subAssets[sub] ?? e?.subAssets[sub];
        if (e == null || uuid == null) {
          return const InspectorEmpty(
            key: Key('asset.empty'),
            icon: FIcons.inventory2Outlined,
            title: 'This part is gone',
            message: 'The file was reimported without it, or deleted.',
          );
        }
        final kind = AssetEntry.subKind(sub);
        final file = p.basename(e.path);
        final payload = AssetDragPayload(id: e.id, sub: sub, kind: kind, name: '$file › $sub');
        final uses = [
          for (final r in d?.dependents ?? const <Map<String, Object?>>[])
            if (r['sub'] == sub) r,
        ];
        return Material(
          type: MaterialType.transparency,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InspectorPageHeader(
                leading: _pictureBox(context, Icon(assetKindIcon(kind), size: 20, color: t.textSecondary)),
                titleKey: const Key('asset.part.title'),
                title: Text(sub),
                subtitle: Text('$kind · a part of $file'),
                below: Align(
                  alignment: Alignment.centerLeft,
                  child: TextButton.icon(
                    key: const Key('asset.part.back'),
                    style: TextButton.styleFrom(
                      visualDensity: VisualDensity.compact,
                      padding: const EdgeInsets.symmetric(horizontal: 4),
                      minimumSize: const Size(0, 20),
                      textStyle: theme.textTheme.bodySmall,
                    ),
                    onPressed: selection == null ? null : () => selection!.inspect(InspectAsset(e.id)),
                    icon: const Icon(FIcons.arrowUpward, size: 12),
                    label: Text(file),
                  ),
                ),
                selection: selection,
              ),
              Expanded(
                child: ListView(
                  key: Key('asset.part.$id.$sub'),
                  padding: const EdgeInsets.only(bottom: 16),
                  children: [
                    FactsRow([('Kind', kind), ('Source', file), ('Importer', e.importer ?? '—')]),
                    Padding(
                      padding: EdgeInsets.fromLTRB(t.pad + 2, 8, t.pad + 2, 0),
                      child: Wrap(
                        spacing: 6,
                        runSpacing: 4,
                        children: [
                          if (kind == 'prefab')
                            PageAction(
                              key: const Key('asset.part.open'),
                              primary: true,
                              icon: FIcons.openInNew,
                              label: 'Open',
                              onPressed: () => onOpenDocument('${e.id}#$sub'),
                            ),
                          Draggable<AssetDragPayload>(
                            key: const Key('asset.part.drag'),
                            data: payload,
                            dragAnchorStrategy: pointerDragAnchorStrategy,
                            feedback: _dragFeedback(context, kind, payload.name),
                            child: PageAction(
                              icon: FIcons.openWith,
                              label: 'Drag onto a $kind field',
                              onPressed: () {},
                            ),
                          ),
                        ],
                      ),
                    ),
                    InspectorSection(
                      title: 'Identity',
                      children: [
                        InspectorValueRow('Part', sub, mono: true),
                        InspectorValueRow(
                          'Uuid',
                          uuid,
                          mono: true,
                          trailing: IconButton(
                            key: const Key('asset.part.copyUuid'),
                            tooltip: 'Copy the uuid',
                            icon: const Icon(FIcons.copy, size: 11),
                            color: t.textDisabled,
                            padding: EdgeInsets.zero,
                            constraints: const BoxConstraints.tightFor(width: 22, height: 20),
                            onPressed: () => Clipboard.setData(ClipboardData(text: uuid)),
                          ),
                        ),
                        InspectorValueRow('File', 'assets/${e.path}', mono: true),
                      ],
                    ),
                    _UsedBy(dependents: uses, onOpenReference: onOpenReference, empty: 'Nothing uses this part.'),
                  ],
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

// ── Folder ─────────────────────────────────────────────────────────────────

class FolderPage extends StatelessWidget {
  const FolderPage({super.key, required this.model, required this.rel, this.selection, this.run = runProcess});

  final ProjectBrowserModel model;

  /// Relative to the asset root; '' is the root.
  final String rel;
  final EditorSelection? selection;
  final RunProcess run;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: model,
      builder: (context, _) {
        final t = context.tokens;
        final theme = Theme.of(context);
        final prefix = rel.isEmpty ? '' : '$rel/';
        final inside = [
          for (final e in model.entries)
            if (e.path.startsWith(prefix)) e,
        ];
        final direct = inside.where((e) => !e.path.substring(prefix.length).contains('/')).length;
        final subfolders = [
          for (final f in model.folders)
            if (f.startsWith(prefix) && f != rel && !f.substring(prefix.length).contains('/')) f,
        ];
        final byKind = <String, int>{};
        for (final e in inside) {
          byKind.update(browserKindOf(e), (n) => n + 1, ifAbsent: () => 1);
        }
        final name = rel.isEmpty ? 'assets' : p.basename(rel);
        return Material(
          type: MaterialType.transparency,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InspectorPageHeader(
                leading: _pictureBox(context, Icon(FIcons.folderOutlined, size: 20, color: t.textSecondary)),
                titleKey: const Key('folder.title'),
                title: Text(name),
                subtitle: Text(rel.isEmpty ? 'the asset root' : 'folder'),
                selection: selection,
              ),
              Expanded(
                child: ListView(
                  key: Key('folder.page.$rel'),
                  padding: const EdgeInsets.only(bottom: 16),
                  children: [
                    FactsRow([
                      ('Assets', '$direct'),
                      ('Folders', '${subfolders.length}'),
                      ('All inside', '${inside.length}'),
                    ]),
                    Padding(
                      padding: EdgeInsets.fromLTRB(t.pad + 2, 8, t.pad + 2, 0),
                      child: Wrap(
                        spacing: 6,
                        runSpacing: 4,
                        children: [
                          PageAction(
                            key: const Key('folder.open'),
                            primary: true,
                            icon: FIcons.folderOpen,
                            label: 'Show in Project',
                            onPressed: () => model.setFolder(rel),
                          ),
                          PageAction(
                            key: const Key('folder.reveal'),
                            icon: FIcons.openInNew,
                            label: 'Reveal in Finder',
                            onPressed: () => _reveal(context, model, rel, run),
                          ),
                        ],
                      ),
                    ),
                    InspectorSection(
                      title: 'Folder',
                      children: [InspectorValueRow('Path', rel.isEmpty ? 'assets' : 'assets/$rel', mono: true)],
                    ),
                    InspectorSection(
                      title: 'Inside, by kind',
                      children: [
                        if (byKind.isEmpty)
                          Text(
                            'Empty.',
                            key: const Key('folder.empty'),
                            style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                          ),
                        for (final k in browserKinds)
                          if (byKind[k] != null) InspectorValueRow(k, '${byKind[k]}', key: Key('folder.kind.$k')),
                      ],
                    ),
                    if (subfolders.isNotEmpty)
                      InspectorSection(
                        title: 'Folders · ${subfolders.length}',
                        children: [
                          for (final f in subfolders)
                            _HoverRow(
                              key: Key('folder.sub.$f'),
                              onTap: selection == null ? null : () => selection!.inspect(InspectFolder(f)),
                              child: Row(
                                children: [
                                  Icon(FIcons.folderOutlined, size: 13, color: t.textSecondary),
                                  const SizedBox(width: 6),
                                  Expanded(
                                    child: Text(
                                      p.basename(f),
                                      style: theme.textTheme.bodySmall?.copyWith(color: t.text),
                                    ),
                                  ),
                                ],
                              ),
                            ),
                        ],
                      ),
                  ],
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}
