// Preferences (⌘,): theme, density, the IDE command, the tool paths, the
// layout reset, the packages' editor extensions.
import 'package:flutter/material.dart';

import '../extensions/editor_extensions.dart';
import '../theme/tokens.dart';
import 'preferences.dart';

Future<void> showPreferencesDialog(
  BuildContext context, {
  required Preferences prefs,
  required VoidCallback onResetLayout,
  required VoidCallback onToolsChanged,
  EditorExtensionHost? extensions,
}) {
  return showDialog<void>(
    animationStyle: AnimationStyle.noAnimation,
    context: context,
    builder: (_) => PreferencesDialog(
      prefs: prefs,
      onResetLayout: onResetLayout,
      onToolsChanged: onToolsChanged,
      extensions: extensions,
    ),
  );
}

class PreferencesDialog extends StatefulWidget {
  const PreferencesDialog({
    super.key,
    required this.prefs,
    required this.onResetLayout,
    required this.onToolsChanged,
    this.extensions,
  });

  final Preferences prefs;
  final VoidCallback onResetLayout;

  /// A tool path changed: the launcher's checks run again.
  final VoidCallback onToolsChanged;

  /// What the packages' editor parts registered (ADR-0028).
  final EditorExtensionHost? extensions;

  @override
  State<PreferencesDialog> createState() => _PreferencesDialogState();
}

class _PreferencesDialogState extends State<PreferencesDialog> {
  late final _ide = TextEditingController(text: widget.prefs.ideCommand);
  late final _cli = TextEditingController(text: widget.prefs.fynkelCli ?? '');
  late final _root = TextEditingController(text: widget.prefs.fynkelRoot ?? '');
  late final _flutter = TextEditingController(text: widget.prefs.flutterCommand);
  var _toolsTouched = false;

  @override
  void dispose() {
    _ide.dispose();
    _cli.dispose();
    _root.dispose();
    _flutter.dispose();
    super.dispose();
  }

  void _apply() {
    final prefs = widget.prefs;
    prefs.setIdeCommand(_ide.text);
    if ((prefs.fynkelCli ?? '') != _cli.text.trim()) {
      prefs.setFynkelCli(_cli.text);
      _toolsTouched = true;
    }
    if ((prefs.fynkelRoot ?? '') != _root.text.trim()) {
      prefs.setFynkelRoot(_root.text);
      _toolsTouched = true;
    }
    if (prefs.flutterCommand != _flutter.text.trim()) {
      prefs.setFlutterCommand(_flutter.text);
      _toolsTouched = true;
    }
    if (_toolsTouched) widget.onToolsChanged();
  }

  @override
  Widget build(BuildContext context) {
    final prefs = widget.prefs;
    final theme = Theme.of(context);
    return ListenableBuilder(
      listenable: prefs,
      builder: (context, _) => AlertDialog(
        title: const Text('Preferences'),
        content: SizedBox(
          width: 560,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Theme', style: theme.textTheme.titleSmall),
                const SizedBox(height: 4),
                SegmentedButton<EditorTheme>(
                  key: const Key('prefs.theme'),
                  segments: const [
                    ButtonSegment(value: EditorTheme.system, label: Text('System')),
                    ButtonSegment(value: EditorTheme.light, label: Text('Light')),
                    ButtonSegment(value: EditorTheme.dark, label: Text('Dark')),
                  ],
                  selected: {prefs.theme},
                  onSelectionChanged: (s) => prefs.setTheme(s.first),
                ),
                const SizedBox(height: 16),
                Text('Density', style: theme.textTheme.titleSmall),
                Text('How tall every row, field and toolbar is.', style: theme.textTheme.bodySmall),
                const SizedBox(height: 4),
                SegmentedButton<EditorDensity>(
                  key: const Key('prefs.density'),
                  segments: [for (final d in EditorDensity.values) ButtonSegment(value: d, label: Text(d.label))],
                  selected: {prefs.density},
                  onSelectionChanged: (s) => prefs.setDensity(s.first),
                ),
                const SizedBox(height: 16),
                Text('Scene view', style: theme.textTheme.titleSmall),
                CheckboxListTile(
                  key: const Key('prefs.zoomToPointer'),
                  contentPadding: EdgeInsets.zero,
                  dense: true,
                  controlAffinity: ListTileControlAffinity.leading,
                  title: const Text('Scroll and pinch zoom toward the pointer'),
                  subtitle: const Text('Off: toward the centre of the view.'),
                  value: prefs.zoomToPointer,
                  onChanged: (v) => prefs.setZoomToPointer(v ?? true),
                ),
                const SizedBox(height: 16),
                Text('Play', style: theme.textTheme.titleSmall),
                CheckboxListTile(
                  key: const Key('prefs.playModeTint'),
                  contentPadding: EdgeInsets.zero,
                  dense: true,
                  controlAffinity: ListTileControlAffinity.leading,
                  title: const Text('Tint the editor while the game plays'),
                  subtitle: const Text('A reminder that changes made to the running game are lost on Stop.'),
                  value: prefs.playModeTint,
                  onChanged: (v) => prefs.setPlayModeTint(v ?? true),
                ),
                const SizedBox(height: 16),
                Text('IDE command', style: theme.textTheme.titleSmall),
                Text(
                  'Console rows open here. {file}, {line} and {column} are substituted.',
                  style: theme.textTheme.bodySmall,
                ),
                const SizedBox(height: 4),
                Row(
                  children: [
                    Expanded(
                      child: TextField(
                        key: const Key('prefs.ide'),
                        controller: _ide,
                        decoration: const InputDecoration(isDense: true, border: OutlineInputBorder()),
                        onChanged: (_) => setState(() {}),
                      ),
                    ),
                    const SizedBox(width: 8),
                    DropdownButton<IdePreset>(
                      key: const Key('prefs.ide.preset'),
                      hint: const Text('Preset'),
                      value: IdePreset.matching(_ide.text),
                      items: [for (final x in IdePreset.presets) DropdownMenuItem(value: x, child: Text(x.name))],
                      onChanged: (x) {
                        if (x != null) setState(() => _ide.text = x.command);
                      },
                    ),
                  ],
                ),
                const SizedBox(height: 16),
                Text('Tools', style: theme.textTheme.titleSmall),
                Text(
                  'Leave a field empty to search PATH and the checkout that contains the editor.',
                  style: theme.textTheme.bodySmall,
                ),
                const SizedBox(height: 4),
                TextField(
                  key: const Key('prefs.cli'),
                  controller: _cli,
                  decoration: const InputDecoration(
                    isDense: true,
                    labelText: 'Fynkel CLI (path to `fynkel`)',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  key: const Key('prefs.root'),
                  controller: _root,
                  decoration: const InputDecoration(
                    isDense: true,
                    labelText: 'Fynkel checkout (for New Project)',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 8),
                TextField(
                  key: const Key('prefs.flutter'),
                  controller: _flutter,
                  decoration: const InputDecoration(
                    isDense: true,
                    labelText: 'Flutter command',
                    border: OutlineInputBorder(),
                  ),
                ),
                const SizedBox(height: 16),
                Text('Extensions', style: theme.textTheme.titleSmall),
                ExtensionsList(host: widget.extensions),
                const SizedBox(height: 16),
                Text('Workspace', style: theme.textTheme.titleSmall),
                const SizedBox(height: 4),
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    OutlinedButton(
                      key: const Key('prefs.resetLayout'),
                      onPressed: widget.onResetLayout,
                      child: const Text('Reset layout'),
                    ),
                    OutlinedButton(
                      key: const Key('prefs.clearRecents'),
                      onPressed: prefs.recents.isEmpty ? null : prefs.clearRecents,
                      child: const Text('Clear recent projects'),
                    ),
                  ],
                ),
                if (prefs.file != null) ...[
                  const SizedBox(height: 12),
                  Text(
                    'Stored in ${prefs.file!.path}',
                    style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
                  ),
                ],
              ],
            ),
          ),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel')),
          FilledButton(
            key: const Key('prefs.save'),
            onPressed: () {
              _apply();
              Navigator.of(context).pop();
            },
            child: const Text('Save'),
          ),
        ],
      ),
    );
  }
}

/// Preferences › Extensions: each package with an editor part and what it
/// registered, or why nothing (ADR-0028 §1).
class ExtensionsList extends StatelessWidget {
  const ExtensionsList({super.key, required this.host});

  final EditorExtensionHost? host;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final small = theme.textTheme.bodySmall;
    final packages = host?.packages ?? const <PackageExtensions>[];
    if (packages.isEmpty) {
      return Text(
        host == null || host!.parts.isEmpty
            ? 'No package has an editor part. A package adds one in lib/editor/editor.dart, '
                  'with void registerEditor(EditorExtensions editor).'
            : 'The editor parts have not registered yet.',
        key: const Key('prefs.extensions.none'),
        style: small,
      );
    }
    return Column(
      key: const Key('prefs.extensions'),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final p in packages)
          Padding(
            key: Key('prefs.extensions.${p.package}'),
            padding: const EdgeInsets.only(top: 4),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(p.package, style: theme.textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600)),
                if (p.error != null)
                  Text('registerEditor failed: ${p.error}', style: small?.copyWith(color: theme.colorScheme.error))
                else if (p.isEmpty)
                  Text('Registers nothing.', style: small)
                else
                  for (final item in p.menuItems) Text('Menu item: ${item.path}  (${item.id})', style: small),
                for (final w in p.windows) Text('Window: ${w.title}  (${w.id})', style: small),
                for (final c in p.contextItems)
                  Text(
                    '${switch (c.place) {
                      ContextMenuPlace.hierarchy => 'Hierarchy',
                      ContextMenuPlace.project => 'Project',
                      ContextMenuPlace.component => '${c.component} card',
                    }} menu: ${c.label}  (${c.id})',
                    style: small,
                  ),
                for (final i in p.inspectors) Text('Inspector: ${i.dartType}  (${i.typeId})', style: small),
                for (final d in p.drawers)
                  Text(d.record != null ? 'Drawer: ${d.record} records' : 'Drawer: "${d.name}"', style: small),
              ],
            ),
          ),
      ],
    );
  }
}
