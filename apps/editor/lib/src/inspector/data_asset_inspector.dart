// A data asset's fields on its Inspector page (ADR-0027 §7), Unity's
// ScriptableObject Inspector: the type's fields drawn from its committed
// schema (`schemas/data/<id>.schema.json`) with the field editors a
// script's card uses, each change one undo step saved at once
// ([AssetEdits]). During Play the shell applies each saved change to the
// running game's instance too.
import 'package:flutter/material.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show AssetDetail;

import '../project/editor_project.dart';
import '../project_browser/project_browser_model.dart';
import '../theme/tokens.dart';
import 'asset_edits.dart';
import 'field_editor.dart';
import 'field_spec.dart';
import 'inspector_chrome.dart';
import 'inspector_model.dart' show describeAssetRef, precheckField;

/// What an asset page's registered sections may use to edit their asset
/// (item 9c d): the page's [AssetEdits] and Project browser. Absent when
/// the page is read-only.
class AssetPageScope extends InheritedWidget {
  const AssetPageScope({super.key, required this.edits, required this.browser, required super.child});

  final AssetEdits edits;
  final ProjectBrowserModel browser;

  static AssetPageScope? maybeOf(BuildContext context) => context.dependOnInheritedWidgetOfExactType<AssetPageScope>();

  @override
  bool updateShouldNotify(AssetPageScope old) => old.edits != edits || old.browser != browser;
}

/// The `data` kind's sections: the editable fields under an
/// [AssetPageScope], the stored values read-only otherwise.
List<Widget> dataAssetSections(BuildContext context, AssetEntry e, AssetDetail d) => [
  DataAssetInspector(key: Key('dataAsset.${e.id}'), id: e.id, readOnly: d.fields),
];

class DataAssetInspector extends StatefulWidget {
  const DataAssetInspector({super.key, required this.id, this.readOnly});

  final String id;

  /// The document's fields when there is nothing to edit through.
  final Map<String, Object?>? readOnly;

  @override
  State<DataAssetInspector> createState() => _DataAssetInspectorState();
}

class _DataAssetInspectorState extends State<DataAssetInspector> {
  AssetEdits? _edits;

  /// A refused value per top-level field, until the next change.
  final Map<String, String> _errors = {};

  String get id => widget.id;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final edits = AssetPageScope.maybeOf(context)?.edits;
    if (edits == _edits) return;
    _release();
    _edits = edits?..addListener(_changed);
    edits?.shown = id;
  }

  @override
  void didUpdateWidget(covariant DataAssetInspector old) {
    super.didUpdateWidget(old);
    if (old.id != id) {
      if (_edits?.shown == old.id) _edits?.shown = null;
      _errors.clear();
    }
    _edits?.shown = id;
  }

  @override
  void dispose() {
    _release();
    super.dispose();
  }

  void _release() {
    final edits = _edits;
    if (edits == null) return;
    edits.removeListener(_changed);
    if (edits.shown == id) edits.shown = null;
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final scope = AssetPageScope.maybeOf(context);
    final edits = scope?.edits;
    final root = edits?.fields(id) ?? widget.readOnly;
    if (root == null) {
      return InspectorSection(
        title: 'Data',
        children: [InspectorValueRow('Error', edits?.lastError ?? 'The data asset could not be opened.')],
      );
    }
    final typeId = root['type'] as String? ?? '';
    final project = scope?.browser.project;
    final type = project?.dataTypes().where((t) => t.id == typeId).firstOrNull;
    final data = root['data'] is Map ? Map<String, Object?>.from(root['data'] as Map) : <String, Object?>{};
    final title = type?.title ?? (typeId.isEmpty ? 'Data' : typeId.split('.').last);
    if (edits == null || project == null || type?.schema == null) {
      return InspectorSection(
        title: title,
        children: [
          InspectorValueRow('Type', typeId, mono: true),
          if (project != null && type?.schema == null)
            InspectorValueRow('Schema', 'none — run fynkel codegen to generate schemas/data/$typeId.schema.json'),
          for (final f in data.entries) InspectorValueRow(humanize(f.key), '${f.value}', mono: true),
        ],
      );
    }
    final metadata = [
      for (final m in project.behaviourMetadata()?['dataTypes'] as List? ?? const [])
        if (m is Map && m['id'] == typeId) Map<String, Object?>.from(m),
    ].firstOrNull;
    final spec = parseComponentSpec(typeId, type!.schema, metadata: metadata);
    final target = _DataTarget(this, edits, scope!.browser, data);
    final rows = <Widget>[];
    String? group;
    for (final f in spec.fields) {
      if (!f.visibleWith(data, siblings: spec.fields)) continue;
      if (f.group != group) {
        group = f.group;
        if (group != null) {
          rows.add(Padding(padding: const EdgeInsets.only(top: 10, bottom: 3), child: InspectorCaption(group)));
        }
      }
      if (f.header != null) {
        rows.add(Padding(padding: const EdgeInsets.only(top: 10, bottom: 3), child: InspectorCaption(f.header!)));
      }
      rows.add(FieldEditor(key: ValueKey('field.${target.key}.${f.name}'), target: target, spec: f, path: f.name));
    }
    final t = context.tokens;
    return InspectorSection(
      title: title,
      children: [
        Padding(
          padding: EdgeInsets.fromLTRB(t.pad, 2, t.pad, 8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (rows.isEmpty)
                Text('No fields.', style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.textDisabled)),
              ...rows,
              if (edits.lastError != null && _errors.isEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: SelectableText(
                    edits.lastError!,
                    key: const Key('dataAsset.error'),
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Theme.of(context).colorScheme.error),
                  ),
                ),
            ],
          ),
        ),
      ],
    );
  }
}

/// A data asset's `data` as a [FieldEditor]'s target: one instance, each
/// write the whole top-level field at `/data/<field>`, one undo step.
final class _DataTarget extends FieldTarget {
  _DataTarget(this.state, this.edits, this.browser, this.data);

  final _DataAssetInspectorState state;
  final AssetEdits edits;
  final ProjectBrowserModel browser;
  final Map<String, Object?> data;

  static List<String> _segments(String path) => path.startsWith('/') ? path.substring(1).split('/') : [path];

  @override
  String get key => 'data.${state.id}';
  @override
  bool get canEdit => edits.canEdit;
  @override
  List<String> get instances => [state.id];

  @override
  Object? shown(String path) {
    Object? v = data;
    for (final s in _segments(path)) {
      final i = int.tryParse(s);
      v = switch (v) {
        final Map<Object?, Object?> m => m[s],
        final List<Object?> l when i != null && i >= 0 && i < l.length => l[i],
        _ => null,
      };
    }
    return v;
  }

  @override
  String? errorFor(String field) => state._errors[field];

  @override
  bool set(String path, Object? value, {FieldSpec? spec}) {
    final segments = _segments(path);
    final top = segments.first;
    if (spec != null) {
      final problem = precheckField(browser.project, spec, value);
      if (problem != null) {
        state._errors[top] = problem;
        state._changed();
        return false;
      }
    }
    state._errors.remove(top);
    final whole = segments.length == 1 ? value : _setIn(_copy(data[top]), segments.sublist(1), value);
    return edits.edit(state.id, {'/data/${_escape(top)}': whole});
  }

  static String _escape(String s) => s.replaceAll('~', '~0').replaceAll('/', '~1');

  /// [into] with [value] at [path] (an index one past a list's end appends;
  /// an index into nothing yet starts a list — a field the file leaves out).
  static Object? _setIn(Object? into, List<String> path, Object? value) {
    if (path.isEmpty) return value;
    final head = path.first, rest = path.sublist(1);
    if (into is List || (into == null && int.tryParse(head) != null)) {
      final list = [...?into as List?];
      final i = int.tryParse(head) ?? list.length;
      if (i >= list.length) {
        list.add(_setIn(null, rest, value));
      } else {
        list[i] = _setIn(list[i], rest, value);
      }
      return list;
    }
    final map = into is Map ? {for (final e in into.entries) e.key as String: e.value} : <String, Object?>{};
    map[head] = _setIn(map[head], rest, value);
    return map;
  }

  static Object? _copy(Object? v) => switch (v) {
    final List<Object?> l => [for (final x in l) _copy(x)],
    final Map<Object?, Object?> m => {for (final e in m.entries) e.key as String: _copy(e.value)},
    _ => v,
  };

  @override
  List<AssetEntry> assetsFor(FieldSpec spec) => assetsForSpec(browser.project.assets(), spec);
  @override
  String describeAsset(Map<String, Object?>? ref) => describeAssetRef(browser.project.assets(), ref);
  @override
  AssetPreview Function(AssetEntry entry)? get assetPreview => browser.preview;
  @override
  void Function(String assetId)? get pingAsset => browser.ping;
  @override
  List<String> layerNames() => browser.project.layerNames();
}
