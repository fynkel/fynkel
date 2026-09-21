import 'package:fynkel/fynkel.dart';

/// The game's observable state (blueprint §10, ADR-0008, docs/hud.md): the
/// coins collected, the win, the clock, the deaths, the hint the HUD shows
/// next to the switch, whether the app paused. Behaviours write the fields
/// from their callbacks; the HUD's widgets bind to them (`ModelBuilder` in
/// lib/main.dart) and rebuild on change only — nothing reads the engine per
/// frame, nothing rebuilds the app per frame.
///
/// `Game.load` installs one instance in the world's context
/// (`world.context.service<GameState>()`); a restart calls [reset] before
/// the scene reloads, so the counters start over with the coins.
class GameState extends GameModel {
  /// Coins collected in this run.
  late final collected = field(ModelValue(0));

  /// Coins the scene started with (counted by the collectibles at `start`).
  late final total = field(ModelValue(0));

  /// Every coin collected.
  late final won = field(ModelValue(false));

  /// Seconds since the run started, in tenths (written from `update` only
  /// when the digit changes).
  late final elapsedTenths = field(ModelValue(0));

  /// Times the player hit a hazard or fell out of the arena.
  late final deaths = field(ModelValue(0));

  /// Footsteps taken: the walk's and the run's `step_l`/`step_r` markers
  /// as they reach `game.Player` (M3-19c).
  late final footsteps = field(ModelValue(0));

  /// A one-line prompt (`E — open the gate`), empty when there is none.
  late final hint = field(ModelValue(''));

  /// Set by the app (the `pause` action, Escape).
  late final paused = field(ModelValue(false));

  /// Back to a fresh run (the scene reload respawns the coins).
  void reset() {
    collected.value = 0;
    total.value = 0;
    won.value = false;
    elapsedTenths.value = 0;
    deaths.value = 0;
    footsteps.value = 0;
    hint.value = '';
  }

  /// One coin counted; the last one wins.
  void collect() {
    collected.value += 1;
    if (collected.value >= total.value) won.value = true;
  }
}
