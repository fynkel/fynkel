import 'package:fynkel/fynkel.dart' show World;

/// The game running in the editor's Play (ADR-0028 §4): what a tool reads
/// and changes while it plays — Unity's editor code on the play-mode
/// scene. [EditorContext.play] gives it during Play, null otherwise.
///
/// Changes follow Live Play mode (ADR-0021): they are the running game's
/// alone and go when Play stops, unless the user applies them to the scene
/// (the Inspector's Apply to Scene). The Hierarchy and the Inspector show
/// them as the game makes them.
abstract interface class EditorPlay {
  /// The running game's world (the first, when its views show several):
  /// find objects, get their scripts, call their methods, as game code
  /// does. A change made while paused is drawn at once.
  World get world;

  /// Every world the game's views show, in the order they appeared.
  List<World> get worlds;

  /// Whether the game is paused (the toolbar's Pause).
  bool get isPaused;

  /// Pauses the game, as the toolbar's Pause does.
  void pause();

  /// Runs it again.
  void resume();

  /// One frame of the game's fixed step (pausing it first).
  void step();

  /// Ends Play, as the toolbar's Stop does.
  void stop();
}
