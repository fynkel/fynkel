// The Inspector's page for a script, as Unity's for a MonoScript
// (docs/ui-refresh): whatever the user selects in the Project's Scripts
// shows here. The header says what it is (`player (Behaviour)`), with Open
// (the IDE) and a ⋮ menu; each behaviour the file declares shows its
// description and fields as the committed metadata has them; then the
// source, read-only; the file's facts fold away under Info. A scripts
// folder gets a page too, so a selection never leaves the Inspector behind.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path/path.dart' as p;

import '../env/tools.dart';
import '../project_browser/project_browser_model.dart';
import '../shell/editor_selection.dart';
import '../shell/notices.dart';
import '../theme/icons.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'field_spec.dart' show humanize;
import 'inspector_chrome.dart';

String _date(DateTime d) => d.toLocal().toString().split('.').first;

/// A `lib/` file's source shown on the page, at most this many lines.
const _maxLines = 3000;

class ScriptPage extends StatelessWidget {
  const ScriptPage({
    super.key,
    required this.model,
    required this.rel,
    this.folder = false,
    this.selection,
    this.onOpenScript,
    this.run = runProcess,
  });

  final ProjectBrowserModel model;

  /// Relative to `lib/`.
  final String rel;
  final bool folder;
  final EditorSelection? selection;

  /// Opens a file in the IDE; the error, if any.
  final Future<String?> Function(String file)? onOpenScript;
  final RunProcess run;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: model,
    builder: (context, _) => Material(
      type: MaterialType.transparency,
      child: folder ? _folderPage(context) : _scriptPage(context),
    ),
  );

  String get _path => model.scriptFile(rel);

  Future<void> _open(BuildContext context) async {
    final notices = Notices.maybeOf(context);
    final err = await onOpenScript?.call(_path);
    if (err != null) notices?.show(err);
  }

  Future<void> _reveal(BuildContext context) async {
    final notices = Notices.maybeOf(context);
    final err = await revealInFileBrowser(rel.isEmpty ? model.scriptsDir : _path, run: run);
    if (err != null) notices?.show('Reveal in Finder failed: $err');
  }

  Widget _menu(BuildContext context) {
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall?.copyWith(color: t.text);
    return PopupMenuButton<String>(
      key: const Key('script.menu'),
      popUpAnimationStyle: AnimationStyle.noAnimation,
      tooltip: 'More',
      padding: EdgeInsets.zero,
      constraints: const BoxConstraints(minWidth: 180),
      icon: Icon(FIcons.moreVert, size: 14, color: t.textSecondary),
      style: IconButton.styleFrom(minimumSize: const Size(24, 24), fixedSize: const Size(24, 24)),
      onSelected: (key) => switch (key) {
        'script.reveal' => _reveal(context),
        'script.copyPath' => Clipboard.setData(ClipboardData(text: rel.isEmpty ? 'lib' : 'lib/$rel')),
        _ => null,
      },
      itemBuilder: (_) => [
        PopupMenuItem(
          key: const Key('script.reveal'),
          value: 'script.reveal',
          height: 26,
          child: Text('Reveal in Finder', style: small),
        ),
        PopupMenuItem(
          key: const Key('script.copyPath'),
          value: 'script.copyPath',
          height: 26,
          child: Text('Copy Path', style: small),
        ),
      ],
    );
  }

  Widget _header(BuildContext context, {required IconData icon, required String title, List<Widget> actions = const []}) {
    final t = context.tokens;
    return InspectorPageHeader(
      leading: inspectorPictureBox(context, Icon(icon, size: 20, color: t.textSecondary)),
      titleKey: const Key('script.title'),
      title: Tooltip(
        message: rel.isEmpty ? 'lib' : 'lib/$rel',
        waitDuration: const Duration(milliseconds: 600),
        child: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
      ),
      actions: [...actions, _menu(context)],
      selection: selection,
    );
  }

  /// The behaviours [rel] declares, from the committed metadata.
  List<Map<String, Object?>> _behaviours() {
    final ids = model.behaviourIds[rel] ?? const <String>[];
    return [
      for (final b in model.project.behaviourMetadata()?['behaviours'] as List? ?? const [])
        if (b is Map && ids.contains(b['id'])) Map<String, Object?>.from(b),
    ];
  }

  Widget _scriptPage(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final file = File(_path);
    final kind = switch (model.scriptKind(rel)) {
      'behaviour' => 'Behaviour',
      'generated' => 'Generated Script',
      _ => 'Script',
    };
    String? source;
    var truncated = false;
    int? size;
    DateTime? modified;
    try {
      final lines = const LineSplitter().convert(file.readAsStringSync());
      truncated = lines.length > _maxLines;
      source = (truncated ? lines.take(_maxLines) : lines).join('\n');
      size = file.lengthSync();
      modified = file.lastModifiedSync();
    } on FileSystemException {
      source = null;
    }
    final behaviours = _behaviours();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _header(
          context,
          icon: FIcons.descriptionOutlined,
          title: '${p.basenameWithoutExtension(rel)} ($kind)',
          actions: [
            if (onOpenScript != null)
              Padding(
                padding: const EdgeInsets.only(top: 1, right: 2),
                child: OutlinedButton(
                  key: const Key('script.open'),
                  style: OutlinedButton.styleFrom(
                    visualDensity: VisualDensity.compact,
                    backgroundColor: t.header,
                    padding: const EdgeInsets.symmetric(horizontal: 10),
                    minimumSize: Size(0, t.row + 2),
                    tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  ),
                  onPressed: () => _open(context),
                  child: const Text('Open'),
                ),
              ),
          ],
        ),
        Expanded(
          child: ListView(
            key: Key('script.page.$rel'),
            padding: const EdgeInsets.only(bottom: 16),
            children: [
              for (final b in behaviours) _behaviour(context, b),
              InspectorSection(
                title: 'Source',
                children: [
                  if (source == null)
                    Text(
                      'The file could not be read.',
                      style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                    )
                  else
                    SelectableText(
                      source,
                      key: const Key('script.source'),
                      style: monoStyle(context, color: t.text).copyWith(height: 1.45),
                    ),
                  if (truncated)
                    Padding(
                      padding: const EdgeInsets.only(top: 6),
                      child: Text(
                        'The first $_maxLines lines; open the file for the rest.',
                        style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                      ),
                    ),
                ],
              ),
              InspectorSection(
                title: 'Info',
                initiallyOpen: false,
                children: [
                  InspectorValueRow('Path', 'lib/$rel', mono: true),
                  if (size != null) InspectorValueRow('Size', formatBytes(size)),
                  if (modified != null) InspectorValueRow('Modified', _date(modified)),
                  for (final b in behaviours) InspectorValueRow('Behaviour', '${b['id']}', mono: true),
                ],
              ),
            ],
          ),
        ),
      ],
    );
  }

  /// One behaviour: what it says it does, and its fields (read-only; a
  /// field is set on the component, in the object's Inspector).
  Widget _behaviour(BuildContext context, Map<String, Object?> b) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final fields = [
      for (final f in b['fields'] as List? ?? const [])
        if (f is Map) Map<String, Object?>.from(f),
    ];
    final description = b['description'];
    final id = '${b['id']}';
    return InspectorSection(
      key: Key('script.behaviour.$id'),
      title: '${b['class'] ?? id.split('.').last}',
      children: [
        if (description is String && description.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Text(description, style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary)),
          ),
        if (fields.isEmpty)
          Text('No fields.', style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled))
        else
          for (final f in fields)
            Tooltip(
              message: f['description'] is String ? f['description'] as String : '',
              waitDuration: const Duration(milliseconds: 600),
              child: InspectorValueRow(
                humanize('${f['name']}'),
                _typeOf(f),
                key: Key('script.field.$id.${f['name']}'),
              ),
            ),
      ],
    );
  }

  /// A field's type as a person reads it: `Coin Kind (data asset), or none`.
  static String _typeOf(Map<String, Object?> f) {
    final type = '${f['type'] ?? '?'}';
    String last(Object? s) => humanize('$s'.split('.').last);
    var shown = switch (type) {
      'data' when f['data'] != null => '${last(f['data'])} (data asset)',
      'asset' when f['kind'] != null => '${last(f['kind'])} asset',
      'entity' || 'entityRef' => 'Object',
      _ => humanize(type),
    };
    if (f['default'] != null) shown = '$shown = ${f['default']}';
    if (f['nullable'] == true) shown = '$shown, or none';
    return shown;
  }

  Widget _folderPage(BuildContext context) {
    final prefix = rel.isEmpty ? '' : '$rel/';
    final files = [
      for (final f in model.scripts.files)
        if (f.startsWith(prefix)) f,
    ];
    final behaviours = files.where((f) => model.scriptKind(f) == 'behaviour').length;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _header(
          context,
          icon: rel.isEmpty ? FIcons.code : FIcons.folderOutlined,
          title: rel.isEmpty ? 'Scripts (lib)' : '${p.basename(rel)} (Folder)',
        ),
        Expanded(
          child: ListView(
            key: Key('script.folder.$rel'),
            padding: const EdgeInsets.only(bottom: 16),
            children: [
              InspectorSection(
                title: 'Inside',
                children: [
                  InspectorValueRow('Scripts', '${files.length}', key: const Key('script.folder.scripts')),
                  InspectorValueRow('Behaviours', '$behaviours'),
                  InspectorValueRow('Path', rel.isEmpty ? 'lib' : 'lib/$rel', mono: true),
                ],
              ),
            ],
          ),
        ),
      ],
    );
  }
}
