import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart' show Brightness, ThemeData;
import 'package:fynkel/fynkel.dart' show Signal, World;

/// A [World]'s state as Flutter listenables, for widgets beside the game.
extension WorldListenables on World {
  /// [World.paused] as a [ValueListenable], for a pause panel that shows
  /// itself: `ValueListenableBuilder(valueListenable:
  /// game.world.pausedListenable, ...)`. The same one each time it's asked
  /// for.
  ValueListenable<bool> get pausedListenable => _paused[this] ??= _pausedOf(this);
}

final _paused = Expando<ValueNotifier<bool>>('pausedListenable');

ValueNotifier<bool> _pausedOf(World world) {
  final notifier = ValueNotifier(world.paused);
  world.onPausedChanged.listen((paused) => notifier.value = paused);
  return notifier;
}

/// The app's theme in the game, so the game can wear the app's colours.
extension WorldTheme on World {
  /// The app's [ThemeData] where the game is shown: `Theme.of(context)` of
  /// its `FynkelGame`, or of a `FynkelView` that makes its own world. It's
  /// there before the root is added, so `build` can read it:
  /// `camera.clearColor = world.theme.colorScheme.surface.toFynkel()`.
  /// Flutter's default (light) theme until a widget sets it, or a test
  /// does.
  ThemeData get theme => _themes[this]?.theme ?? _plain;

  /// Sets [theme], telling [onThemeChanged] if it's a different one.
  set theme(ThemeData value) {
    final held = _themes[this] ??= _Theme(value);
    if (held.theme == value) return;
    held.theme = value;
    held.changed.emit(value);
  }

  /// Told the new [theme] when it changes: the app going dark, say. As
  /// the app fades from one theme to the next, it's told each step of the
  /// way, so what follows it fades too. Heard while the app builds: change
  /// the game here, and widgets through listenables.
  Signal<ThemeData> get onThemeChanged => (_themes[this] ??= _Theme(null)).changed;

  /// Whether [theme] is dark.
  bool get isDark => theme.brightness == Brightness.dark;
}

final _themes = Expando<_Theme>('theme');
final _plain = ThemeData();

class _Theme {
  _Theme(this.theme);
  ThemeData? theme;
  final changed = Signal<ThemeData>();
}
