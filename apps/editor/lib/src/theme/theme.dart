// The editor's `ThemeData`, built from `FynkelTokens` (blueprint §19.2e
// item 9a). Every component theme here is explicit on purpose: Material's
// own defaults are sized for fingers, animated, tinted from a seed colour
// and rounded like a phone app, and any one of them left alone shows
// through as "a Flutter app". Nothing below reads a Material default, and
// nothing in the panels names a colour that is not a token.
import 'dart:io';

import 'package:flutter/material.dart';

import 'tokens.dart';

/// The platform's UI face. On macOS `.AppleSystemUIFont` resolves to SF
/// Pro; elsewhere the engine's default is already the platform's own.
String? get uiFontFamily => Platform.isMacOS ? '.AppleSystemUIFont' : null;

/// The platform's monospace face, for ids, paths, numbers and raw JSON —
/// everything the eye compares character by character.
String get monoFontFamily => Platform.isMacOS ? 'SF Mono' : 'monospace';

const List<String> _monoFallback = ['Menlo', 'Consolas', 'DejaVu Sans Mono', 'monospace'];

/// Numbers in a column line up; a digit is as wide as its neighbour.
const _tabular = [FontFeature.tabularFigures()];

/// Builds the editor's theme. [density] decides every row height through
/// the tokens; [brightness] picks one of the two palettes.
ThemeData fynkelTheme({required Brightness brightness, EditorDensity density = EditorDensity.standard, Color? tint}) {
  final t = FynkelTokens.of(brightness, density, tint);
  // Every role the UI reads is spelled out, including the container roles:
  // a panel that asks for `colorScheme.surfaceContainerHighest` must land
  // on a token, not on whatever Material would have derived from a seed.
  final scheme = ColorScheme(
    brightness: brightness,
    primary: t.accent,
    onPrimary: t.onAccent,
    // "primary container" is this editor's selected row.
    primaryContainer: t.selection,
    onPrimaryContainer: t.onSelection,
    secondary: t.accent,
    onSecondary: t.onAccent,
    // A neutral badge: a count, a kind, a keyboard hint.
    secondaryContainer: t.lineStrong,
    onSecondaryContainer: t.text,
    tertiary: t.accent,
    onTertiary: t.onAccent,
    // Prefabs, which the editor tints with the accent the way Unity does.
    tertiaryContainer: t.surfaceOf(t.accent, .28),
    onTertiaryContainer: t.accent,
    error: t.error,
    onError: t.onAccent,
    errorContainer: t.surfaceOf(t.error, .22),
    onErrorContainer: t.error,
    surface: t.panel,
    onSurface: t.text,
    onSurfaceVariant: t.textSecondary,
    surfaceDim: t.window,
    surfaceBright: t.popup,
    surfaceContainerLowest: t.window,
    surfaceContainerLow: t.panel,
    surfaceContainer: t.header,
    surfaceContainerHigh: t.header,
    surfaceContainerHighest: t.header,
    surfaceTint: Colors.transparent, // M3 tints raised surfaces; we do not.
    outline: t.lineStrong,
    outlineVariant: t.line,
    shadow: t.shadow,
    scrim: t.shadow,
    inverseSurface: t.text,
    onInverseSurface: t.panel,
    inversePrimary: t.accent,
  );

  // Slightly tight: SF at 11–12 px reads wide next to Unity's Inter at 12.
  TextStyle ui(double size, {FontWeight weight = FontWeight.w400, Color? color}) => TextStyle(
    fontFamily: uiFontFamily,
    fontSize: size,
    fontWeight: weight,
    color: color ?? t.text,
    height: 1.3,
    letterSpacing: -0.15,
  );

  final text = TextTheme(
    // The editor's own scale: one reading size, one quiet size, one title.
    bodyMedium: ui(11.5),
    bodySmall: ui(11, color: t.textSecondary),
    bodyLarge: ui(12.5),
    titleSmall: ui(12.5, weight: FontWeight.w600),
    titleMedium: ui(13.5, weight: FontWeight.w600),
    titleLarge: ui(16, weight: FontWeight.w600),
    labelLarge: ui(11.5, weight: FontWeight.w500),
    labelMedium: ui(11, weight: FontWeight.w500, color: t.textSecondary),
    labelSmall: ui(10.5, weight: FontWeight.w500, color: t.textSecondary),
    headlineSmall: ui(18, weight: FontWeight.w600),
    headlineMedium: ui(22, weight: FontWeight.w600),
  );

  final border = OutlineInputBorder(
    borderRadius: t.cornerControl,
    borderSide: BorderSide(color: t.fieldEdge, width: 1),
  );

  return ThemeData(
    useMaterial3: true,
    brightness: brightness,
    colorScheme: scheme,
    extensions: [t],
    textTheme: text,
    fontFamily: uiFontFamily,
    visualDensity: VisualDensity.compact,
    materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,

    // ── surfaces ────────────────────────────────────────────────────────
    scaffoldBackgroundColor: t.window,
    canvasColor: t.panel,
    cardColor: t.panel,
    dividerColor: t.line,
    disabledColor: t.textDisabled,
    hintColor: t.textSecondary,
    shadowColor: t.shadow,

    // ── no motion but hover ─────────────────────────────────────────────
    splashFactory: NoSplash.splashFactory,
    splashColor: Colors.transparent,
    highlightColor: Colors.transparent,
    hoverColor: t.hover,
    focusColor: t.hover,
    pageTransitionsTheme: const PageTransitionsTheme(
      builders: {
        TargetPlatform.macOS: _NoTransition(),
        TargetPlatform.windows: _NoTransition(),
        TargetPlatform.linux: _NoTransition(),
      },
    ),

    // ── components ──────────────────────────────────────────────────────
    dividerTheme: DividerThemeData(color: t.line, thickness: 1, space: 1),
    iconTheme: IconThemeData(size: t.icon, color: t.textSecondary),
    primaryIconTheme: IconThemeData(size: t.icon, color: t.text),
    scrollbarTheme: ScrollbarThemeData(
      thickness: const WidgetStatePropertyAll(8),
      radius: const Radius.circular(4),
      thumbColor: WidgetStatePropertyAll(t.textDisabled.withValues(alpha: .5)),
      crossAxisMargin: 1,
    ),
    textSelectionTheme: TextSelectionThemeData(
      cursorColor: t.accent,
      selectionColor: t.selection.withValues(alpha: .5),
      selectionHandleColor: t.accent,
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: t.field, // recessed, not raised
      isDense: true,
      contentPadding: EdgeInsets.symmetric(horizontal: 6, vertical: (t.row - 12) / 2),
      border: border,
      enabledBorder: border,
      disabledBorder: OutlineInputBorder(
        borderRadius: t.cornerControl,
        borderSide: BorderSide(color: t.line),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: t.cornerControl,
        borderSide: BorderSide(color: t.accent, width: 1),
      ),
      errorBorder: OutlineInputBorder(
        borderRadius: t.cornerControl,
        borderSide: BorderSide(color: t.error),
      ),
      focusedErrorBorder: OutlineInputBorder(
        borderRadius: t.cornerControl,
        borderSide: BorderSide(color: t.error),
      ),
      hintStyle: text.bodySmall?.copyWith(color: t.textDisabled),
      labelStyle: text.bodySmall,
      helperStyle: text.bodySmall,
      errorStyle: text.bodySmall?.copyWith(color: t.error),
    ),
    textButtonTheme: TextButtonThemeData(
      style: ButtonStyle(
        minimumSize: WidgetStatePropertyAll(Size(0, t.row + 4)),
        padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 10)),
        foregroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.disabled) ? t.textDisabled : t.accent,
        ),
        overlayColor: WidgetStatePropertyAll(t.hover),
        textStyle: WidgetStatePropertyAll(text.labelLarge),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: t.cornerControl)),
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: ButtonStyle(
        minimumSize: WidgetStatePropertyAll(Size(0, t.row + 6)),
        padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 12)),
        backgroundColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.disabled) ? t.line : t.accent),
        foregroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.disabled) ? t.textDisabled : t.onAccent,
        ),
        textStyle: WidgetStatePropertyAll(text.labelLarge),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: t.cornerControl)),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: ButtonStyle(
        minimumSize: WidgetStatePropertyAll(Size(0, t.row + 4)),
        padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 10)),
        foregroundColor: WidgetStatePropertyAll(t.text),
        side: WidgetStatePropertyAll(BorderSide(color: t.lineStrong)),
        overlayColor: WidgetStatePropertyAll(t.hover),
        textStyle: WidgetStatePropertyAll(text.labelLarge),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: t.cornerControl)),
      ),
    ),
    iconButtonTheme: IconButtonThemeData(
      style: ButtonStyle(
        iconSize: WidgetStatePropertyAll(t.icon),
        minimumSize: WidgetStatePropertyAll(Size.square(t.row + 4)),
        maximumSize: WidgetStatePropertyAll(Size.square(t.row + 8)),
        padding: const WidgetStatePropertyAll(EdgeInsets.all(2)),
        foregroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.disabled) ? t.textDisabled : t.textSecondary,
        ),
        overlayColor: WidgetStatePropertyAll(t.hover),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: t.cornerControl)),
      ),
    ),
    segmentedButtonTheme: SegmentedButtonThemeData(
      style: ButtonStyle(
        minimumSize: WidgetStatePropertyAll(Size(0, t.row + 4)),
        padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(horizontal: 10)),
        textStyle: WidgetStatePropertyAll(text.labelLarge),
        // An on segment is a lighter grey, Unity's pressed toggle: blue is
        // for the selection and focus only (docs/ui-refresh).
        backgroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.selected) ? t.selectionQuiet : Colors.transparent,
        ),
        foregroundColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.selected) ? t.text : t.textSecondary,
        ),
        side: WidgetStatePropertyAll(BorderSide(color: t.lineStrong)),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: t.cornerControl)),
      ),
    ),
    checkboxTheme: CheckboxThemeData(
      materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
      visualDensity: VisualDensity.compact,
      side: BorderSide(color: t.lineStrong, width: 1),
      fillColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? t.accent : t.field),
      checkColor: WidgetStatePropertyAll(t.onAccent),
      overlayColor: WidgetStatePropertyAll(t.hover),
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(2)),
    ),
    switchTheme: SwitchThemeData(
      materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
      trackOutlineColor: WidgetStatePropertyAll(t.lineStrong),
      trackColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? t.accent : t.field),
      thumbColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.selected) ? t.onAccent : t.textSecondary,
      ),
    ),
    radioTheme: RadioThemeData(
      materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
      visualDensity: VisualDensity.compact,
      fillColor: WidgetStatePropertyAll(t.accent),
    ),
    // Unity's slider: a grey track and a small light thumb — no blue.
    sliderTheme: SliderThemeData(
      activeTrackColor: t.textDisabled,
      inactiveTrackColor: t.field,
      thumbColor: t.text,
      thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 5, disabledThumbRadius: 4, elevation: 0),
      overlayShape: SliderComponentShape.noOverlay,
      trackHeight: 2,
      tickMarkShape: SliderTickMarkShape.noTickMark,
    ),
    listTileTheme: ListTileThemeData(
      dense: true,
      minVerticalPadding: 0,
      horizontalTitleGap: t.gap,
      contentPadding: EdgeInsets.symmetric(horizontal: t.pad),
      titleTextStyle: text.bodyMedium,
      subtitleTextStyle: text.bodySmall,
      iconColor: t.textSecondary,
      selectedColor: t.text,
      selectedTileColor: t.selectionQuiet,
      shape: RoundedRectangleBorder(borderRadius: t.cornerControl),
    ),
    tooltipTheme: TooltipThemeData(
      waitDuration: const Duration(milliseconds: 500),
      showDuration: const Duration(milliseconds: 1500),
      preferBelow: true,
      textStyle: text.bodySmall?.copyWith(color: t.text),
      decoration: BoxDecoration(
        color: t.popup,
        border: Border.all(color: t.lineStrong),
        borderRadius: t.cornerPanel,
        boxShadow: [BoxShadow(color: t.shadow, blurRadius: 12, offset: const Offset(0, 4))],
      ),
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
    ),
    popupMenuTheme: PopupMenuThemeData(
      color: t.popup,
      surfaceTintColor: Colors.transparent,
      elevation: 8,
      shadowColor: t.shadow,
      textStyle: text.bodyMedium,
      labelTextStyle: WidgetStatePropertyAll(text.bodyMedium),
      shape: RoundedRectangleBorder(
        borderRadius: t.cornerPanel,
        side: BorderSide(color: t.lineStrong),
      ),
    ),
    menuTheme: MenuThemeData(
      style: MenuStyle(
        backgroundColor: WidgetStatePropertyAll(t.popup),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
        shadowColor: WidgetStatePropertyAll(t.shadow),
        shape: WidgetStatePropertyAll(
          RoundedRectangleBorder(
            borderRadius: t.cornerPanel,
            side: BorderSide(color: t.lineStrong),
          ),
        ),
      ),
    ),
    menuButtonTheme: MenuButtonThemeData(
      style: ButtonStyle(
        minimumSize: WidgetStatePropertyAll(Size(0, t.row + 4)),
        textStyle: WidgetStatePropertyAll(text.bodyMedium),
        foregroundColor: WidgetStatePropertyAll(t.text),
        overlayColor: WidgetStatePropertyAll(t.hover),
        shape: WidgetStatePropertyAll(RoundedRectangleBorder(borderRadius: t.cornerControl)),
      ),
    ),
    dropdownMenuTheme: DropdownMenuThemeData(
      textStyle: text.bodyMedium,
      menuStyle: MenuStyle(
        backgroundColor: WidgetStatePropertyAll(t.popup),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
        shape: WidgetStatePropertyAll(
          RoundedRectangleBorder(
            borderRadius: t.cornerPanel,
            side: BorderSide(color: t.lineStrong),
          ),
        ),
      ),
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: t.popup,
      surfaceTintColor: Colors.transparent,
      elevation: 16,
      shadowColor: t.shadow,
      titleTextStyle: text.titleMedium,
      contentTextStyle: text.bodyMedium,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(6),
        side: BorderSide(color: t.lineStrong),
      ),
    ),
    bannerTheme: MaterialBannerThemeData(
      backgroundColor: t.header,
      surfaceTintColor: Colors.transparent,
      elevation: 0,
      contentTextStyle: text.bodyMedium,
      padding: EdgeInsets.symmetric(horizontal: t.pad, vertical: 4),
      dividerColor: t.line,
    ),
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.fixed,
      backgroundColor: t.popup,
      contentTextStyle: text.bodyMedium,
      actionTextColor: t.accent,
      elevation: 0,
      shape: const RoundedRectangleBorder(),
    ),
    tabBarTheme: TabBarThemeData(
      labelColor: t.text,
      unselectedLabelColor: t.textSecondary,
      labelStyle: text.labelLarge,
      unselectedLabelStyle: text.labelLarge,
      // The on tab is marked in the text's colour: blue is for the
      // selection and focus only (docs/ui-refresh).
      indicatorColor: t.textSecondary,
      indicatorSize: TabBarIndicatorSize.tab,
      dividerColor: t.line,
      overlayColor: WidgetStatePropertyAll(t.hover),
    ),
    // Flat toggles (the Console's counts, Collapse): an on chip is a
    // lighter grey, Unity's pressed toggle — no outline, no blue.
    chipTheme: ChipThemeData(
      backgroundColor: Colors.transparent,
      selectedColor: t.selectionQuiet,
      side: BorderSide.none,
      labelStyle: text.bodySmall!.copyWith(color: t.text),
      padding: const EdgeInsets.symmetric(horizontal: 2, vertical: 0),
      labelPadding: const EdgeInsets.only(left: 2, right: 4),
      shape: RoundedRectangleBorder(borderRadius: t.cornerControl),
      showCheckmark: false,
    ),
    progressIndicatorTheme: ProgressIndicatorThemeData(
      color: t.accent,
      linearTrackColor: t.line,
      circularTrackColor: t.line,
    ),
    expansionTileTheme: ExpansionTileThemeData(
      backgroundColor: t.panel,
      collapsedBackgroundColor: t.panel,
      iconColor: t.textSecondary,
      collapsedIconColor: t.textSecondary,
      textColor: t.text,
      collapsedTextColor: t.text,
      tilePadding: EdgeInsets.symmetric(horizontal: t.pad),
      childrenPadding: EdgeInsets.zero,
      expansionAnimationStyle: AnimationStyle.noAnimation,
      shape: const Border(),
      collapsedShape: const Border(),
    ),
  );
}

/// Ids, paths, numbers and raw JSON — the text the eye compares character
/// by character. Digits are tabular so columns of numbers line up.
TextStyle monoStyle(BuildContext context, {Color? color, double? size, FontWeight? weight}) {
  final t = context.tokens;
  final base = Theme.of(context).textTheme.bodySmall;
  return (base ?? const TextStyle()).copyWith(
    fontFamily: monoFontFamily,
    fontFamilyFallback: _monoFallback,
    fontSize: size ?? 11,
    fontWeight: weight,
    color: color ?? t.textSecondary,
    fontFeatures: _tabular,
  );
}

/// A route that simply appears: item 9a allows hover and press, nothing
/// else, and a dialog sliding in from nowhere is exactly the Material
/// tell the editor is getting rid of.
final class _NoTransition extends PageTransitionsBuilder {
  const _NoTransition();

  @override
  Widget buildTransitions<T>(
    PageRoute<T> route,
    BuildContext context,
    Animation<double> animation,
    Animation<double> secondaryAnimation,
    Widget child,
  ) => child;
}
