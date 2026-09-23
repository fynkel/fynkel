// The editor's design tokens (blueprint §19.2e item 9a): every colour and
// every measurement the UI uses, in one place, reached through
// `Theme.of(context).extension<FynkelTokens>()!` — or, shorter, the
// `context.tokens` getter below.
//
// The language is Unity's density, Rider's structure and Framer's finish:
// neutral greys with a slight blue bias, one accent used only for
// selection, focus and active state, diagnostics in their own hues, 1px
// hairlines, small radii, and rows a mouse can hit but an eye can scan.
// A field is *recessed* — darker than the panel it sits in, the way a
// game editor's fields are, not lighter the way Material's are.
import 'package:flutter/material.dart';

/// How tall a list row is. The user picks one in the preferences; every
/// row, field and toolbar in the editor derives its height from it, so the
/// whole app gets denser or airier together.
enum EditorDensity {
  compact(18),
  standard(20),
  comfortable(24);

  const EditorDensity(this.row);

  /// The height of one row in logical pixels.
  final double row;

  String get label => switch (this) {
    EditorDensity.compact => 'Compact',
    EditorDensity.standard => 'Default',
    EditorDensity.comfortable => 'Comfortable',
  };

  static EditorDensity parse(Object? v) => values.firstWhere((d) => d.name == v, orElse: () => standard);
}

/// The editor's palette and metrics. Built by [FynkelTokens.of] for a
/// brightness and a density; nothing in the UI names a colour that is not
/// here.
@immutable
final class FynkelTokens extends ThemeExtension<FynkelTokens> {
  const FynkelTokens({
    required this.brightness,
    required this.density,
    required this.window,
    required this.panel,
    required this.header,
    required this.field,
    required this.popup,
    required this.line,
    required this.lineStrong,
    required this.text,
    required this.textSecondary,
    required this.textDisabled,
    required this.accent,
    required this.onAccent,
    required this.selection,
    required this.onSelection,
    required this.selectionQuiet,
    required this.accentSoft,
    required this.hover,
    required this.error,
    required this.warning,
    required this.ok,
    required this.info,
    required this.shadow,
    required this.overlay,
    required this.overlayEdge,
    required this.axisX,
    required this.axisY,
    required this.axisZ,
  });

  final Brightness brightness;
  final EditorDensity density;

  /// Behind the panels; a gutter of it separates them.
  final Color window;
  final Color panel;

  /// A panel's title bar, a component header in the Inspector.
  final Color header;

  /// Text fields, number fields, dropdowns — recessed, not raised.
  final Color field;

  /// Menus, dialogs, anything floating above the window.
  final Color popup;

  /// The 1px line between things, and its stronger form on a control's edge.
  final Color line;
  final Color lineStrong;

  final Color text;
  final Color textSecondary;
  final Color textDisabled;

  /// Selection, focus and active state — and nothing else.
  final Color accent;
  final Color onAccent;
  final Color selection;
  final Color onSelection;

  /// A selected row in a panel that does not hold focus (item 9b): the
  /// accent follows focus, so only one panel's selection is ever blue.
  final Color selectionQuiet;

  /// A toggle that is on, a hovered pick button: the accent as a tint,
  /// its icon in [accent] (item 9b — on-toggles are tinted, not solid).
  final Color accentSoft;
  final Color hover;

  /// Diagnostics, deliberately their own hues: a severity must never be
  /// mistaken for a selection.
  final Color error;
  final Color warning;
  final Color ok;
  final Color info;

  final Color shadow;

  /// The Scene view's floating toolbars: translucent over the picture.
  final Color overlay;
  final Color overlayEdge;

  /// The axis letters inside a vector field, the gizmo's own hues.
  final Color axisX;
  final Color axisY;
  final Color axisZ;

  // ── metrics ───────────────────────────────────────────────────────────
  double get row => density.row;

  /// A toolbar's height, and a tab strip's.
  double get toolbar => density.row + 8;
  double get tab => density.row + 6;

  /// A text or number field: one row, minus the hairline above and below.
  double get fieldHeight => density.row;

  double get icon => 16;
  double get radius => 3;
  double get radiusPanel => 4;
  double get gutter => 4;

  /// Horizontal padding inside a panel's rows and toolbars.
  double get pad => 8;

  /// The gap between a label and its control, and between controls.
  double get gap => 6;

  /// Hover and press; the only motion in the editor.
  Duration get motion => const Duration(milliseconds: 90);

  BorderRadius get cornerControl => BorderRadius.circular(radius);
  BorderRadius get cornerPanel => BorderRadius.circular(radiusPanel);
  BorderSide get hairline => BorderSide(color: line, width: 1);

  /// A washed-out version of [c] laid over the panel: the background of a
  /// badge, a severity strip or a selected-but-unfocused row. Opaque, so it
  /// never doubles up when two of them overlap.
  Color surfaceOf(Color c, [double amount = 0.18]) => Color.alphaBlend(c.withValues(alpha: amount), panel);

  /// A selected row's fill: the accent in the panel that holds focus, a
  /// quiet tint everywhere else (item 9b).
  Color selectedRow({required bool focused}) => focused ? selection : selectionQuiet;

  /// The text on a selected row: white on the accent, the ordinary text
  /// colour on the quiet tint.
  Color onSelectedRow({required bool focused}) => focused ? onSelection : text;

  /// Secondary text on a selected row — still legible on the accent.
  Color onSelectedRowSecondary({required bool focused}) =>
      focused ? onSelection.withValues(alpha: 0.78) : textSecondary;

  /// The colour a diagnostic of this severity is drawn in.
  Color severity({required bool isError, required bool isWarning}) => isError
      ? error
      : isWarning
      ? warning
      : info;

  static const _darkPalette = _Palette(
    window: Color(0xFF17181A),
    panel: Color(0xFF1E1F22),
    header: Color(0xFF212328),
    field: Color(0xFF131416),
    popup: Color(0xFF26282D),
    line: Color(0xFF2A2C31),
    lineStrong: Color(0xFF33363B),
    text: Color(0xFFD8DADD),
    textSecondary: Color(0xFF9BA0A6),
    textDisabled: Color(0xFF6B7076),
    accent: Color(0xFF3E7BFA),
    onAccent: Color(0xFFFFFFFF),
    selection: Color(0xFF2F5AA8),
    onSelection: Color(0xFFFFFFFF),
    selectionQuiet: Color(0xFF2A3140),
    accentSoft: Color(0x333E7BFA),
    hover: Color(0x0FFFFFFF),
    error: Color(0xFFE5484D),
    warning: Color(0xFFF5A524),
    ok: Color(0xFF30A46C),
    info: Color(0xFF7A8189),
    shadow: Color(0x80000000),
    overlay: Color(0xB81E1F22),
    overlayEdge: Color(0x12FFFFFF),
    axisX: Color(0xFFE5645F),
    axisY: Color(0xFF5BBF7D),
    axisZ: Color(0xFF5B8EFA),
  );

  /// Not an inversion of the dark set: the greys are lighter and cooler,
  /// the field is the brightest surface (paper), and the accent darkens so
  /// it still reads as selected with white text on it.
  static const _lightPalette = _Palette(
    window: Color(0xFFDDDFE3),
    panel: Color(0xFFF2F3F5),
    header: Color(0xFFE8EAED),
    field: Color(0xFFFFFFFF),
    popup: Color(0xFFFAFBFC),
    line: Color(0xFFD2D5DA),
    lineStrong: Color(0xFFC2C6CC),
    text: Color(0xFF1B1D21),
    textSecondary: Color(0xFF5A6067),
    textDisabled: Color(0xFF878D95),
    accent: Color(0xFF2C5FD8),
    onAccent: Color(0xFFFFFFFF),
    selection: Color(0xFF2C5FD8),
    onSelection: Color(0xFFFFFFFF),
    selectionQuiet: Color(0xFFD4DCEC),
    accentSoft: Color(0x242C5FD8),
    hover: Color(0x0D000000),
    error: Color(0xFFC62A2F),
    warning: Color(0xFF9A6200),
    ok: Color(0xFF1C7A50),
    info: Color(0xFF6B7178),
    shadow: Color(0x33000000),
    overlay: Color(0xC7F8F9FB),
    overlayEdge: Color(0x1A000000),
    axisX: Color(0xFFC9453F),
    axisY: Color(0xFF2E8B57),
    axisZ: Color(0xFF2C5FD8),
  );

  factory FynkelTokens.of(Brightness brightness, [EditorDensity density = EditorDensity.standard]) {
    final p = brightness == Brightness.dark ? _darkPalette : _lightPalette;
    return FynkelTokens(
      brightness: brightness,
      density: density,
      window: p.window,
      panel: p.panel,
      header: p.header,
      field: p.field,
      popup: p.popup,
      line: p.line,
      lineStrong: p.lineStrong,
      text: p.text,
      textSecondary: p.textSecondary,
      textDisabled: p.textDisabled,
      accent: p.accent,
      onAccent: p.onAccent,
      selection: p.selection,
      onSelection: p.onSelection,
      selectionQuiet: p.selectionQuiet,
      accentSoft: p.accentSoft,
      hover: p.hover,
      error: p.error,
      warning: p.warning,
      ok: p.ok,
      info: p.info,
      shadow: p.shadow,
      overlay: p.overlay,
      overlayEdge: p.overlayEdge,
      axisX: p.axisX,
      axisY: p.axisY,
      axisZ: p.axisZ,
    );
  }

  @override
  FynkelTokens copyWith({Brightness? brightness, EditorDensity? density}) =>
      FynkelTokens.of(brightness ?? this.brightness, density ?? this.density);

  /// Themes do not cross-fade in this editor (item 9a: no motion but
  /// hover), so a lerp past halfway is simply the other set.
  @override
  FynkelTokens lerp(ThemeExtension<FynkelTokens>? other, double t) {
    if (other is! FynkelTokens) return this;
    return t < 0.5 ? this : other;
  }
}

/// The two palettes, written once each so [FynkelTokens.of] stays a table
/// and not two long constructors.
@immutable
final class _Palette {
  const _Palette({
    required this.window,
    required this.panel,
    required this.header,
    required this.field,
    required this.popup,
    required this.line,
    required this.lineStrong,
    required this.text,
    required this.textSecondary,
    required this.textDisabled,
    required this.accent,
    required this.onAccent,
    required this.selection,
    required this.onSelection,
    required this.selectionQuiet,
    required this.accentSoft,
    required this.hover,
    required this.error,
    required this.warning,
    required this.ok,
    required this.info,
    required this.shadow,
    required this.overlay,
    required this.overlayEdge,
    required this.axisX,
    required this.axisY,
    required this.axisZ,
  });

  final Color window, panel, header, field, popup, line, lineStrong;
  final Color text, textSecondary, textDisabled;
  final Color accent, onAccent, selection, onSelection, selectionQuiet, accentSoft, hover;
  final Color error, warning, ok, info, shadow, overlay, overlayEdge, axisX, axisY, axisZ;
}

extension FynkelTokensOfTheme on ThemeData {
  /// The editor's tokens off a [ThemeData] that is already in hand — the
  /// same set [FynkelTokensOf.tokens] returns.
  FynkelTokens get tokens => extension<FynkelTokens>() ?? FynkelTokens.of(brightness);
}

extension FynkelTokensOf on BuildContext {
  /// The editor's tokens. Every widget in `apps/editor` reads its colours
  /// and metrics from here; a literal colour in a panel is a bug.
  FynkelTokens get tokens => Theme.of(this).extension<FynkelTokens>() ?? FynkelTokens.of(Brightness.dark);
}

/// The Scene view's gizmo metrics and axis colours (ADR-0019 §4), used by
/// the view model, which has no `BuildContext`. Widths are logical pixels;
/// colours linear RGBA, as the renderer's overlay takes them.
abstract final class GizmoTokens {
  /// Annotations nobody drags: outlines, icons, light ranges. One point
  /// (two pixels on a Retina display): 1.5 read as heavy at the 9f gate.
  static const double lineWidth = 1.0;

  /// Things the user drags: the move, rotate and scale handles.
  static const double handleWidth = 2.5;

  static const List<double> xAxis = [0.90, 0.25, 0.25, 0.8];
  static const List<double> yAxis = [0.35, 0.85, 0.35, 0.8];
  static const List<double> zAxis = [0.25, 0.45, 0.95, 0.8];
}
