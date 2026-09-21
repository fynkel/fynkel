import 'package:fynkel/fynkel.dart';

/// Which of the game's scenes is up (M3-15): the Menu, the Playground or
/// the Results. `Game` sets it from the scene manager as each load lands;
/// the app's overlay switches on it (lib/main.dart).
enum Screen { menu, playing, results }

/// A finished run, as the Results screen shows it: taken from the
/// counters by [GameState.finish] the moment the Playground is left.
class RunResult {
  const RunResult({
    required this.collected,
    required this.total,
    required this.elapsedTenths,
    required this.deaths,
    required this.footsteps,
  });

  final int collected;
  final int total;
  final int elapsedTenths;
  final int deaths;
  final int footsteps;

  /// Every coin taken.
  bool get won => total > 0 && collected >= total;

  /// The clock as the HUD shows it: `mm:ss.t`.
  String get clock => formatTenths(elapsedTenths);
}

/// `mm:ss.t` from a count of tenths.
String formatTenths(int t) =>
    '${(t ~/ 600).toString().padLeft(2, '0')}:${((t ~/ 10) % 60).toString().padLeft(2, '0')}.${t % 10}';

/// The game's observable state (blueprint §10, ADR-0008, docs/hud.md): the
/// screen, the coins collected, the win, the clock, the deaths, the hint the
/// HUD shows next to the switch, whether the app paused, the last run's
/// numbers. Behaviours write the fields from their callbacks; the HUD's
/// widgets bind to them (`ModelBuilder` in lib/ui) and rebuild on change
/// only — nothing reads the engine per frame, nothing rebuilds the app per
/// frame.
///
/// `Game.load` installs one instance in the world's context
/// (`world.context.service<GameState>()`); a new run calls [reset] before
/// the scene reloads, so the counters start over with the coins.
class GameState extends GameModel {
  /// The scene that is up: the overlay's switch (M3-15).
  late final screen = field(ModelValue(Screen.menu));

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

  /// The last run's numbers, for the Results screen; null before a run
  /// ended.
  late final lastRun = field(ModelValue<RunResult?>(null));

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

  /// The run is over: its numbers into [lastRun] (the Results screen reads
  /// them after the Playground is gone).
  void finish() {
    lastRun.value = RunResult(
      collected: collected.value,
      total: total.value,
      elapsedTenths: elapsedTenths.value,
      deaths: deaths.value,
      footsteps: footsteps.value,
    );
  }
}
