import 'package:fynkel/fynkel.dart';

import 'game_state.dart';
import 'gate.dart';

/// A floor pad: while the player stands in its `switch` zone the HUD shows
/// a hint, and the `interact` press opens the [Gate] named [target].
@FynkelBehaviour(id: 'game.Switch')
class Switch extends Behaviour {
  /// The entity carrying the `game.Gate` to open.
  @Expose()
  String target = 'Gate';

  /// The HUD's prompt while the player stands here.
  @Expose()
  String prompt = 'E — open the gate';

  bool occupied = false;
  bool used = false;

  GameState get state => context.service<GameState>();

  @override
  void onZoneEnter(ZoneEvent event) {
    if (event.event != 'switch' || used) return;
    occupied = true;
    state.hint.value = prompt;
  }

  @override
  void onZoneExit(ZoneEvent event) {
    if (event.event != 'switch') return;
    occupied = false;
    if (state.hint.value == prompt) state.hint.value = '';
  }

  @override
  void fixedUpdate(FixedUpdateContext context) {
    if (!occupied || used || !Input.pressed('interact')) return;
    final gate = world.find(target)?.behaviours.whereType<Gate>().firstOrNull;
    if (gate == null) return;
    gate.open();
    used = true;
    occupied = false;
    state.hint.value = '';
  }
}
