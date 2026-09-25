import 'package:flutter/widgets.dart';

import 'assets.dart';
import 'context.dart';
import 'field.dart';
import 'inspector.dart';
import 'scene.dart';

/// What `registerEditor` is given: one package's way into the editor. The
/// editor calls every package's `registerEditor` at start and after every
/// hot reload, with a fresh one each time — what it registered before is
/// gone, so a removed menu item disappears too.
abstract interface class EditorExtensions {
  /// The package whose editor part this is (`code_only`).
  String get package;

  /// A menu command (Unity's `[MenuItem]`).
  ///
  /// [path] is the menu, an optional submenu and the label, split by `/`:
  /// `Tools/Snap to Grid`, `Tools/Level/Ring of Coins`. The menu is one of
  /// the editor's (`Project`, `Edit`, `GameObject`, `View`, `Play`,
  /// `Tools`, `Window`, `Help`); extensions usually use `Tools`.
  ///
  /// [id] is the command's stable id, dotted and prefixed by the package's
  /// own name or namespace (`game.snapToGrid`): the command palette, the
  /// shortcuts and automation (ADR-0017) know it by that id.
  ///
  /// [run] runs it; [enabled], when given, greys it out while it returns
  /// false (Unity's validate method). [shortcut] is its key, e.g.
  /// `SingleActivator(LogicalKeyboardKey.keyG, meta: true, alt: true)`; one
  /// the editor already uses is refused.
  ///
  /// A bad path, a taken id or a taken shortcut is a Console error naming
  /// the package, and the item is left out.
  void menuItem(
    String path, {
    required String id,
    required void Function(EditorContext context) run,
    bool Function(EditorContext context)? enabled,
    SingleActivator? shortcut,
    String? description,
  });

  /// A custom Inspector (Unity's `CustomEditor`): [build] draws the card of
  /// the script [T] (`inspector<Collectible>`) on the objects the
  /// Inspector shows, or the page of the data type [T]
  /// (`inspector<CoinKind>`), in place of the default one.
  ///
  /// [T] is a `@FynkelBehaviour` or `@FynkelDataAsset` class of the game;
  /// one the editor's game does not register is a Console error, and the
  /// default Inspector stays. [build] runs again whenever the values
  /// change. A throw in it shows the default Inspector with a line saying
  /// why, and a Console error.
  ///
  /// Hints come first (ADR-0018: `@Expose(min:, kind:, header:, …)`):
  /// write a custom Inspector when they cannot say what you need.
  void inspector<T>(Widget Function(BuildContext context, InspectorContext inspector) build);

  /// A property drawer for the record class [T] (`drawer<Wave>`, a
  /// `@FynkelRecord`): [build] draws every field holding one — alone, in a
  /// list, inside another record — in place of its foldout (Unity's
  /// `CustomPropertyDrawer(typeof(Wave))`). A record is known by its class
  /// name, as the schema's `x-fynkel.record` names it.
  ///
  /// A throw in [build] draws the field the default way with a line
  /// saying why, and a Console error. Hints come first (ADR-0018).
  void drawer<T>(Widget Function(BuildContext context, FieldContext field) build);

  /// A property drawer by name: [build] draws every field annotated
  /// `@Expose(drawer: name)` (each item, on a list), whatever its type. A
  /// field's named drawer comes before its record's.
  void namedDrawer(String name, Widget Function(BuildContext context, FieldContext field) build);

  /// A window (Unity's `EditorWindow`): a panel the user docks anywhere,
  /// whose place the layout keeps across restarts and hot reloads.
  ///
  /// [id] is its stable id, dotted like a menu item's (`game.levelTools`).
  /// [menu] is the menu path that shows it (`Tools/Level Tools`); by
  /// default `Window/<title>`. [build] draws it with the editor's theme and
  /// runs again whenever the scene, the selection or the assets change, so
  /// what it reads from [EditorContext] is current. A throw in [build] is
  /// shown in the window's frame and logged in the Console.
  void window(
    String id, {
    required String title,
    String? menu,
    IconData? icon,
    required Widget Function(BuildContext context, EditorContext editor) build,
  });

  /// An entry in the Hierarchy's context menu (Unity's `GameObject/` menu
  /// items on a right click): [run] gets the selected objects. With
  /// [withComponent] (`game.Collectible`) it is offered only when every
  /// selected object has one; [enabled] greys it out.
  void hierarchyMenuItem(
    String label, {
    required String id,
    String? withComponent,
    required void Function(EditorContext context, List<EditorObject> objects) run,
    bool Function(EditorContext context, List<EditorObject> objects)? enabled,
  });

  /// An entry in the Project panel's context menu (Unity's `Assets/` menu
  /// items): [run] gets the selected assets. With [kind] (`prefab`) or
  /// [dataType] (`game.CoinKind`) it is offered only when every selected
  /// asset is one; [enabled] greys it out.
  void projectMenuItem(
    String label, {
    required String id,
    String? kind,
    String? dataType,
    required void Function(EditorContext context, List<EditorAsset> assets) run,
    bool Function(EditorContext context, List<EditorAsset> assets)? enabled,
  });

  /// An entry in the ⋮ menu of the Inspector's [type] cards (Unity's
  /// `CONTEXT/<Type>/` items and `[ContextMenu]`): [run] gets the objects
  /// the Inspector shows, each with a [type] component.
  void componentMenuItem(
    String type,
    String label, {
    required String id,
    required void Function(EditorContext context, List<EditorObject> objects) run,
    bool Function(EditorContext context, List<EditorObject> objects)? enabled,
  });
}
