import 'package:fynkel/fynkel.dart';

import 'generated/behaviours.g.dart';

class HealthBase {
  @Expose()
  double hp = 1;
}

mixin OlderHealthDefaults {
  @Expose()
  double hp = 50;
}

mixin HealthDefaults {
  @Expose()
  double hp = 100;
  @Expose()
  bool invulnerable = true;
  @Expose()
  GameObject? target;
}

// Class fields win over mixins; the rightmost mixin wins over earlier mixins
// and the superclass. Exposed mixin references must serialize too.
@FynkelComponent(id: 'game.Health')
class Health extends HealthBase with OlderHealthDefaults, HealthDefaults {
  @override
  @Expose()
  bool invulnerable = false;
}

// Effective callback discovery includes mixins, not just methods on the class.
mixin DoorLogic on Behaviour {
  GameObject? get player;
  @override
  void fixedUpdate(FixedUpdateContext context) {
    if ((player?.readData(gameHealthType)?.hp ?? 100) <= 0)
      gameObject.name = 'Unlocked';
  }
}

@FynkelBehaviour(id: 'game.Door')
class Door extends Behaviour with DoorLogic {
  @override
  @Expose()
  GameObject? player;
}

void registerCombat(World world) {
  registerComponents(world);
  registerBehaviours(world.registry);
  world.registerSystems([
    GameplaySystem(
      id: 'game.Damage',
      phase: GameplayPhase.fixedUpdate,
      before: ['fynkel.behaviours.fixedUpdate'],
      reads: ['game.Health'],
      writes: ['game.Health'],
      run: (context) => world.updateComponents(gameHealthType, (_, value) {
        if (!value.invulnerable) value.hp = (value.hp - 10).clamp(0, 100);
        return value;
      }),
    ),
  ]);
}
