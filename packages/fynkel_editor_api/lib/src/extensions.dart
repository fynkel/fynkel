import 'package:flutter/widgets.dart';

import 'context.dart';

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
}
