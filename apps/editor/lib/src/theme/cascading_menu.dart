// A context menu with submenus that open on hover (Unity's Hierarchy
// menus: GameObject ▸ 3D Object ▸ Cube). `showMenu` has no submenus, so
// this puts a `MenuAnchor` in the overlay at the pointer and opens it.
import 'dart:async';

import 'package:flutter/material.dart';

/// One line of a [showCascadingMenu].
sealed class MenuLine {
  const MenuLine();
}

final class MenuAction extends MenuLine {
  const MenuAction(this.label, {this.key, this.enabled = true, required this.onSelected});

  final String label;
  final Key? key;
  final bool enabled;
  final VoidCallback onSelected;
}

final class MenuSubmenu extends MenuLine {
  const MenuSubmenu(this.label, this.lines, {this.key});

  final String label;
  final Key? key;
  final List<MenuLine> lines;
}

final class MenuSeparator extends MenuLine {
  const MenuSeparator();
}

/// Shows [lines] with the top-left corner at [globalPosition]; completes
/// once the menu has closed (after the chosen action ran).
Future<void> showCascadingMenu(BuildContext context, Offset globalPosition, List<MenuLine> lines) {
  final overlay = Overlay.of(context);
  final box = overlay.context.findRenderObject()! as RenderBox;
  final at = box.globalToLocal(globalPosition);
  final controller = MenuController();
  final done = Completer<void>();
  var closed = false;
  late final OverlayEntry entry;
  entry = OverlayEntry(
    builder: (context) => Positioned(
      left: at.dx,
      top: at.dy,
      child: MenuAnchor(
        controller: controller,
        onClose: () {
          if (closed) return;
          closed = true;
          WidgetsBinding.instance.addPostFrameCallback((_) {
            entry.remove();
            entry.dispose();
            done.complete();
          });
        },
        menuChildren: _widgets(lines),
        child: const SizedBox.shrink(),
      ),
    ),
  );
  overlay.insert(entry);
  WidgetsBinding.instance.addPostFrameCallback((_) {
    if (!closed) controller.open();
  });
  return done.future;
}

List<Widget> _widgets(List<MenuLine> lines) => [
  for (final line in lines)
    switch (line) {
      MenuAction() => MenuItemButton(
        key: line.key,
        onPressed: line.enabled ? line.onSelected : null,
        child: Text(line.label),
      ),
      MenuSubmenu() => SubmenuButton(key: line.key, menuChildren: _widgets(line.lines), child: Text(line.label)),
      MenuSeparator() => const Divider(height: 1),
    },
];
