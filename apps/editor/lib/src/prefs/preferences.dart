// The editor's own preferences: the only file the editor writes outside a
// project (blueprint §19.2e "Files the editor reads or writes directly").
// One JSON document in the platform's application-support directory,
// replaced atomically on every change.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path/path.dart' as p;

import '../theme/tokens.dart';

enum EditorTheme {
  system,
  light,
  dark;

  static EditorTheme parse(Object? v) => values.firstWhere((t) => t.name == v, orElse: () => system);
}

/// A recently opened project, newest first in [Preferences.recents].
final class RecentProject {
  const RecentProject({required this.dir, required this.name, required this.lastOpened});

  RecentProject.fromJson(Map<String, Object?> json)
    : dir = json['dir'] as String? ?? '',
      name = json['name'] as String? ?? '',
      lastOpened = DateTime.tryParse(json['lastOpened'] as String? ?? '') ?? DateTime.fromMillisecondsSinceEpoch(0);

  final String dir;
  final String name;
  final DateTime lastOpened;

  bool get exists => File(p.join(dir, 'fynkel.project.json')).existsSync();

  Map<String, Object?> toJson() => {'dir': dir, 'name': name, 'lastOpened': lastOpened.toIso8601String()};
}

/// A named IDE command template; `{file}`, `{line}` and `{column}` are
/// substituted (line and column default to 1 when a diagnostic has none).
final class IdePreset {
  const IdePreset(this.name, this.command);

  final String name;
  final String command;

  static const presets = [
    IdePreset('Visual Studio Code', 'code -g {file}:{line}:{column}'),
    IdePreset('Cursor', 'cursor -g {file}:{line}:{column}'),
    IdePreset('Zed', 'zed {file}:{line}:{column}'),
    IdePreset('IntelliJ IDEA / Android Studio', 'idea --line {line} --column {column} {file}'),
  ];

  static IdePreset? matching(String command) => presets.where((x) => x.command == command).firstOrNull;
}

final class Preferences extends ChangeNotifier {
  Preferences._(this.file, Map<String, Object?> json)
    : theme = EditorTheme.parse(json['theme']),
      density = EditorDensity.parse(json['density']),
      ideCommand = json['ideCommand'] as String? ?? IdePreset.presets.first.command,
      fynkelCli = _blankToNull(json['fynkelCli'] as String?),
      fynkelRoot = _blankToNull(json['fynkelRoot'] as String?),
      flutterCommand = json['flutterCommand'] as String? ?? 'flutter',
      zoomToPointer = json['zoomToPointer'] as bool? ?? true,
      _recents = [
        for (final r in json['recents'] as List? ?? const [])
          if (r is Map<String, Object?>) RecentProject.fromJson(r),
      ];

  /// In memory only (tests, and the fallback when the file cannot be read).
  Preferences.inMemory() : this._(null, const {});

  /// Reads [file]; a missing or unreadable file gives defaults (and the
  /// problem in [loadError]) — the editor never refuses to start over it.
  factory Preferences.load(File file) {
    try {
      if (!file.existsSync()) return Preferences._(file, const {});
      final decoded = jsonDecode(file.readAsStringSync());
      if (decoded is Map<String, Object?>) return Preferences._(file, decoded);
      return Preferences._(file, const {})..loadError = 'preferences are not a JSON object';
    } catch (e) {
      return Preferences._(file, const {})..loadError = '$e';
    }
  }

  /// `~/Library/Application Support/Fynkel Editor/` on macOS, `$XDG_CONFIG_HOME/fynkel-editor/` elsewhere.
  static Directory defaultDirectory([Map<String, String>? environment]) {
    final env = environment ?? Platform.environment;
    final home = env['HOME'] ?? env['USERPROFILE'] ?? '.';
    if (Platform.isMacOS) return Directory(p.join(home, 'Library', 'Application Support', 'Fynkel Editor'));
    final xdg = env['XDG_CONFIG_HOME'];
    return Directory(p.join(xdg == null || xdg.isEmpty ? p.join(home, '.config') : xdg, 'fynkel-editor'));
  }

  static File defaultFile([Map<String, String>? environment]) =>
      File(p.join(defaultDirectory(environment).path, 'preferences.json'));

  static String? _blankToNull(String? s) => s == null || s.trim().isEmpty ? null : s;

  final File? file;
  String? loadError;

  EditorTheme theme;

  /// How tall every row, field and toolbar in the editor is.
  EditorDensity density;
  String ideCommand;

  /// Explicit path of the `fynkel` CLI, when the user set one.
  String? fynkelCli;

  /// Explicit Fynkel checkout, when the user set one (`fynkel create --root`).
  String? fynkelRoot;
  String flutterCommand;

  /// A Scene view scroll or pinch zooms about the point under the pointer;
  /// off, about the view's centre (Unity's default).
  bool zoomToPointer;

  List<RecentProject> _recents;
  List<RecentProject> get recents => List.unmodifiable(_recents);

  static const maxRecents = 12;

  void setTheme(EditorTheme t) => _update(() => theme = t);
  void setDensity(EditorDensity d) => _update(() => density = d);
  void setIdeCommand(String c) => _update(() => ideCommand = c.trim().isEmpty ? IdePreset.presets.first.command : c);
  void setFynkelCli(String? path) => _update(() => fynkelCli = _blankToNull(path));
  void setFynkelRoot(String? path) => _update(() => fynkelRoot = _blankToNull(path));
  void setZoomToPointer(bool on) => _update(() => zoomToPointer = on);
  void setFlutterCommand(String c) => _update(() => flutterCommand = c.trim().isEmpty ? 'flutter' : c.trim());

  void addRecent(String dir, String name, {DateTime? now}) => _update(() {
    _recents.removeWhere((r) => p.equals(r.dir, dir));
    _recents.insert(0, RecentProject(dir: dir, name: name, lastOpened: now ?? DateTime.now()));
    if (_recents.length > maxRecents) _recents = _recents.sublist(0, maxRecents);
  });

  void removeRecent(String dir) => _update(() => _recents.removeWhere((r) => p.equals(r.dir, dir)));
  void clearRecents() => _update(() => _recents = []);

  void _update(void Function() f) {
    f();
    save();
    notifyListeners();
  }

  Map<String, Object?> toJson() => {
    'theme': theme.name,
    'density': density.name,
    'ideCommand': ideCommand,
    'fynkelCli': fynkelCli,
    'fynkelRoot': fynkelRoot,
    'flutterCommand': flutterCommand,
    'zoomToPointer': zoomToPointer,
    'recents': [for (final r in _recents) r.toJson()],
  };

  /// Atomic replacement: write `preferences.json.tmp`, then rename over.
  String? save() {
    final f = file;
    if (f == null) return null;
    try {
      f.parent.createSync(recursive: true);
      final tmp = File('${f.path}.tmp');
      tmp.writeAsStringSync('${const JsonEncoder.withIndent('  ').convert(toJson())}\n');
      tmp.renameSync(f.path);
      return null;
    } catch (e) {
      return 'could not save preferences: $e';
    }
  }
}
