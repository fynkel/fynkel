---
title: "Particles and the world"
description: "Bouncing sparks, sub emitters, trails and dust."
---

![Fireworks, dust and sparks that bounce](/shots/particles-world.webp)

These are Unity's modules of the same names.

### Emission over distance

`Emission(rateOverDistance: 7)` makes 7 a metre
its object moves: dust under feet and wheels, none standing still.

### Inherit velocity

InheritVelocity(curve:)` gives each
particle that much of its object's speed when it's made
(`InheritVelocityMode.initial`), or all through its life (`.current`); a
negative one throws dust back from the way it goes.

### Sub emitters

[SubEmitter(system, type)]`, up to 4:
another system, a child of this one, makes its burst (its own `emission`'s
bursts) where a particle is born, dies or hits something
(`SubEmitterType.birth`, `.death`, `.collision`); a birth one keeps making
its rate along the particle as it flies.

`inherit: {SubEmitterInherit.color, .size, .rotation}` passes on what the
particle had, and `probability` makes only some of them do it. A sub emitter
runs only for its parent, never on its own timeline: fireworks are a shell
with a trail on birth and a burst on death.

### Collision

ParticleCollision(planes: [floor])`, up to 6
objects, each a plane through its position facing its up: the floor, a wall.
A particle hitting one bounces (`bounce`, 0 to 1, as Unity's), loses speed
(`dampen`) and life (`lifetimeLoss`), and dies below `minKillSpeed`;
`radiusScale` sizes it for hitting.

With `sendCollisionMessages`, `onParticleCollision(plane, count)` is called
once a frame for each plane hit, with how many particles hit it (Unity's
`OnParticleCollision`). Collision with the physics world's colliders is not
in this version.

### Trails

A `TrailRenderer` is a component, as Unity's is: a strip left
behind its object as it moves, `time` seconds long, a point each
`minVertexDistance`, `widthMultiplier` wide times `widthCurve` (`(t) => 1 -
t` tapers it), coloured by `colorGradient` from the object (0) to the tail
(1), drawn with its `material` as particles are.

`emitting = false` lets it run out; `clear()` drops its points (a ball put
back on the tee); `positionCount` says how many it has. It keeps at most
512.

```dart
/// Sparks off a grinder that bounce on the floor and flash where some hit.
GameObject grinder(GameObject floor, Texture2D dot) {
  final glow = Material(baseColorTexture: dot, blending: Blending.additive);
  final flash = ParticleSystem(
    startLifetime: const Range(0.15),
    startSpeed: const Range(0),
    emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(1))]),
    material: glow,
  );
  return GameObject(
    name: 'grinder',
    components: [
      ParticleSystem(
        startSpeed: const Range(2.5, 4),
        gravityModifier: 1,
        simulationSpace: ParticleSpace.world,
        emission: const Emission(rateOverTime: 70),
        collision: ParticleCollision(
          planes: [floor],
          bounce: const ParticleCurve(0.5),
          lifetimeLoss: const ParticleCurve(0.15),
          sendCollisionMessages: true,
        ),
        subEmitters: [SubEmitter(flash, SubEmitterType.collision, probability: 0.3)],
        onParticleCollision: (plane, count) => print('$count hit ${plane.name}'),
        material: glow,
      ),
    ],
    children: [GameObject(name: 'flash', components: [flash])],
  );
}
```

`Quality.softParticles` turns soft particles on (medium and high) or off
(low, where reading the depth costs too much on a phone), as Unity's
quality setting of the same name. `world.debug.drawShapes = true` draws
each system's shape and the planes it collides with, in the debug lines,
as Unity's Scene view shows them when one is selected.

The lab's "Particles and the world" has fireworks, a rolling ball with
dust and a trail, and sparks bouncing off the floor.

Settings change while it plays, and its particles stay. A value it
can't have (a negative lifetime, a cone over 90°, more than 8 bursts)
throws an `ArgumentError` and the old one stays.
