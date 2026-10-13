part of 'world.dart';

/// A key on the keyboard, by where it is rather than what it types (W is
/// where W is on a US keyboard, Z on a French one), so WASD works on every
/// layout, as Unity's Input System's `Key`. package:fynkel_flutter fills
/// them from Flutter's `PhysicalKeyboardKey`s. Named `KeyCode`, as
/// Unity's older one, so it doesn't clash with Flutter's `Key`.
///
/// The values have Flutter's (and the W3C's) names, not Unity's:
/// `shiftLeft`, not `LeftShift`; `arrowUp`, not `UpArrow`; `bracketLeft`,
/// not `LeftBracket`; `digit1`, not `Alpha1`.
enum KeyCode implements InputButton {
  /// The A key, where it is on a US keyboard.
  a,

  /// The B key, where it is on a US keyboard.
  b,

  /// The C key, where it is on a US keyboard.
  c,

  /// The D key, where it is on a US keyboard.
  d,

  /// The E key, where it is on a US keyboard.
  e,

  /// The F key, where it is on a US keyboard.
  f,

  /// The G key, where it is on a US keyboard.
  g,

  /// The H key, where it is on a US keyboard.
  h,

  /// The I key, where it is on a US keyboard.
  i,

  /// The J key, where it is on a US keyboard.
  j,

  /// The K key, where it is on a US keyboard.
  k,

  /// The L key, where it is on a US keyboard.
  l,

  /// The M key, where it is on a US keyboard.
  m,

  /// The N key, where it is on a US keyboard.
  n,

  /// The O key, where it is on a US keyboard.
  o,

  /// The P key, where it is on a US keyboard.
  p,

  /// The Q key, where it is on a US keyboard.
  q,

  /// The R key, where it is on a US keyboard.
  r,

  /// The S key, where it is on a US keyboard.
  s,

  /// The T key, where it is on a US keyboard.
  t,

  /// The U key, where it is on a US keyboard.
  u,

  /// The V key, where it is on a US keyboard.
  v,

  /// The W key, where it is on a US keyboard.
  w,

  /// The X key, where it is on a US keyboard.
  x,

  /// The Y key, where it is on a US keyboard.
  y,

  /// The Z key, where it is on a US keyboard.
  z,

  /// The 0 key above the letters.
  digit0,

  /// The 1 key above the letters.
  digit1,

  /// The 2 key above the letters.
  digit2,

  /// The 3 key above the letters.
  digit3,

  /// The 4 key above the letters.
  digit4,

  /// The 5 key above the letters.
  digit5,

  /// The 6 key above the letters.
  digit6,

  /// The 7 key above the letters.
  digit7,

  /// The 8 key above the letters.
  digit8,

  /// The 9 key above the letters.
  digit9,

  /// The F1 key.
  f1,

  /// The F2 key.
  f2,

  /// The F3 key.
  f3,

  /// The F4 key.
  f4,

  /// The F5 key.
  f5,

  /// The F6 key.
  f6,

  /// The F7 key.
  f7,

  /// The F8 key.
  f8,

  /// The F9 key.
  f9,

  /// The F10 key.
  f10,

  /// The F11 key.
  f11,

  /// The F12 key.
  f12,

  /// The space bar.
  space,

  /// The Enter (Return) key.
  enter,

  /// The Escape key.
  escape,

  /// The Tab key.
  tab,

  /// The Backspace key.
  backspace,

  /// The Delete key (forward delete).
  delete,

  /// The Insert key.
  insert,

  /// The Home key.
  home,

  /// The End key.
  end,

  /// The Page Up key.
  pageUp,

  /// The Page Down key.
  pageDown,

  /// The up arrow key.
  arrowUp,

  /// The down arrow key.
  arrowDown,

  /// The left arrow key.
  arrowLeft,

  /// The right arrow key.
  arrowRight,

  /// The left Shift key.
  shiftLeft,

  /// The right Shift key.
  shiftRight,

  /// The left Control key.
  controlLeft,

  /// The right Control key.
  controlRight,

  /// The left Alt key (Option on a Mac).
  altLeft,

  /// The right Alt key (Option on a Mac).
  altRight,

  /// The left Command key on a Mac, Windows key on a PC.
  metaLeft,

  /// The right Command key on a Mac, Windows key on a PC.
  metaRight,

  /// The Caps Lock key.
  capsLock,

  /// The `-` key, right of 0 on a US keyboard.
  minus,

  /// The `=` key, left of Backspace on a US keyboard.
  equal,

  /// The `[` key on a US keyboard.
  bracketLeft,

  /// The `]` key on a US keyboard.
  bracketRight,

  /// The `\` key on a US keyboard.
  backslash,

  /// The `;` key on a US keyboard.
  semicolon,

  /// The `'` key on a US keyboard.
  quote,

  /// The backquote (`` ` ``) key, left of 1 on a US keyboard.
  backquote,

  /// The `,` key on a US keyboard.
  comma,

  /// The `.` key on a US keyboard.
  period,

  /// The `/` key on a US keyboard.
  slash,

  /// The 0 key on the number pad.
  numpad0,

  /// The 1 key on the number pad.
  numpad1,

  /// The 2 key on the number pad.
  numpad2,

  /// The 3 key on the number pad.
  numpad3,

  /// The 4 key on the number pad.
  numpad4,

  /// The 5 key on the number pad.
  numpad5,

  /// The 6 key on the number pad.
  numpad6,

  /// The 7 key on the number pad.
  numpad7,

  /// The 8 key on the number pad.
  numpad8,

  /// The 9 key on the number pad.
  numpad9,

  /// The `+` key on the number pad.
  numpadAdd,

  /// The `-` key on the number pad.
  numpadSubtract,

  /// The `*` key on the number pad.
  numpadMultiply,

  /// The `/` key on the number pad.
  numpadDivide,

  /// The `.` key on the number pad.
  numpadDecimal,

  /// The Enter key on the number pad.
  numpadEnter;

  /// A short name to show the player, as on a US keyboard: `W`, `1`,
  /// `Space`, `↑`, `Left Shift`, `Num 5`. For a controls page.
  String get label => switch (this) {
    arrowUp => '↑',
    arrowDown => '↓',
    arrowLeft => '←',
    arrowRight => '→',
    minus => '-',
    equal => '=',
    bracketLeft => '[',
    bracketRight => ']',
    backslash => r'\',
    semicolon => ';',
    quote => "'",
    backquote => '`',
    comma => ',',
    period => '.',
    slash => '/',
    numpadAdd => 'Num +',
    numpadSubtract => 'Num -',
    numpadMultiply => 'Num *',
    numpadDivide => 'Num /',
    numpadDecimal => 'Num .',
    numpadEnter => 'Num Enter',
    escape => 'Esc',
    _ when name.length == 1 || (name.startsWith('f') && int.tryParse(name.substring(1)) != null) => name.toUpperCase(),
    _ when name.startsWith('digit') => name.substring(5),
    _ when name.startsWith('numpad') => 'Num ${name.substring(6)}',
    _ when name.endsWith('Left') => 'Left ${_words(name.substring(0, name.length - 4))}',
    _ when name.endsWith('Right') => 'Right ${_words(name.substring(0, name.length - 5))}',
    _ => _words(name),
  };

  // pageUp → Page Up.
  static String _words(String name) => name
      .replaceAllMapped(RegExp('[A-Z]'), (m) => ' ${m[0]}')
      .replaceFirstMapped(RegExp('^.'), (m) => m[0]!.toUpperCase());
}
