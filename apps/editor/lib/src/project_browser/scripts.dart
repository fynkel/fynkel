// The Project browser's Scripts root (roadmap Phase 2 item 1): the game's
// Dart source under `lib/`, beside the assets. Scripts are not assets —
// the session never indexes them — so the browser lists the directory
// itself, opens a file in the IDE, and makes a new behaviour from a
// template (Unity's Create → C# Script): the file, then `fynkel codegen`
// for its metadata and schema, then the session's `registerBehaviour`.
import 'dart:io';

import 'package:path/path.dart' as p;

/// What the Scripts root holds now: every folder and `.dart` file under
/// `lib/`, relative and `/`-separated, sorted; hidden entries skipped.
final class ScriptTree {
  const ScriptTree({required this.folders, required this.files});

  static const empty = ScriptTree(folders: [], files: []);

  final List<String> folders;
  final List<String> files;

  static ScriptTree read(String libDir) {
    final root = Directory(libDir);
    if (!root.existsSync()) return empty;
    final folders = <String>[];
    final files = <String>[];
    void walk(Directory d, String prefix) {
      final List<FileSystemEntity> children;
      try {
        children = d.listSync(followLinks: false);
      } on FileSystemException {
        return;
      }
      for (final c in children) {
        final name = p.basename(c.path);
        if (name.startsWith('.')) continue;
        final rel = prefix.isEmpty ? name : '$prefix/$name';
        if (c is Directory) {
          folders.add(rel);
          walk(c, rel);
        } else if (c is File && name.endsWith('.dart')) {
          files.add(rel);
        }
      }
    }

    walk(root, '');
    int byName(String a, String b) => a.toLowerCase().compareTo(b.toLowerCase());
    return ScriptTree(folders: folders..sort(byName), files: files..sort(byName));
  }
}

/// Whether [rel] (under `lib/`) is written by `fynkel codegen`.
bool isGeneratedScript(String rel) => rel.startsWith('generated/') || rel.endsWith('.g.dart');

/// The behaviour ids per file under `lib/` from the metadata's `library`
/// (`package:<pkg>/<rel>`): which scripts declare a behaviour.
Map<String, List<String>> behaviourScripts(Map<String, Object?>? metadata) {
  final out = <String, List<String>>{};
  final package = metadata?['package'];
  for (final b in metadata?['behaviours'] as List? ?? const []) {
    if (b is! Map) continue;
    final lib = b['library'];
    final id = b['id'];
    if (lib is! String || id is! String) continue;
    final prefix = package is String ? 'package:$package/' : null;
    final at = prefix != null && lib.startsWith(prefix) ? prefix.length : lib.indexOf('/') + 1;
    if (!lib.startsWith('package:') || at <= 0) continue;
    out.putIfAbsent(lib.substring(at), () => []).add(id);
  }
  return out;
}

/// A class name a new behaviour may take: UpperCamelCase, letters and digits.
bool isBehaviourClassName(String name) => RegExp(r'^[A-Z][A-Za-z0-9]*$').hasMatch(name);

/// `HoverPad` → `hover_pad`, `HUDLabel` → `hud_label`.
String snakeCase(String name) => name
    .replaceAllMapped(RegExp(r'([A-Z]+)([A-Z][a-z])'), (m) => '${m[1]}_${m[2]}')
    .replaceAllMapped(RegExp(r'([a-z0-9])([A-Z])'), (m) => '${m[1]}_${m[2]}')
    .toLowerCase();

/// The namespace new behaviour ids take: the one every existing behaviour
/// shares (`game` for `game.Player`, `game.Gate`), else `game`.
String behaviourNamespace(Map<String, Object?>? metadata) {
  final spaces = <String>{};
  for (final b in metadata?['behaviours'] as List? ?? const []) {
    final id = b is Map ? b['id'] : null;
    if (id is String && id.contains('.')) spaces.add(id.substring(0, id.lastIndexOf('.')));
  }
  return spaces.length == 1 ? spaces.single : 'game';
}

/// The new behaviour's source: one exposed field and the usual overrides,
/// ready to fill in. Its doc comment is the metadata's description, so it
/// stays one line.
String behaviourSource(String id, String className) =>
    '''
import 'package:fynkel/gameplay.dart';

/// What $className does (the Inspector shows this comment).
@FynkelBehaviour(id: '$id')
class $className extends Behaviour {
  @Expose(min: 0, step: 0.1, tooltip: 'Units per second')
  double speed = 1.0;

  @override
  void start() {}

  @override
  void update(UpdateContext context) {}
}
''';

/// A behaviour made by [writeBehaviour].
final class NewBehaviour {
  const NewBehaviour({required this.id, required this.className, required this.file, required this.rel});

  final String id;
  final String className;

  /// The absolute path of the new file.
  final String file;

  /// Its path under `lib/`.
  final String rel;
}

/// Writes [className]'s behaviour file in [folder] (under [libDir]) —
/// refused, with nothing written, for a bad name, a file already there or
/// an id in [taken]. Returns the behaviour, or throws a [FormatException]
/// with the reason.
NewBehaviour writeBehaviour({
  required String libDir,
  required String folder,
  required String className,
  required String namespace,
  Set<String> taken = const {},
}) {
  final name = className.trim();
  if (!isBehaviourClassName(name)) {
    throw FormatException('“$name” is not a class name: start with a capital letter, then letters and digits.');
  }
  final rel = [if (folder.isNotEmpty) folder, '${snakeCase(name)}.dart'].join('/');
  final file = p.join(libDir, p.joinAll(rel.split('/')));
  if (File(file).existsSync()) throw FormatException('lib/$rel is already there.');
  final id = '$namespace.$name';
  if (taken.contains(id)) throw FormatException('A behaviour named $id is already there.');
  try {
    File(file)
      ..parent.createSync(recursive: true)
      ..writeAsStringSync(behaviourSource(id, name));
  } on FileSystemException catch (e) {
    throw FormatException('Cannot write lib/$rel: ${e.osError?.message ?? e.message}');
  }
  return NewBehaviour(id: id, className: name, file: file, rel: rel);
}
