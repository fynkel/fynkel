// The Inspector's pages for the Project browser's things (item 9c, "one
// Inspector" — what the Asset panel of item 5 showed, moved here):
//
// * an asset, as Unity shows one (docs/ui-refresh): the header — its
//   picture (the id in its tooltip, a click copies it), `name (Kind)`,
//   Open for a scene or prefab and a ⋮ menu (Reveal, Reimport, Copy,
//   Delete) — then only the asset itself: a source's import settings (edited here, written
//   through the session with Apply and Reimport on the bar pinned to the
//   bottom); a model's Materials (each embedded material remapped to a
//   project material, or Extract Materials); its parts (click one to
//   inspect it, drag it onto a field); a
//   material's factors; a prefab's root; and, folded away under Info, the
//   file's facts and every object that uses it (a click opens the document
//   at the field);
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
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../env/tools.dart';
import '../project/editor_project.dart';
import '../project_browser/asset_preview.dart';
import 'asset_edits.dart';
import 'component_specs.dart';
import 'field_spec.dart' show humanize;
import 'inspector_panel.dart' show componentIcon;
import 'data_asset_inspector.dart';
import 'material_inspector.dart';
import 'physics_material_inspector.dart';
import 'volume_inspector.dart';
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

/// The documents that reference an asset (or one part of it), grouped by
/// document, each use named by the object that holds it (`Coin 3 ·
/// MeshRenderer`); a row opens the document at the field.
class _UsedBy extends StatelessWidget {
  const _UsedBy({required this.dependents, required this.onOpenReference, required this.model, this.empty});

  final List<Map<String, Object?>> dependents;
  final void Function(String documentPath, String? pointer) onOpenReference;
  final ProjectBrowserModel model;
  final String? empty;

  /// A document's entities (`entities` of its JSON) by file, while the
  /// file is unchanged.
  static final _documents = <String, (DateTime, Map<String, Object?>)>{};

  Map<String, Object?> _entitiesOf(String documentPath) {
    try {
      final file = File(model.project.assetFile(documentPath));
      final stamp = file.lastModifiedSync();
      final cached = _documents[file.path];
      if (cached != null && cached.$1 == stamp) return cached.$2;
      final json = jsonDecode(file.readAsStringSync());
      final entities = json is Map && json['entities'] is Map
          ? Map<String, Object?>.from(json['entities'] as Map)
          : <String, Object?>{};
      _documents[file.path] = (stamp, entities);
      return entities;
    } on Object {
      return const {};
    }
  }

  /// What a person calls the use at [pointer]: the object's name and the
  /// component, or the field of a document with no objects.
  (String, String?) _describe(String documentPath, String pointer) {
    final parts = pointer.split('/').where((s) => s.isNotEmpty).toList();
    if (parts.length >= 2 && parts[0] == 'entities') {
      final entity = _entitiesOf(documentPath)[parts[1]];
      final name = entity is Map && entity['name'] is String ? entity['name'] as String : 'An object';
      String? what;
      if (parts.length >= 4 && parts[2] == 'components' && entity is Map && entity['components'] is Map) {
        final c = (entity['components'] as Map)[parts[3]];
        final type = c is Map ? c['type'] as String? : null;
        if (type != null) what = type.contains('.') ? type.substring(type.indexOf('.') + 1) : type;
      }
      return (name, what);
    }
    return (parts.isEmpty ? 'The document' : humanize(parts.last), null);
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final byDoc = <String, List<Map<String, Object?>>>{};
    for (final r in dependents) {
      byDoc.putIfAbsent(r['path'] as String? ?? '?', () => []).add(r);
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.only(top: 8, bottom: 2),
          child: InspectorCaption('Used by · ${dependents.length}'),
        ),
        if (dependents.isEmpty)
          Text(
            empty ?? 'Nothing uses this asset.',
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
                    style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
                  ),
                ),
              ],
            ),
          ),
          for (final r in doc.value)
            _HoverRow(
              key: Key('asset.dependent.${doc.key}${r['pointer']}'),
              onTap: () => onOpenReference(doc.key, r['pointer'] as String?),
              child: Builder(
                builder: (context) {
                  final (name, what) = _describe(doc.key, r['pointer'] as String? ?? '');
                  final sub = r['sub'] is String && (r['sub'] as String).isNotEmpty ? r['sub'] as String : null;
                  return Tooltip(
                    message: '${r['pointer'] ?? ''}${sub == null ? '' : '  › $sub'}',
                    waitDuration: const Duration(milliseconds: 600),
                    child: Row(
                      children: [
                        const SizedBox(width: 18),
                        Expanded(
                          child: Text.rich(
                            TextSpan(
                              text: name,
                              children: [
                                if (what != null || sub != null)
                                  TextSpan(
                                    text: '  ·  ${[?what, ?sub].join(' › ')}',
                                    style: TextStyle(color: t.textDisabled),
                                  ),
                              ],
                            ),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: theme.textTheme.bodySmall?.copyWith(color: t.text),
                          ),
                        ),
                        Icon(FIcons.arrowForward, size: 12, color: t.textDisabled),
                      ],
                    ),
                  );
                },
              ),
            ),
        ],
      ],
    );
  }

  static String? _documentKindOf(String path) {
    for (final k in ['scene', 'prefab', 'material', 'rig', 'blend', 'volume', 'physics_material', 'data']) {
      if (path.endsWith(documentSuffix(k))) return k;
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
  static final Map<String, AssetSectionBuilder> _byKind = {'material': _materialSections, 'data': dataAssetSections};

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

  // A scene shows nothing but its header (Unity's); a prefab's root is
  // drawn by the page; the object count is under Info.
  static List<Widget> _generic(BuildContext context, AssetEntry e, AssetDetail d) => [
    if (d.entities != null)
      ...const <Widget>[]
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
    this.assetEdits,
  });

  final ProjectBrowserModel model;
  final String id;

  /// Edits a `fynkel.pbr@1` material in place (its page's fields); the
  /// material's summary is read-only without it.
  final AssetEdits? assetEdits;

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
  NavigatorState? _navigator;

  @override
  void initState() {
    super.initState();
    model.addListener(_onModel);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _navigator = Navigator.maybeOf(context, rootNavigator: true);
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
    _leaving();
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
      _leaving();
      _draftFor = id;
      _draft = Map.of(d.settings);
      _json.text = _pretty(_draft);
      _jsonError = null;
    }
    return _draft;
  }

  /// Unity's "Unapplied import settings": the page leaving an asset whose
  /// settings were changed and not applied asks, once it has gone, to
  /// Apply (write the sidecar and reimport) or Revert.
  void _leaving() {
    final id = _draftFor;
    final nav = _navigator;
    if (id == null || nav == null || !model.canEdit || _jsonError != null) return;
    final d = model.detail(id);
    if (d == null || !_dirty(d)) return;
    final draft = Map.of(_draft);
    final path = model.entry(id)?.path;
    final name = path == null ? id : 'assets/$path';
    final m = model;
    _draftFor = null;
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      if (!nav.mounted) return;
      final apply = await showDialog<bool>(
        animationStyle: AnimationStyle.noAnimation,
        context: nav.context,
        builder: (context) => AlertDialog(
          key: const Key('asset.unapplied'),
          title: const Text('Unapplied import settings'),
          content: SizedBox(width: 420, child: Text("Unapplied import settings for '$name'")),
          actions: [
            TextButton(
              key: const Key('asset.unapplied.revert'),
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Revert'),
            ),
            FilledButton(
              key: const Key('asset.unapplied.apply'),
              autofocus: true,
              onPressed: () => Navigator.pop(context, true),
              child: const Text('Apply'),
            ),
          ],
        ),
      );
      if (apply == true && m.setImportSettings(id, draft) != null) m.reimport(id);
    });
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
    final editsMaterial = widget.assetEdits != null && d.material?['shader'] == pbrShader;
    final editsVolume = widget.assetEdits != null && e.document == 'volume';
    final editsPhysics = widget.assetEdits != null && e.document == 'physics_material';
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
                if (editsMaterial)
                  MaterialInspector(key: Key('material.${e.id}'), edits: widget.assetEdits!, browser: model, id: e.id),
                if (editsVolume) VolumeInspector(key: Key('volume.${e.id}'), edits: widget.assetEdits!, id: e.id),
                if (editsPhysics)
                  PhysicsMaterialInspector(key: Key('physicsMaterial.${e.id}'), edits: widget.assetEdits!, id: e.id),
                if (e.document == 'prefab') _PrefabRoot(model: model, entry: e, onOpen: () => widget.onOpenDocument(e.id)),
                if (draft != null) _importSettings(context, e, d, draft),
                if (draft != null && _isModel(e)) _materials(context, e, d, draft),
                if (e.isSource && d.subAssets.isNotEmpty) _parts(context, e, d),
                if (!editsMaterial && !editsVolume && !editsPhysics)
                  for (final section in AssetDocumentSections.of(context, e, d))
                    // A registered section edits through the page's AssetEdits.
                    if (widget.assetEdits case final edits?)
                      AssetPageScope(edits: edits, browser: model, child: section)
                    else
                      section,
                _info(context, e, d),
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

  /// `coin (Prefab)`, `swatch (Texture)`, `coin (Coin Kind)`: Unity's title.
  String _title(AssetEntry e, AssetDetail d) {
    final file = p.basename(e.path);
    final doc = e.document;
    final stem = doc != null && file.endsWith(documentSuffix(doc))
        ? file.substring(0, file.length - documentSuffix(doc).length)
        : p.basenameWithoutExtension(file);
    var kind = browserKindOf(e).split(' ').map(_capital).join(' ');
    if (doc == 'data') {
      final type = d.fields?['type'];
      if (type is String && type.isNotEmpty) {
        kind = model.project.dataTypes().where((x) => x.id == type).firstOrNull?.title ?? type.split('.').last;
      }
    }
    return '$stem ($kind)';
  }

  Widget _header(BuildContext context, AssetEntry e, AssetDetail d) {
    final t = context.tokens;
    final isDoc = e.document == 'scene' || e.document == 'prefab';
    return InspectorPageHeader(
      // The id is in the picture's tooltip; a click copies it (as an object's).
      leading: Tooltip(
        message: '${e.id}\nClick to copy the id',
        waitDuration: const Duration(milliseconds: 500),
        child: MouseRegion(
          cursor: SystemMouseCursors.click,
          child: GestureDetector(
            key: const Key('asset.copyId'),
            onTap: () => Clipboard.setData(ClipboardData(text: e.id)),
            child: inspectorPictureBox(context, AssetPreviewWidget(preview: model.preview(e), size: 40)),
          ),
        ),
      ),
      titleKey: const Key('asset.title'),
      title: Tooltip(
        message: 'assets/${e.path}',
        waitDuration: const Duration(milliseconds: 600),
        child: Text(_title(e, d), maxLines: 1, overflow: TextOverflow.ellipsis),
      ),
      actions: [
        if (isDoc)
          Padding(
            padding: const EdgeInsets.only(top: 1, right: 2),
            child: OutlinedButton(
              key: const Key('asset.open'),
              style: OutlinedButton.styleFrom(
                visualDensity: VisualDensity.compact,
                backgroundColor: t.header,
                padding: const EdgeInsets.symmetric(horizontal: 10),
                minimumSize: Size(0, t.row + 2),
                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
              ),
              onPressed: () => widget.onOpenDocument(e.id),
              child: const Text('Open'),
            ),
          ),
        _menu(context, e),
      ],
      selection: widget.selection,
    );
  }

  /// Unity's right-click entries for the asset, on the header's ⋮.
  Widget _menu(BuildContext context, AssetEntry e) {
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall;
    PopupMenuItem<String> item(String key, String label, {bool enabled = true}) => PopupMenuItem(
      key: Key(key),
      value: key,
      enabled: enabled,
      height: 26,
      child: Text(label, style: small?.copyWith(color: enabled ? t.text : t.textDisabled)),
    );
    return PopupMenuButton<String>(
      key: const Key('asset.menu'),
      popUpAnimationStyle: AnimationStyle.noAnimation,
      tooltip: 'More',
      padding: EdgeInsets.zero,
      constraints: const BoxConstraints(minWidth: 180),
      icon: Icon(FIcons.moreVert, size: 14, color: t.textSecondary),
      style: IconButton.styleFrom(minimumSize: const Size(24, 24), fixedSize: const Size(24, 24)),
      onSelected: (key) => switch (key) {
        'asset.reveal' => _reveal(context, model, e.path, widget.run),
        'asset.reimport' => _runReimport(e.id),
        'asset.copyPath' => Clipboard.setData(ClipboardData(text: 'assets/${e.path}')),
        'asset.copyIdItem' => Clipboard.setData(ClipboardData(text: e.id)),
        'asset.delete' => model.delete(ids: [e.id]),
        _ => null,
      },
      itemBuilder: (_) => [
        item('asset.reveal', 'Reveal in Finder'),
        if (e.isSource) item('asset.reimport', 'Reimport', enabled: model.canEdit),
        const PopupMenuDivider(),
        item('asset.copyPath', 'Copy Path'),
        item('asset.copyIdItem', 'Copy ID'),
        const PopupMenuDivider(),
        item('asset.delete', 'Delete', enabled: model.canEdit),
      ],
    );
  }

  /// The file's facts and its uses, folded away (Unity keeps them off the
  /// page; they are here for when they are wanted).
  Widget _info(BuildContext context, AssetEntry e, AssetDetail d) => InspectorSection(
    title: 'Info',
    initiallyOpen: false,
    children: [
      InspectorValueRow('Path', 'assets/${e.path}', mono: true),
      if (d.size != null) InspectorValueRow('Size', formatBytes(d.size!)),
      if (d.modified != null) InspectorValueRow('Modified', _date(d.modified!)),
      if (d.entities != null) InspectorValueRow('Objects', '${d.entities}'),
      if (e.isSource) ...[
        InspectorValueRow(
          'Importer',
          '${d.importer ?? e.importer}${d.importerVersion == null ? '' : ' v${d.importerVersion}'}',
        ),
        InspectorValueRow(
          'Sidecar',
          d.sidecarPath == null ? '${p.basename(e.path)}.meta.json' : p.basename(d.sidecarPath!),
          mono: true,
        ),
      ],
      _UsedBy(dependents: d.dependents, onOpenReference: widget.onOpenReference, model: model),
    ],
  );

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
    final controls = <Widget>[];
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
        final calculate = draft['normals'] == 'calculate';
        controls.addAll([
          NumberField(
            key: const Key('asset.settings.scaleFactor'),
            label: 'Scale Factor',
            tooltip: 'Baked into the model: 0.01 brings a centimetre file to metres.',
            value: draft['scaleFactor'] is num ? draft['scaleFactor'] as num : 1,
            min: 0.0001,
            step: 0.01,
            enabled: enabled,
            onCommit: (v) => _set('scaleFactor', v == 1 ? null : v),
          ),
          FieldRow(
            label: 'Normals',
            child: _Choice(
              key: const Key('asset.settings.normals'),
              value: calculate ? 'calculate' : 'import',
              options: const {'import': 'Import', 'calculate': 'Calculate'},
              enabled: enabled,
              onChanged: (s) {
                _set('normals', s == 'import' ? null : s);
                if (s == 'import') _set('smoothingAngle', null);
              },
            ),
          ),
          if (calculate)
            NumberField(
              key: const Key('asset.settings.smoothingAngle'),
              label: 'Smoothing Angle',
              tooltip: 'Edges sharper than this stay hard; gentler ones are smoothed.',
              value: draft['smoothingAngle'] is num ? draft['smoothingAngle'] as num : 60,
              min: 0,
              max: 180,
              step: 1,
              slider: true,
              enabled: enabled,
              onCommit: (v) => _set('smoothingAngle', v == 60 ? null : v),
            ),
          FieldRow(
            label: 'Import Animation',
            child: BoolField(
              key: const Key('asset.settings.importAnimation'),
              value: draft['importAnimation'] != false,
              enabled: enabled,
              onCommit: (b) => _set('importAnimation', b ? null : false),
            ),
          ),
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
      title: 'Import Settings',
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

  static bool _isModel(AssetEntry e) => e.importer == 'fynkel.gltf' || e.importer == 'fynkel.fbx';

  /// `settings.materials` of the draft: embedded key → `{ asset }`.
  Map<String, Object?> _remaps(Map<String, Object?> draft) {
    final m = draft['materials'];
    return m is Map ? {for (final e in m.entries) e.key as String: e.value} : {};
  }

  void _setRemap(String key, Map<String, Object?>? ref) {
    final table = _remaps(_draft);
    if (ref == null || ref['asset'] == null) {
      table.remove(key);
    } else {
      table[key] = {'asset': ref['asset']};
    }
    _set('materials', table.isEmpty ? null : table);
  }

  void _extract(AssetEntry e) {
    final r = model.extractMaterials(e.id);
    if (r == null) return;
    final n = (r.json['materials'] as List?)?.length ?? 0;
    Notices.maybeOf(context)?.show('Extracted $n material${n == 1 ? '' : 's'} to ${p.dirname(e.path) == '.' ? '' : '${p.dirname(e.path)}/'}Materials');
    _draftFor = null;
    _runReimport(e.id);
  }

  /// A model's embedded materials, one row each: the project material it
  /// is remapped to, or None (the embedded one). The rows edit the draft
  /// like every import setting; Extract Materials writes editable copies
  /// and remaps them in one undoable step.
  Widget _materials(BuildContext context, AssetEntry e, AssetDetail d, Map<String, Object?> draft) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final keys = [for (final k in d.subAssets.keys) if (k.startsWith('material/')) k];
    final remaps = _remaps(draft);
    String describe(Map<String, Object?>? ref) {
      if (ref == null || ref['asset'] == null) return 'None (embedded)';
      final id = ref['asset'] as String;
      final target = model.entry(id);
      return target == null ? id : target.name;
    }

    List<AssetEntry> candidates() => [
      for (final a in model.entries)
        if (!a.isSource && a.document == 'material') a,
    ];
    final unmapped = keys.where((k) => !remaps.containsKey(k)).length;
    return InspectorSection(
      title: 'Materials',
      children: [
        if (keys.isEmpty)
          Text(
            'The file has no materials.',
            style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
          )
        else ...[
          for (final k in keys)
            FieldRow(
              label: k.substring('material/'.length),
              child: AssetField(
                key: Key('asset.materials.$k'),
                value: switch (remaps[k]) {
                  final Map<Object?, Object?> m => {for (final x in m.entries) x.key as String: x.value},
                  _ => null,
                },
                kind: 'material',
                nullable: true,
                subject: k,
                enabled: model.canEdit,
                describe: describe,
                candidates: candidates,
                thumbnail: (a, size) => AssetPreviewWidget(preview: model.preview(a), size: size),
                onCommit: (ref) => _setRemap(k, ref),
              ),
            ),
          _help(
            context,
            'None draws the material the file carries. A project material replaces it in every instance of the model; an instance\'s own override still wins.',
          ),
          Padding(
            padding: const EdgeInsets.only(top: 4),
            child: Align(
              alignment: Alignment.centerLeft,
              child: PageAction(
                key: const Key('asset.extractMaterials'),
                icon: FIcons.unarchiveOutlined,
                label: 'Extract Materials',
                onPressed: model.canEdit && unmapped > 0 && !_dirty(d) ? () => _extract(e) : null,
              ),
            ),
          ),
          _help(
            context,
            unmapped == 0
                ? 'Every material is remapped.'
                : 'Writes the $unmapped embedded material${unmapped == 1 ? '' : 's'} as editable documents in Materials/ next to the model and remaps ${unmapped == 1 ? 'it' : 'them'}.',
          ),
        ],
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

/// A prefab asset's root object, as Unity shows it on the asset's page:
/// its name, then its components (open the prefab to edit them).
class _PrefabRoot extends StatelessWidget {
  const _PrefabRoot({required this.model, required this.entry, required this.onOpen});

  final ProjectBrowserModel model;
  final AssetEntry entry;
  final VoidCallback onOpen;

  /// The root entity of the prefab file, or null when it cannot be read.
  Map<String, Object?>? _root() {
    try {
      final json = jsonDecode(File(model.project.assetFile(entry.path)).readAsStringSync());
      final entities = json is Map && json['entities'] is Map ? json['entities'] as Map : const {};
      for (final e in entities.values) {
        if (e is Map && e['parent'] == null) return Map<String, Object?>.from(e);
      }
    } on Object {
      return null;
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final root = _root();
    if (root == null) return const SizedBox.shrink();
    final specs = ComponentSpecs(model.project);
    final components = [
      for (final c in (root['components'] is Map ? root['components'] as Map : const {}).values)
        if (c is Map && c['type'] is String) specs.of(c['type'] as String),
    ];
    return Column(
      key: const Key('asset.prefabRoot'),
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          margin: const EdgeInsets.only(top: 8),
          padding: EdgeInsets.fromLTRB(t.pad + 2, 5, t.pad + 2, 5),
          decoration: BoxDecoration(
            color: t.header,
            border: Border(top: t.hairline, bottom: t.hairline),
          ),
          child: Text.rich(
            TextSpan(
              text: 'Root in Prefab Asset ',
              children: [
                TextSpan(
                  text: '(Open for full editing)',
                  style: TextStyle(color: t.textDisabled),
                ),
              ],
            ),
            style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
          ),
        ),
        Padding(
          padding: EdgeInsets.fromLTRB(t.pad + 2, 8, t.pad + 2, 6),
          child: Row(
            children: [
              Icon(FIcons.viewInArOutlined, size: 16, color: t.prefab),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  root['name'] as String? ?? entry.name,
                  key: const Key('asset.prefabRoot.name'),
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodySmall?.copyWith(color: t.text, fontWeight: FontWeight.w600),
                ),
              ),
            ],
          ),
        ),
        // Each component as its header band; the fields are the prefab's
        // to edit in Prefab mode (a double-click opens it).
        for (final spec in components)
          GestureDetector(
            onDoubleTap: onOpen,
            child: Container(
              key: ValueKey('asset.prefabRoot.${spec.type}'),
              height: t.row + 6,
              decoration: BoxDecoration(
                color: t.header,
                border: Border(top: t.hairline),
              ),
              padding: EdgeInsets.only(left: t.pad + 2, right: t.pad),
              child: Row(
                children: [
                  Icon(componentIcon(spec), size: 14, color: t.textSecondary),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      spec.title,
                      overflow: TextOverflow.ellipsis,
                      style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600, color: t.text),
                    ),
                  ),
                ],
              ),
            ),
          ),
        if (components.isNotEmpty) Container(height: 1, color: t.line),
      ],
    );
  }
}

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
                leading: inspectorPictureBox(context, Icon(assetKindIcon(kind), size: 20, color: t.textSecondary)),
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
                    InspectorSection(
                      title: 'Uses',
                      children: [
                        _UsedBy(
                          dependents: uses,
                          onOpenReference: onOpenReference,
                          model: model,
                          empty: 'Nothing uses this part.',
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
                leading: inspectorPictureBox(context, Icon(FIcons.folderOutlined, size: 20, color: t.textSecondary)),
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
                      title: 'Inside, by kind',
                      children: [
                        InspectorValueRow('Here', '$direct asset${direct == 1 ? '' : 's'}'),
                        InspectorValueRow('All inside', '${inside.length}'),
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
