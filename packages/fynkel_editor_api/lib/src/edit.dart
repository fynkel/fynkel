import 'package:fynkel/fynkel.dart' show ObjectSpec;

import 'scene.dart';

/// The changes one [EditorContext.edit] makes, in order, as one undo step.
///
/// Values are what the game uses — `Vec3`, `Quat`, `AssetRef`, a written
/// `DataAsset`, an enum — or the document's own JSON (lists, maps,
/// numbers, strings).
abstract interface class EditorEdit {
  /// Sets [field] of [object]'s component of [type] to [value]. [field]
  /// is a field name (`position`) or a pointer into one (`/position/1`).
  void setField(EditorObject object, String type, String field, Object? value);

  /// Renames [object].
  void rename(EditorObject object, String name);

  /// Sets [object]'s own active flag.
  void setActive(EditorObject object, bool active);

  /// Adds a component of [type] with its defaults, then [data] over them.
  void addComponent(EditorObject object, String type, [Map<String, Object?> data = const {}]);

  /// Removes [object]'s component of [type].
  void removeComponent(EditorObject object, String type);

  /// Moves [object] under [parent] (null: to the top), last among its
  /// children.
  void reparent(EditorObject object, EditorObject? parent);

  /// Deletes [object] and everything under it.
  void remove(EditorObject object);

  /// Adds [spec] and everything under it, as `world.build` would make it
  /// and a written scene would hold it (ADR-0026), under [parent] (null:
  /// at the top). [EditResult.added] has it once the edit is done.
  void add(ObjectSpec spec, {EditorObject? parent});
}

/// What an [EditorContext.edit] did.
final class EditResult {
  const EditResult({required this.ok, this.error, this.added = const []});

  /// Written, as one undo step.
  final bool ok;

  /// Why nothing was written (also in the Console).
  final String? error;

  /// The top object of each [EditorEdit.add], in order.
  final List<EditorObject> added;
}
