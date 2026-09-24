// Rendered thumbnails for the Project grid (Unity's asset previews): a
// model, a prefab or a mesh drawn at a three-quarter view, a material on a
// sphere — by `fynkel thumbnail`, in the background, a batch at a time,
// into `.fynkel/thumbnails/`. A thumbnail is stale when its asset's file
// changed (its size and modification time); the old picture stays up until
// the new one is drawn, as Unity's does. A CLI that cannot draw one (an
// older build, no GPU) leaves the kind's icon, and is not asked again for
// that file until it changes.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart';
import 'package:path/path.dart' as p;

import '../env/tools.dart' show RunProcess, runProcess;

final class ThumbnailCache extends ChangeNotifier {
  ThumbnailCache({required this.projectDir, required this.cli, this.run = runProcess, this.size = 128});

  final String projectDir;

  /// The `fynkel` executable, when one was found.
  final String? Function() cli;
  final RunProcess run;

  /// Pixels a side: the largest tile, on a Retina screen.
  final int size;

  String get dir => p.join(projectDir, '.fynkel', 'thumbnails');
  String get _indexFile => p.join(dir, 'index.json');

  /// The file of [asset] (and its part [sub]) under [dir]: the CLI's name.
  static String fileName(String asset, String? sub) =>
      sub == null ? '$asset.png' : '${asset}__${sub.replaceAll('/', '_')}.png';

  /// What each thumbnail was drawn from (`asset` or `asset#sub` → stamp).
  late final Map<String, String> _drawn = _load();

  /// Asked for and not drawn (the CLI's reason), by the stamp it was asked at.
  final _refused = <String, String>{};

  /// Waiting for the next batch: key → stamp.
  final _queue = <String, String>{};
  bool _running = false;
  Timer? _debounce;

  /// Bumped when a thumbnail is drawn again, so a tile's image is new.
  final _generation = <String, int>{};

  Map<String, String> _load() {
    try {
      final json = jsonDecode(File(_indexFile).readAsStringSync());
      if (json is Map) return {for (final e in json.entries) '${e.key}': '${e.value}'};
    } catch (_) {}
    return {};
  }

  static String _key(String asset, String? sub) => sub == null ? asset : '$asset#$sub';

  /// The size and modification time of [source]; null when it is gone.
  static String? stampOf(String source) {
    final s = FileStat.statSync(source);
    if (s.type == FileSystemEntityType.notFound) return null;
    return '${s.size}:${s.modified.millisecondsSinceEpoch}';
  }

  /// The picture of [asset] (its part [sub]), drawn from the file
  /// [source]: the image when there is one (stale or not), else null. A
  /// missing or stale one is queued to be drawn.
  ImageProvider? image(String asset, String? sub, String source) {
    final key = _key(asset, sub);
    final stamp = stampOf(source);
    final file = File(p.join(dir, fileName(asset, sub)));
    final exists = file.existsSync();
    if (stamp != null && (!exists || _drawn[key] != stamp) && _refused[key] != stamp && _queue[key] != stamp) {
      _queue[key] = stamp;
      _debounce ??= Timer(const Duration(milliseconds: 150), _flush);
    }
    return exists ? ThumbnailImage(file, _generation[key] ?? 0) : null;
  }

  /// Draws what is queued, a batch per CLI run, until nothing is.
  Future<void> _flush() async {
    _debounce = null;
    if (_running) return;
    final exe = cli();
    if (exe == null) {
      _queue.clear();
      return;
    }
    _running = true;
    try {
      while (_queue.isNotEmpty) {
        final batch = Map.of(_queue);
        _queue.clear();
        Directory(dir).createSync(recursive: true);
        ProcessResult r;
        try {
          r = await run(exe, [
            'thumbnail',
            '--project',
            projectDir,
            '--size',
            '$size',
            '--out-dir',
            dir,
            '--json',
            for (final k in batch.keys) k,
          ], workingDirectory: projectDir);
        } catch (e) {
          batch.forEach((k, s) => _refused[k] = s);
          continue;
        }
        final drawn = <String>{};
        try {
          final json = jsonDecode('${r.stdout}');
          for (final t in (json as Map)['thumbnails'] as List) {
            final m = t as Map;
            final key = _key('${m['asset']}', m['sub'] as String?);
            if (m['file'] != null && batch.containsKey(key)) drawn.add(key);
          }
        } catch (_) {
          // Not JSON: an older CLI without the command, a failed cook.
        }
        for (final e in batch.entries) {
          if (drawn.contains(e.key)) {
            _drawn[e.key] = e.value;
            _refused.remove(e.key);
            _generation[e.key] = (_generation[e.key] ?? 0) + 1;
          } else {
            _refused[e.key] = e.value;
          }
        }
        if (drawn.isNotEmpty) {
          try {
            File(_indexFile).writeAsStringSync(jsonEncode(_drawn));
          } catch (_) {}
          notifyListeners();
        }
      }
    } finally {
      _running = false;
    }
  }

  /// Whether a batch is being drawn or waits (tests).
  bool get busy => _running || _queue.isNotEmpty || _debounce != null;

  @override
  void dispose() {
    _debounce?.cancel();
    super.dispose();
  }
}

/// A thumbnail's file at one generation: drawn again, it is a new image
/// (the image cache keys a plain `FileImage` by its path alone).
final class ThumbnailImage extends FileImage {
  const ThumbnailImage(super.file, this.generation);

  final int generation;

  @override
  bool operator ==(Object other) =>
      other is ThumbnailImage && other.file.path == file.path && other.generation == generation;

  @override
  int get hashCode => Object.hash(file.path, generation);
}
