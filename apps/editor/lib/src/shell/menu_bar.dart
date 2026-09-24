// The menu bar over the command registry. On macOS it is the native menu
// bar (`PlatformMenuBar`: the system shows the shortcuts and the app menu
// gets About / Preferences / Quit); everywhere else — widget tests included —
// it is Flutter's in-window `MenuBar`. Both list exactly the registry.
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../commands/commands.dart';

/// Native menus on macOS, unless a test turns them off.
bool useNativeMenuBar() => !kIsWeb && defaultTargetPlatform == TargetPlatform.macOS;

class EditorMenuBar extends StatelessWidget {
  const EditorMenuBar({
    super.key,
    required this.registry,
    required this.child,
    this.native,
    this.aboutCommand = 'help.about',
    this.preferencesCommand = 'app.preferences',
  });

  final CommandRegistry registry;
  final Widget child;
  final bool? native;
  final String aboutCommand;
  final String preferencesCommand;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: registry,
      builder: (context, _) {
        if (native ?? useNativeMenuBar()) {
          return PlatformMenuBar(menus: platformMenus(), child: child);
        }
        return Column(
          children: [
            MenuBar(key: const Key('menubar'), children: _widgetMenus(context)),
            Expanded(child: child),
          ],
        );
      },
    );
  }

  /// Menu items grouped by `group`, in registry order.
  List<List<EditorCommand>> _groups(EditorMenu menu) {
    final groups = <List<EditorCommand>>[];
    int? current;
    for (final c in registry.inMenu(menu)) {
      if (c.group != current) {
        groups.add([]);
        current = c.group;
      }
      groups.last.add(c);
    }
    return groups;
  }

  /// A group's items with the ones of one submenu gathered: `(null, [c])`
  /// for an item of its own, `(title, items)` for a submenu.
  static List<(String?, List<EditorCommand>)> _entries(List<EditorCommand> group) {
    final out = <(String?, List<EditorCommand>)>[];
    for (final c in group) {
      if (c.submenu != null && out.isNotEmpty && out.last.$1 == c.submenu) {
        out.last.$2.add(c);
      } else {
        out.add((c.submenu, [c]));
      }
    }
    return out;
  }

  /// The native menus. About, Preferences and Quit live in the app menu, so a
  /// group or a menu left empty by that move is dropped: `PlatformMenuItemGroup`
  /// asserts on an empty member list (the Edit menu only carries Preferences).
  @visibleForTesting
  List<PlatformMenu> platformMenus() {
    final about = registry.byId(aboutCommand);
    final prefs = registry.byId(preferencesCommand);
    List<PlatformMenuItemGroup> groupsOf(EditorMenu menu) => [
      for (final group in _groups(menu))
        if (group.any(_inOwnMenu))
          PlatformMenuItemGroup(
            members: [
              for (final (submenu, items) in _entries(group.where(_inOwnMenu).toList()))
                if (submenu == null)
                  _platformItem(items.single)
                else
                  PlatformMenu(label: submenu, menus: [for (final c in items) _platformItem(c)]),
            ],
          ),
    ];
    return [
      PlatformMenu(
        label: 'Fynkel Editor',
        menus: [
          if (about != null) PlatformMenuItem(label: about.label, onSelected: () => registry.execute(about.id)),
          if (prefs != null)
            PlatformMenuItemGroup(
              members: [
                PlatformMenuItem(
                  label: prefs.label,
                  shortcut: prefs.shortcut,
                  onSelected: () => registry.execute(prefs.id),
                ),
              ],
            ),
          if (PlatformProvidedMenuItem.hasMenu(PlatformProvidedMenuItemType.servicesSubmenu))
            const PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.servicesSubmenu),
          const PlatformMenuItemGroup(
            members: [
              PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.hide),
              PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.hideOtherApplications),
              PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.showAllApplications),
            ],
          ),
          const PlatformProvidedMenuItem(type: PlatformProvidedMenuItemType.quit),
        ],
      ),
      for (final menu in EditorMenu.values)
        if (groupsOf(menu) case final groups when groups.isNotEmpty) PlatformMenu(label: menu.title, menus: groups),
    ];
  }

  PlatformMenuItem _platformItem(EditorCommand c) => PlatformMenuItem(
    label: c.label,
    shortcut: c.shortcut,
    onSelected: c.isEnabled ? () => registry.execute(c.id) : null,
  );

  /// False for the commands the app menu already carries.
  bool _inOwnMenu(EditorCommand c) => c.id != aboutCommand && c.id != preferencesCommand && c.id != 'app.quit';

  List<Widget> _widgetMenus(BuildContext context) {
    return [
      for (final menu in EditorMenu.values)
        SubmenuButton(
          key: Key('menu.${menu.name}'),
          menuChildren: [
            for (final (i, group) in _groups(menu).indexed) ...[
              if (i > 0) const Divider(height: 1),
              for (final (submenu, items) in _entries(group))
                if (submenu == null)
                  _widgetItem(items.single)
                else
                  SubmenuButton(
                    key: Key('submenu.${menu.name}.$submenu'),
                    menuChildren: [for (final c in items) _widgetItem(c)],
                    child: Text(submenu),
                  ),
            ],
          ],
          child: Text(menu.title),
        ),
    ];
  }

  Widget _widgetItem(EditorCommand c) => MenuItemButton(
    key: Key('menuitem.${c.id}'),
    // Our own label (⌘K), not Flutter's platform spelling.
    trailingIcon: c.shortcut == null ? null : Text(CommandRegistry.shortcutLabel(c.shortcut!)),
    onPressed: c.isEnabled ? () => registry.execute(c.id) : null,
    child: Text(c.label),
  );
}
