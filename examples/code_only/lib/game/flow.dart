// What the behaviours need of the game's flow, apart from how the places
// are made (lib/game/game.dart): tool/scenes.dart compiles the behaviours
// without the scene constants it is about to write.

import 'package:fynkel/fynkel.dart';

import 'game_state.dart';

/// Seconds the win banner stays before the Results.
const winDelay = 1.5;

/// The project's tags: the Player carries [playerTag], every coin
/// [pickupTag].
const playerTag = 'player';
const pickupTag = 'pickup';

/// Whether [entity] (a zone event's `other`) is the player.
bool isPlayer(World world, int entity) => world.findWithTag(playerTag)?.entity == entity;

/// Moves the game between its places; installed in the world's context so
/// behaviours can end or restart the run (`context.service<Flow>()`).
/// Each call works from inside a tick.
abstract class Flow {
  Flow(this.world);

  final World world;

  GameState get state => world.context.service<GameState>();

  /// A fresh run: the counters reset, the level from the start.
  void play() {
    state.reset();
    show(Screen.playing);
  }

  /// The run is over: its numbers kept, the Results.
  void finish() {
    state.finish();
    show(Screen.results);
  }

  /// Back to the title.
  void toMenu() {
    state.reset();
    show(Screen.menu);
  }

  void show(Screen screen);
}
