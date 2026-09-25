import 'package:fynkel/fynkel.dart';

/// Which screen is up: the Menu, the level or the Results. The app's
/// overlay switches on it (lib/main.dart).
enum Screen { menu, playing, results }

/// A finished run, as the Results screen shows it.
class RunResult {
  const RunResult({required this.collected, required this.total, required this.elapsedTenths, required this.deaths});

  final int collected;
  final int total;
  final int elapsedTenths;
  final int deaths;

  /// Every coin taken.
  bool get won => total > 0 && collected >= total;

  /// The clock as the HUD shows it: `mm:ss.t`.
  String get clock => formatTenths(elapsedTenths);
}

/// `mm:ss.t` from a count of tenths.
String formatTenths(int t) =>
    '${(t ~/ 600).toString().padLeft(2, '0')}:${((t ~/ 10) % 60).toString().padLeft(2, '0')}.${t % 10}';

/// The game's observable state (docs/hud.md): behaviours write the fields
/// from their callbacks, the HUD's widgets bind to them and rebuild on
/// change only. `Game.load` installs one in the world's context.
class GameState extends GameModel {
  late final screen = field(ModelValue(Screen.menu));

  /// Coins collected in this run.
  late final collected = field(ModelValue(0));

  /// Coins the level started with (each `Collectible` counts itself).
  late final total = field(ModelValue(0));

  /// Every coin collected.
  late final won = field(ModelValue(false));

  /// Time since the run started, in tenths of a second.
  late final elapsedTenths = field(ModelValue(0));

  /// Times the player hit the sweeper or fell off.
  late final deaths = field(ModelValue(0));

  /// A one-line prompt (`E — open the gate`), empty when there is none.
  late final hint = field(ModelValue(''));

  /// Set by the app (the `pause` action).
  late final paused = field(ModelValue(false));

  /// The last run's numbers, for the Results screen.
  late final lastRun = field(ModelValue<RunResult?>(null));

  /// A fresh run.
  void reset() {
    collected.value = 0;
    total.value = 0;
    won.value = false;
    elapsedTenths.value = 0;
    deaths.value = 0;
    hint.value = '';
  }

  /// One coin counted; the last one wins.
  void collect() {
    collected.value += 1;
    if (collected.value >= total.value) won.value = true;
  }

  /// The run is over: its numbers into [lastRun].
  void finish() {
    lastRun.value = RunResult(
      collected: collected.value,
      total: total.value,
      elapsedTenths: elapsedTenths.value,
      deaths: deaths.value,
    );
  }
}
