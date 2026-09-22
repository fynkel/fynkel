// `.fynkel/editor.json`: the project-local editor state the blueprint lets
// the editor write directly (§19.2e "Files the editor reads or writes
// directly") — per scene the editor camera and the hidden and locked
// flags, plus the Scene view's toggles. Editor-only by design: none of it
// is in a document, so `fynkel validate` and git never see a camera move.
// One JSON file, replaced atomically like the preferences; a file that
// does not parse is replaced, never repaired in place.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path/path.dart' as p;

import 'scene_math.dart';

/// What the editor remembers about one scene or prefab document, by its id.
final class SceneEditorState {
  SceneEditorState({EditorCamera? camera, Set<String>? hidden, Set<String>? locked})
    : camera = camera ?? EditorCamera.initial(),
      hidden = hidden ?? <String>{},
      locked = locked ?? <String>{};

  EditorCamera camera;

  /// Entity ids left out of the picture and of picking (with their
  /// descendants) — editor-only.
  final Set<String> hidden;

  /// Entity ids the Scene view neither picks nor drags (the Hierarchy and
  /// the Inspector still edit them) — editor-only.
  final Set<String> locked;

  Map<String, Object?> toJson() => {
    'camera': camera.toJson(),
    if (hidden.isNotEmpty) 'hidden': hidden.toList()..sort(),
    if (locked.isNotEmpty) 'locked': locked.toList()..sort(),
  };

  static SceneEditorState fromJson(Object? json) {
    if (json is! Map) return SceneEditorState();
    Set<String> ids(Object? v) => v is List
        ? {
            for (final s in v)
              if (s is String) s,
          }
        : <String>{};
    return SceneEditorState(
      camera: EditorCamera.fromJson(json['camera']),
      hidden: ids(json['hidden']),
      locked: ids(json['locked']),
    );
  }
}

/// The gizmo tools.
enum SceneTool { select, move, rotate, scale }

/// The gizmo's frame.
enum GizmoSpace { local, world }

/// The Scene view's snapping: on/off and the steps (metres, degrees, a
/// scale factor step).
final class SnapSettings {
  const SnapSettings({this.enabled = false, this.move = 0.5, this.angle = 15, this.scale = 0.1});

  final bool enabled;
  final double move, angle, scale;

  SnapSettings copyWith({bool? enabled, double? move, double? angle, double? scale}) => SnapSettings(
    enabled: enabled ?? this.enabled,
    move: move ?? this.move,
    angle: angle ?? this.angle,
    scale: scale ?? this.scale,
  );

  Map<String, Object?> toJson() => {'enabled': enabled, 'move': move, 'angle': angle, 'scale': scale};

  static SnapSettings fromJson(Object? json) {
    if (json is! Map) return const SnapSettings();
    double step(String k, double fallback) {
      final v = json[k];
      return v is num && v.isFinite && v > 0 ? v.toDouble() : fallback;
    }

    return SnapSettings(
      enabled: json['enabled'] == true,
      move: step('move', 0.5),
      angle: step('angle', 15),
      scale: step('scale', 0.1),
    );
  }
}

/// The whole file. [file] null keeps it in memory (tests, a project the
/// editor may not write into).
final class EditorState extends ChangeNotifier {
  EditorState._(this.file, Map<String, Object?> json)
    : tool = SceneTool.values.asNameMap()[json['tool']] ?? SceneTool.move,
      space = GizmoSpace.values.asNameMap()[json['space']] ?? GizmoSpace.world,
      snap = SnapSettings.fromJson(json['snap']),
      showGrid = json['grid'] != false,
      showOverlays = json['overlays'] != false,
      showStats = json['stats'] == true,
      _scenes = {
        if (json['scenes'] is Map)
          for (final e in (json['scenes'] as Map).entries)
            if (e.key is String) e.key as String: SceneEditorState.fromJson(e.value),
      };

  static const formatVersion = 1;
  static const fileName = 'editor.json';

  /// The file of a project directory: `<dir>/.fynkel/editor.json`.
  static File fileOf(String projectDir) => File(p.join(projectDir, '.fynkel', fileName));

  /// Reads the project's file (a missing or unreadable one starts fresh;
  /// a newer format is kept in memory only, never overwritten).
  static EditorState load(String projectDir) => _read(fileOf(projectDir));

  static EditorState inMemory() => EditorState._(null, const {});

  static EditorState _read(File f) {
    try {
      if (!f.existsSync()) return EditorState._(f, const {});
      final json = jsonDecode(f.readAsStringSync());
      if (json is! Map<String, Object?>) return EditorState._(f, const {});
      final version = json['formatVersion'];
      if (version is int && version > formatVersion) {
        // Written by a newer editor: read what we understand, write nothing.
        return EditorState._(null, json)..readOnlyReason = 'written by a newer editor (format $version)';
      }
      return EditorState._(f, json);
    } catch (e) {
      return EditorState._(f, const {})..loadProblem = '$e';
    }
  }

  final File? file;
  final Map<String, SceneEditorState> _scenes;

  /// Set when the file did not parse (it is replaced on the next save).
  String? loadProblem;
  String? readOnlyReason;

  SceneTool tool;
  GizmoSpace space;
  SnapSettings snap;
  bool showGrid, showOverlays, showStats;

  /// The state of a document, created on first use.
  SceneEditorState scene(String documentId) => _scenes.putIfAbsent(documentId, SceneEditorState.new);

  bool hasScene(String documentId) => _scenes.containsKey(documentId);
  Iterable<String> get sceneIds => _scenes.keys;

  /// Marks a change and saves; the toggles above are written by the model
  /// through this.
  void changed() {
    save();
    notifyListeners();
  }

  Timer? _pending;

  /// A camera moved: save soon (one write per pause, not one per pixel of
  /// an orbit). [flush] writes at once.
  void scheduleSave({Duration after = const Duration(milliseconds: 400)}) {
    _pending?.cancel();
    _pending = Timer(after, flush);
  }

  /// Writes a pending [scheduleSave] now (a tab or project closing).
  void flush() {
    if (_pending == null) return;
    _pending?.cancel();
    _pending = null;
    save();
  }

  /// A save is waiting (tests).
  bool get savePending => _pending != null;

  @override
  void dispose() {
    flush();
    super.dispose();
  }

  Map<String, Object?> toJson() => {
    'formatVersion': formatVersion,
    'tool': tool.name,
    'space': space.name,
    'snap': snap.toJson(),
    'grid': showGrid,
    'overlays': showOverlays,
    'stats': showStats,
    'scenes': {for (final e in _scenes.entries) e.key: e.value.toJson()},
  };

  /// Atomic replacement (`editor.json.tmp`, then rename over); null when
  /// written or not backed by a file, else why not.
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
      return 'could not save ${f.path}: $e';
    }
  }
}
