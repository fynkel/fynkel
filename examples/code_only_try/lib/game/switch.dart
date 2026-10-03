import 'package:fynkel/gameplay.dart';

import 'flow.dart';
import 'game_state.dart';
import 'gate.dart';

/// A floor pad: while the player stands in its `switch` zone the HUD shows
/// [prompt], and the `interact` press opens the [Gate] on [target].
@FynkelBehaviour(id: 'game.Switch')
class Switch extends Behaviour {
  /// The object carrying the `game.Gate` to open.
  @Expose()
  @Gizmo.lineTo()
  GameObject? target;

  /// The HUD's prompt while the player stands here.
  @Expose()
  String prompt = 'E — open the gate';

  bool occupied = false;
  bool used = false;

  GameState get state => context.service<GameState>();

  @override
  void onZoneEnter(ZoneEvent event) {
    if (event.event != 'switch' || used || !isPlayer(world, event.other)) return;
    occupied = true;
    state.hint.value = prompt;
  }

  @override
  void onZoneExit(ZoneEvent event) {
    if (event.event != 'switch' || !isPlayer(world, event.other)) return;
    occupied = false;
    if (state.hint.value == prompt) state.hint.value = '';
  }

  @override
  void fixedUpdate(FixedUpdateContext context) {
    if (!occupied || used || !Input.pressed('interact')) return;
    final gate = target?.behaviour<Gate>();
    if (gate == null) return;
    gate.open();
    used = true;
    occupied = false;
    state.hint.value = '';
  }
}
