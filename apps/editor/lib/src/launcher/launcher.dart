// The launcher (§22.3): New project through `fynkel create`, Open, Recent,
// and the doctor-style checks — what the editor shows until a project is open.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:path/path.dart' as p;

import '../env/tools.dart';
import '../prefs/preferences.dart';
import '../project/editor_project.dart';
import '../theme/icons.dart';
import '../theme/tokens.dart';

/// Picks a directory; null when the user cancels. Injected so tests never open a dialog.
typedef ChooseDirectory = Future<String?> Function(BuildContext context, {String? title, String? initialDirectory});

class LauncherScreen extends StatefulWidget {
  const LauncherScreen({
    super.key,
    required this.prefs,
    required this.toolchain,
    required this.chooseDirectory,
    required this.onOpen,
    required this.onRefreshChecks,
    required this.onPreferences,
    this.run = runProcess,
    this.version = '0.1.0',
  });

  final Preferences prefs;

  /// The current checks (null while detection runs).
  final Toolchain? toolchain;
  final ChooseDirectory chooseDirectory;

  /// Opens a project directory (the shell validates it first).
  final Future<void> Function(String dir) onOpen;
  final VoidCallback onRefreshChecks;
  final VoidCallback onPreferences;
  final RunProcess run;
  final String version;

  @override
  State<LauncherScreen> createState() => _LauncherScreenState();
}

class _LauncherScreenState extends State<LauncherScreen> {
  String? _error;

  Future<void> _open() async {
    final dir = await widget.chooseDirectory(context, title: 'Open a Fynkel project');
    if (dir == null || !mounted) return;
    await _openDir(dir);
  }

  Future<void> _openDir(String dir) async {
    if (!isFynkelProject(dir)) {
      setState(() => _error = '$dir is not a Fynkel project (no fynkel.project.json).');
      return;
    }
    setState(() => _error = null);
    await widget.onOpen(dir);
  }

  Future<void> _newProject() async {
    final tc = widget.toolchain;
    if (tc == null || !tc.canCreate) {
      setState(
        () => _error = 'New Project needs the Fynkel CLI and checkout — see the checks on the right (Preferences sets their paths).',
      );
      return;
    }
    final created = await showDialog<String>(
      animationStyle: AnimationStyle.noAnimation,
      context: context,
      builder: (_) => NewProjectDialog(toolchain: tc, chooseDirectory: widget.chooseDirectory, run: widget.run),
    );
    if (created != null && mounted) await _openDir(created);
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ListenableBuilder(
      listenable: widget.prefs,
      builder: (context, _) {
        final recents = widget.prefs.recents;
        return Scaffold(
          body: Row(
            children: [
              Expanded(
                flex: 3,
                child: Padding(
                  padding: const EdgeInsets.all(32),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text('Fynkel Editor', style: theme.textTheme.headlineMedium),
                      Text('version ${widget.version}', style: theme.textTheme.bodySmall),
                      const SizedBox(height: 24),
                      Wrap(
                        spacing: 12,
                        runSpacing: 8,
                        children: [
                          FilledButton.icon(
                            key: const Key('launcher.new'),
                            onPressed: _newProject,
                            icon: const Icon(FIcons.add),
                            label: const Text('New Project…'),
                          ),
                          OutlinedButton.icon(
                            key: const Key('launcher.open'),
                            onPressed: _open,
                            icon: const Icon(FIcons.folderOpen),
                            label: const Text('Open Project…'),
                          ),
                          TextButton.icon(
                            key: const Key('launcher.preferences'),
                            onPressed: widget.onPreferences,
                            icon: const Icon(FIcons.settings),
                            label: const Text('Preferences…'),
                          ),
                        ],
                      ),
                      if (_error != null) ...[
                        const SizedBox(height: 12),
                        Text(
                          _error!,
                          key: const Key('launcher.error'),
                          style: TextStyle(color: theme.colorScheme.error),
                        ),
                      ],
                      const SizedBox(height: 24),
                      Text('Recent projects', style: theme.textTheme.titleMedium),
                      const SizedBox(height: 8),
                      Expanded(
                        child: recents.isEmpty
                            ? Text('None yet.', style: theme.textTheme.bodyMedium?.copyWith(color: theme.disabledColor))
                            : ListView(
                                children: [
                                  for (final r in recents)
                                    _RecentTile(
                                      recent: r,
                                      onOpen: () => _openDir(r.dir),
                                      onRemove: () => widget.prefs.removeRecent(r.dir),
                                    ),
                                ],
                              ),
                      ),
                    ],
                  ),
                ),
              ),
              const VerticalDivider(width: 1),
              Expanded(
                flex: 2,
                child: _ChecksPanel(toolchain: widget.toolchain, onRefresh: widget.onRefreshChecks),
              ),
            ],
          ),
        );
      },
    );
  }
}

class _RecentTile extends StatelessWidget {
  const _RecentTile({required this.recent, required this.onOpen, required this.onRemove});

  final RecentProject recent;
  final VoidCallback onOpen;
  final VoidCallback onRemove;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final exists = recent.exists;
    return ListTile(
      key: Key('launcher.recent.${recent.dir}'),
      dense: true,
      enabled: exists,
      leading: Icon(exists ? FIcons.videogameAsset : FIcons.linkOff),
      title: Text(recent.name.isEmpty ? p.basename(recent.dir) : recent.name),
      subtitle: Text(
        exists ? recent.dir : '${recent.dir} — missing',
        style: theme.textTheme.bodySmall,
        maxLines: 1,
        overflow: TextOverflow.ellipsis, // a dense tile has one subtitle line; long paths must not overflow it
      ),
      trailing: IconButton(
        key: Key('launcher.recent.remove.${recent.dir}'),
        tooltip: 'Remove from recent projects',
        icon: const Icon(FIcons.close, size: 16),
        onPressed: onRemove,
      ),
      onTap: exists ? onOpen : null,
    );
  }
}

class _ChecksPanel extends StatelessWidget {
  const _ChecksPanel({required this.toolchain, required this.onRefresh});

  final Toolchain? toolchain;
  final VoidCallback onRefresh;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final tc = toolchain;
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text('Checks', style: theme.textTheme.titleMedium),
              const Spacer(),
              IconButton(
                key: const Key('launcher.checks.refresh'),
                tooltip: 'Run the checks again',
                icon: const Icon(FIcons.refresh, size: 18),
                onPressed: onRefresh,
              ),
            ],
          ),
          const SizedBox(height: 8),
          if (tc == null)
            const Row(
              children: [
                SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2)),
                SizedBox(width: 8),
                Expanded(child: Text('Looking for the Fynkel CLI, Flutter and the runtime…')),
              ],
            )
          else
            Expanded(
              child: ListView(
                children: [
                  // Not a ListTile: a tile's height is fixed by isThreeLine, and a
                  // path plus its fix wraps to more lines than that in a narrow window.
                  for (final c in tc.checks)
                    Padding(
                      key: Key('launcher.check.${c.title}'),
                      padding: const EdgeInsets.symmetric(vertical: 6),
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Icon(
                            c.ok
                                ? FIcons.checkCircle
                                : c.warning
                                ? FIcons.warningAmber
                                : FIcons.cancel,
                            size: 20,
                            color: c.ok
                                ? theme.tokens.ok
                                : c.warning
                                ? theme.tokens.warning
                                : theme.tokens.error,
                          ),
                          const SizedBox(width: 12),
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(c.title, style: theme.textTheme.bodyMedium),
                                Text(c.detail, style: theme.textTheme.bodySmall),
                                if (c.fix != null)
                                  Text(
                                    c.fix!,
                                    style: theme.textTheme.bodySmall?.copyWith(
                                      color: theme.colorScheme.onSurfaceVariant,
                                    ),
                                  ),
                              ],
                            ),
                          ),
                        ],
                      ),
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

/// Runs `fynkel create` and returns the created directory.
class NewProjectDialog extends StatefulWidget {
  const NewProjectDialog({super.key, required this.toolchain, required this.chooseDirectory, this.run = runProcess});

  final Toolchain toolchain;
  final ChooseDirectory chooseDirectory;
  final RunProcess run;

  static final packageName = RegExp(r'^[a-z][a-z0-9_]*$');
  static final orgName = RegExp(r'^[a-z][a-z0-9]*(\.[a-z][a-z0-9_]*)+$');

  /// `My Game` → `my_game`.
  static String packageFromTitle(String title) {
    final s = title.trim().toLowerCase().replaceAll(RegExp(r'[^a-z0-9]+'), '_').replaceAll(RegExp(r'^_+|_+$'), '');
    return s.isEmpty ? '' : (RegExp(r'^[0-9]').hasMatch(s) ? 'game_$s' : s);
  }

  /// The exact command line, for the log and the tests.
  static List<String> arguments({
    required String dir,
    required String package,
    required String title,
    required String org,
    required String root,
    required bool flutter,
  }) => [
    'create',
    dir,
    '--name',
    package,
    '--title',
    title,
    '--org',
    org,
    '--root',
    root,
    if (!flutter) '--no-flutter',
    '--json',
  ];

  @override
  State<NewProjectDialog> createState() => _NewProjectDialogState();
}

class _NewProjectDialogState extends State<NewProjectDialog> {
  final _title = TextEditingController(text: 'My Game');
  final _package = TextEditingController(text: 'my_game');
  final _org = TextEditingController(text: 'com.example');
  final _parent = TextEditingController();
  var _packageEdited = false;
  var _flutter = true;
  var _running = false;
  String? _log;
  String? _error;

  @override
  void initState() {
    super.initState();
    _parent.text = Platform.environment['HOME'] ?? Directory.current.path;
    _flutter = widget.toolchain.flutter != null;
  }

  @override
  void dispose() {
    _title.dispose();
    _package.dispose();
    _org.dispose();
    _parent.dispose();
    super.dispose();
  }

  String get _dir => p.join(_parent.text.trim(), _package.text.trim());

  String? _validate() {
    if (_title.text.trim().isEmpty) return 'Give the game a title.';
    if (!NewProjectDialog.packageName.hasMatch(_package.text.trim())) {
      return 'The package name must be lower_snake_case starting with a letter.';
    }
    if (!NewProjectDialog.orgName.hasMatch(_org.text.trim())) {
      return 'The organisation must be a reverse domain (com.example).';
    }
    if (_parent.text.trim().isEmpty || !Directory(_parent.text.trim()).existsSync()) {
      return 'Pick an existing parent directory.';
    }
    if (Directory(_dir).existsSync() && Directory(_dir).listSync().isNotEmpty) return '$_dir exists and is not empty.';
    return null;
  }

  Future<void> _create() async {
    final problem = _validate();
    if (problem != null) {
      setState(() => _error = problem);
      return;
    }
    final tc = widget.toolchain;
    final args = NewProjectDialog.arguments(
      dir: _dir,
      package: _package.text.trim(),
      title: _title.text.trim(),
      org: _org.text.trim(),
      root: tc.fynkelRoot!,
      flutter: _flutter && tc.flutter != null,
    );
    setState(() {
      _running = true;
      _error = null;
      _log = '${p.basename(tc.fynkelCli!)} ${args.join(' ')}\n';
    });
    try {
      final r = await widget.run(tc.fynkelCli!, args, workingDirectory: _parent.text.trim());
      final out = '${r.stdout}'.trim();
      final err = '${r.stderr}'.trim();
      Map<String, Object?>? report;
      try {
        final decoded = jsonDecode(out);
        if (decoded is Map<String, Object?>) report = decoded;
      } catch (_) {}
      if (!mounted) return;
      if (r.exitCode == 0 && report != null && report['ok'] == true) {
        Navigator.of(context).pop(report['dir'] as String? ?? _dir);
        return;
      }
      final diags = report?['diagnostics'];
      final reason = diags is List && diags.isNotEmpty
          ? diags.map((d) => d is Map ? '${d['code']}: ${d['message']}' : '$d').join('\n')
          : (err.isNotEmpty ? err : out);
      setState(() {
        _running = false;
        _error = 'fynkel create failed (exit ${r.exitCode}).';
        _log = '$_log$reason';
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _running = false;
        _error = 'could not run the Fynkel CLI: $e';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final tc = widget.toolchain;
    return AlertDialog(
      title: const Text('New project'),
      content: SizedBox(
        width: 520,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            TextField(
              key: const Key('new.title'),
              controller: _title,
              decoration: const InputDecoration(
                labelText: 'Title',
                helperText: 'Shown in the window and the project file',
              ),
              onChanged: (v) {
                if (!_packageEdited) _package.text = NewProjectDialog.packageFromTitle(v);
                setState(() {});
              },
            ),
            TextField(
              key: const Key('new.package'),
              controller: _package,
              decoration: const InputDecoration(
                labelText: 'Package name',
                helperText: 'lower_snake_case; also the directory name',
              ),
              onChanged: (_) => setState(() => _packageEdited = true),
            ),
            TextField(
              key: const Key('new.org'),
              controller: _org,
              decoration: const InputDecoration(
                labelText: 'Organisation',
                helperText: 'Reverse domain, e.g. com.example',
              ),
              onChanged: (_) => setState(() {}),
            ),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    key: const Key('new.parent'),
                    controller: _parent,
                    decoration: const InputDecoration(labelText: 'Create in'),
                    onChanged: (_) => setState(() {}),
                  ),
                ),
                const SizedBox(width: 8),
                TextButton(
                  key: const Key('new.choose'),
                  onPressed: () async {
                    final d = await widget.chooseDirectory(
                      context,
                      title: 'Create the project in…',
                      initialDirectory: _parent.text,
                    );
                    if (d != null) setState(() => _parent.text = d);
                  },
                  child: const Text('Choose…'),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text('→ $_dir', style: theme.textTheme.bodySmall),
            CheckboxListTile(
              key: const Key('new.flutter'),
              dense: true,
              contentPadding: EdgeInsets.zero,
              value: _flutter && tc.flutter != null,
              onChanged: tc.flutter == null ? null : (v) => setState(() => _flutter = v ?? true),
              title: const Text('Flutter platform runners (macOS, iOS)'),
              subtitle: Text(
                tc.flutter == null
                    ? 'Flutter was not found; the project is created without runners.'
                    : 'Runs `flutter create` inside the project.',
                style: theme.textTheme.bodySmall,
              ),
            ),
            if (_error != null)
              Text(
                _error!,
                key: const Key('new.error'),
                style: TextStyle(color: theme.colorScheme.error),
              ),
            if (_log != null)
              Container(
                margin: const EdgeInsets.only(top: 8),
                padding: const EdgeInsets.all(8),
                constraints: const BoxConstraints(maxHeight: 140),
                decoration: BoxDecoration(
                  color: theme.colorScheme.surfaceContainerHighest,
                  borderRadius: BorderRadius.circular(4),
                ),
                child: SingleChildScrollView(
                  child: SelectableText(
                    _log!,
                    key: const Key('new.log'),
                    style: theme.textTheme.bodySmall?.copyWith(fontFamily: 'Menlo'),
                  ),
                ),
              ),
          ],
        ),
      ),
      actions: [
        TextButton(onPressed: _running ? null : () => Navigator.of(context).pop(), child: const Text('Cancel')),
        FilledButton(
          key: const Key('new.create'),
          onPressed: _running ? null : _create,
          child: _running
              ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
              : const Text('Create'),
        ),
      ],
    );
  }
}
