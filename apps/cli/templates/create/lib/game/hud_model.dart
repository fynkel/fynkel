import 'package:fynkel/gameplay.dart';

/// What the HUD shows of the game, as observable fields (blueprint §10,
/// ADR-0008, docs/hud.md): behaviours write them from `update`, the
/// widgets bound to them (`ModelBuilder` in lib/main.dart) rebuild when a
/// value changes and never otherwise — nothing reads the engine snapshot
/// per frame, nothing rebuilds the app per frame.
///
/// `Game.load` installs one instance in the world's context
/// (`world.context.service<HudModel>()`); `Mover.update` fills it. Add a
/// field per fact a screen needs (`score`, `health`, `screen`), register it
/// with `field` so `notifyAll`/`dispose` reach it.
class HudModel extends GameModel {
  /// The mover's position, written when it changes.
  late final position = field(ModelValue(Vec3.zero));

  /// Jumps so far.
  late final jumps = field(ModelValue(0));

  /// In the air right now.
  late final airborne = field(ModelValue(false));

  /// Set by the app (the `pause` action, Escape).
  late final paused = field(ModelValue(false));
}
