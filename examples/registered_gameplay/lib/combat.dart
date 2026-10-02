import 'package:fynkel/fynkel.dart';

import 'generated/behaviours.g.dart';

@FynkelComponent(id: 'game.Health')
class Health {
  @Expose()
  double hp = 100;
  @Expose()
  bool invulnerable = false;
  @Expose()
  GameObject? target;
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
