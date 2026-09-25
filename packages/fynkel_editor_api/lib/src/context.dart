import 'assets.dart';
import 'edit.dart';
import 'play.dart';
import 'scene.dart';

/// The editor, as an extension's callbacks see it (ADR-0028 §4). Every
/// callback is given one; it speaks for the package that registered the
/// callback.
///
/// What it reads is a snapshot taken when asked: read [scene] or
/// [selection] again after an [edit] to see the change.
abstract interface class EditorContext {
  /// The package it speaks for.
  String get package;

  /// A line in the Console, attributed to [package] (Unity's `Debug.Log`).
  void log(String message);

  /// A warning in the Console (`Debug.LogWarning`).
  void logWarning(String message);

  /// An error in the Console (`Debug.LogError`).
  void logError(String message);

  /// The scene or prefab in front — the one the Hierarchy shows (during
  /// Play, the running game's); null when none is open.
  EditorScene? get scene;

  /// The objects selected in [scene], in the order they were selected
  /// (Unity's `Selection.objects`).
  List<EditorObject> get selection;

  /// Selects [objects] of [scene] (none: clears the selection).
  void select(Iterable<EditorObject> objects);

  /// Changes [scene] as one undo step (Unity's `Undo.RecordObject` and
  /// `Undo.RegisterCreatedObjectUndo` in one): [changes] lists what to
  /// do on [EditorEdit]; nothing is written until it returns, and then
  /// everything is written, or nothing when one change is refused. The
  /// changes go through the editor, as the Inspector's and the
  /// Hierarchy's do: undo, prefab overrides and, during Play, the running
  /// game (never saved unless applied, as any Play edit).
  ///
  /// [label] names the step (a refusal in the Console says which).
  EditResult edit(String label, void Function(EditorEdit edit) changes);

  /// The project's assets.
  EditorAssets get assets;

  /// The running game during Play; null while the editor is stopped (or
  /// has no game compiled in). A window's build runs again when Play
  /// starts and stops.
  EditorPlay? get play;
}
