import 'package:flutter/widgets.dart';

import 'context.dart';

/// What a custom Inspector's builder gets (ADR-0028 §3, Unity's
/// `SerializedObject` with `CustomEditor`'s helpers): the component on
/// every object the Inspector shows, or a data asset's fields.
///
/// It works on the document's values: no behaviour instance runs in edit
/// mode, so a custom Inspector never calls a script's methods. Every write
/// goes the way the default Inspector's do — prechecked, one undo step, a
/// prefab override where the object comes from a prefab, and during Play
/// the running game's.
abstract interface class InspectorContext {
  /// The component's or data type's id (`game.Collectible`).
  String get type;

  /// How many objects the Inspector shows (1 for a data asset).
  int get targets;

  /// Whether a write is possible now (a read-only document is not).
  bool get canEdit;

  /// The type's fields, in the schema's order.
  List<String> get fields;

  /// [field]'s value as the document holds it (the first target's when
  /// they differ): a number, a string, a bool, a list for a vector or a
  /// colour, `{"asset": id}` for an asset or data asset, null when unset.
  Object? value(String field);

  /// Whether the targets differ at [field].
  bool isMixed(String field);

  /// Writes [value] into [field] on every target, one undo step. It takes
  /// what [value] reads, or a Fynkel value (`Vec3`, `AssetRef`, a written
  /// `DataAsset`, an enum). False when it was refused; the field's row
  /// then says why.
  bool set(String field, Object? value);

  /// [field]'s standard editor, as the default Inspector draws it.
  Widget field(String field);

  /// Every field, as the default Inspector draws them, but those in
  /// [except] (Unity's `DrawDefaultInspector` and
  /// `DrawPropertiesExcluding`).
  Widget defaultInspector({Iterable<String> except = const []});

  /// The editor: the scene, the selection, assets and the Console.
  EditorContext get editor;
}
