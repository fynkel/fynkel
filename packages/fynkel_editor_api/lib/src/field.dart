import 'package:flutter/widgets.dart';

import 'context.dart';

/// What a property drawer's builder gets (ADR-0028 §3, Unity's
/// `PropertyDrawer`): one field, wherever the Inspector shows it — a
/// script's card, a data asset's page, a record inside another, a list's
/// item.
///
/// Like an [InspectorContext] it works on the document's values, and every
/// write goes the default Inspector's way: prechecked, one undo step on
/// every object shown, a prefab override where it should be, the running
/// game's during Play.
abstract interface class FieldContext {
  /// The field's name (`boss`; a list item's index, `2`).
  String get name;

  /// What its row says (`Boss`; a list item's `Item 2`).
  String get label;

  /// The record class it holds (`Wave`), or null for another field.
  String? get record;

  /// Its value as the document holds it: a record as a map of its fields,
  /// a vector as a list, `{"asset": id}` for a reference, null when unset.
  Object? get value;

  /// Whether the objects shown differ here.
  bool get isMixed;

  /// Whether a write is possible now.
  bool get canEdit;

  /// Writes [value] (a document value or a Fynkel one: `Vec3`,
  /// `AssetRef`, an enum); false when refused.
  bool set(Object? value);

  /// A record's fields, in the schema's order (empty for another field).
  List<String> get fields;

  /// A record's [field] value, and a write of it alone.
  Object? valueOf(String field);
  bool setField(String field, Object? value);

  /// A record's [field] drawn by its standard editor (or its own drawer).
  Widget field(String field);

  /// [child] as this field's value, beside its label, with the prefab
  /// override mark and its Revert / Apply — a drawer's usual row.
  Widget row(Widget child);

  /// The field as the Inspector draws it without this drawer.
  Widget defaultEditor();

  /// The editor: the scene, the selection, assets and the Console.
  EditorContext get editor;
}
