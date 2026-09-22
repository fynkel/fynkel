// The panel chrome — the docking tabs (`tabbed_view`, which `docking`
// draws with) and the split dividers (`multi_split_view`) — built from the
// same tokens as everything else (blueprint item 9a stage b).
//
// Both packages ship presets (`TabbedViewThemeData.dark()`, `.classic()`),
// and those presets are the most "third-party Flutter widget" part of the
// window: rounded grey tabs, a wide draggable divider, a ripple. Nothing
// below is a preset.
import 'package:docking/docking.dart';
import 'package:flutter/material.dart';

import 'theme.dart';
import 'tokens.dart';

/// A panel tab strip: flat, the selected tab the colour of the panel below
/// it — the join Rider and Unity both use to say "this tab owns the
/// surface under it". The accent is not here: it follows focus (item 9b),
/// so the focused panel's tab shows it on its icon (`workspace.dart`).
/// Since item 9e a tab's content — icon, title, padlock, the × that fades
/// in under the pointer — is the workspace's own widget, padding included
/// so the pointer is "on the tab" everywhere in it; the package's title is
/// kept at no size and no colour (it still names the tab for the drag, the
/// overflow menu and a screen reader) and it draws no buttons.
TabbedViewThemeData fynkelTabbedViewTheme(FynkelTokens t) {
  const hiddenTitle = TextStyle(fontSize: 0.01, height: 1, color: Colors.transparent);

  return TabbedViewThemeData(
    tabsArea: TabsAreaThemeData(
      color: t.window,
      border: Border(bottom: BorderSide(color: t.line)),
      equalHeights: EqualHeights.all,
      buttonsAreaPadding: const EdgeInsets.symmetric(horizontal: 4),
      buttonPadding: const EdgeInsets.all(3),
      buttonIconSize: 14,
      normalButtonColor: t.textSecondary,
      hoverButtonColor: t.text,
      disabledButtonColor: t.textDisabled,
      hoverButtonBackground: BoxDecoration(color: t.hover, borderRadius: t.cornerControl),
      // The drop target while a panel is being dragged somewhere else.
      dropColor: t.accent.withValues(alpha: .25),
    ),
    tab: TabThemeData(
      textStyle: hiddenTitle,
      padding: EdgeInsets.zero,
      paddingWithoutButton: EdgeInsets.zero,
      buttonsOffset: 0,
      buttonIconSize: 12,
      decoration: const BoxDecoration(color: Colors.transparent),
      draggingDecoration: BoxDecoration(
        color: t.panel,
        border: Border.all(color: t.accent),
      ),
      draggingOpacity: 1,
      normalButtonColor: Colors.transparent,
      hoverButtonColor: t.text,
      disabledButtonColor: Colors.transparent,
      hoverButtonBackground: BoxDecoration(color: t.hover, borderRadius: t.cornerControl),
      buttonPadding: const EdgeInsets.all(2),
      selectedStatus: TabStatusThemeData(
        decoration: BoxDecoration(
          color: t.panel,
          borderRadius: BorderRadius.vertical(top: Radius.circular(t.radiusPanel)),
        ),
        normalButtonColor: t.textDisabled,
        disabledButtonColor: t.textDisabled,
      ),
      highlightedStatus: TabStatusThemeData(
        decoration: BoxDecoration(
          color: t.hover,
          borderRadius: BorderRadius.vertical(top: Radius.circular(t.radiusPanel)),
        ),
        normalButtonColor: t.textDisabled,
        disabledButtonColor: t.textDisabled,
      ),
    ),
    // The panel's own widget paints its background and padding; the content
    // area is only the surface the tab sits on.
    contentArea: ContentAreaThemeData(
      decoration: BoxDecoration(color: t.panel),
      decorationNoTabsArea: BoxDecoration(color: t.panel),
      padding: EdgeInsets.zero,
    ),
    menu: TabbedViewMenuThemeData(
      color: t.popup,
      border: Border.all(color: t.lineStrong),
      hoverColor: t.hover,
      dividerColor: t.line,
      blur: false,
      textStyle: TextStyle(fontFamily: uiFontFamily, fontSize: 11.5, color: t.text),
      menuItemPadding: EdgeInsets.symmetric(horizontal: t.pad, vertical: 5),
      ellipsisOverflowText: true,
    ),
  );
}

/// The gutter between panels: the window colour showing through, with the
/// grip only on hover — a divider you can grab but never notice.
MultiSplitViewThemeData fynkelSplitTheme(FynkelTokens t) => MultiSplitViewThemeData(
  dividerThickness: t.gutter,
  dividerPainter: DividerPainters.background(color: t.window, highlightedColor: t.accent, animationEnabled: false),
);
