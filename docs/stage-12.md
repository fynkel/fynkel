# Stage 12 plan: particles

Status: accepted 2026-10-23 ("let's go with how Unity works"), with the
recommended answer to each decision (§7). Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).
Stage 11 is done ([report](stage-11-report.md)).

A Fynkel game today has no smoke, dust, sparks, fire, rain or bursts of
light. A game can fake a few with objects and tweens, but a puff of
dust is fifty little quads that each fade, grow and drift, and that
isn't something to write a class for. Every game needs them: a burst
when a coin is picked up, dust under running feet, sparks when pins are
hit. Without them a game looks still, however good its light. Stage 12
adds **particles, as Unity's Particle System has them**: a component
on an object that emits, with Unity's modules (emission, shape, colour
and size over a particle's life) and its names, simulated natively and
drawn in one call per system.

**The goal.** An effect takes a few lines, and a game uses it as it
would in Unity:

```dart
class Dog extends GameObject {
  late final ParticleSystem _dust;

  @override
  Future<void> onLoad() async {
    final smoke = await assets.texture('particles/smoke.png');
    // Dust kicked up behind the dog: more the further it runs.
    _dust = addComponent(ParticleSystem(
      startLifetime: const Range(0.6, 1.0),
      startSpeed: const Range(0.2, 0.6),
      startSize: const Range(0.3, 0.5),
      startColor: const Color(0xFFC8B48A),
      gravityModifier: -0.05,
      emission: Emission(rateOverTime: 0, rateOverDistance: 6),
      shape: ParticleShape.hemisphere(radius: 0.3),
      sizeOverLifetime: (t) => 0.6 + t, // any function of 0..1: Ease.outQuad, a Flutter curve
      colorOverLifetime: ColorGradient.fadeOut(),
      material: Material(baseColorTexture: smoke, blending: Blending.transparent),
    ));
  }
}

class Coin extends GameObject {
  void pickedUp() {
    // A burst where the coin was, which goes when its particles have.
    world.add(GameObject(name: 'sparkle', transform: Transform(position: position), components: [
      ParticleSystem(looping: false, stopAction: StopAction.destroy, startColor: const Color(0xFFFFD34D),
          emission: Emission(rateOverTime: 0, bursts: [Burst(count: 30)]),
          material: Material(baseColorTexture: _spark, blending: Blending.additive)),
    ]));
    destroy();
  }
}
```

**The road after it**: stage 13, animation 2 cut down to 2D blends,
look-at and foot IK; then stage 14, shipping = 1.0. (Shipping was
stage 12 until particles came before it, agreed 2026-10-21.)

## 1. What there is today

- **No particles**, no sprites, no vertex colours, and no curve or
  gradient type.
- **Blending is there** (stage 8): transparent and additive materials,
  drawn after everything solid, sorted furthest first by their bounds,
  with fog. Stage 8 made additive "ready for particles later".
- **Many of one thing is one draw.** Runs of the same mesh and material
  are one instanced call, with per-instance data (a matrix, colours, a
  material's own numbers).
- **A material's own shader** (stage 10) and **what's behind** (stage 10
  phase 3): a surface can read the scene's depth, which is what soft
  particles need. It costs passes on the Note10+ (stage 10's report).
- **The world lives in Rust.** Objects, their matrices, the animator:
  Dart sets them and the engine steps them. Particles can step the same
  way, reading where their object is without Dart doing anything each
  frame.
- **Eases are plain functions** (`EaseFunction`, `Ease.outQuad`, and
  Flutter's curves through `.transform`), which particles can take for
  their curves.
- **A frame's buffer** is how debug lines and sound positions already go
  to the engine in one call.
- **Stage 3's advice** for "many of a thing" (bullets, particles): one
  manager that moves plain objects. This stage is that manager, in the
  engine.

Fynkel 1 never had particles, so there's nothing to learn from it here.

## 2. Unity's way, and Fynkel's

| In Unity | In Fynkel after stage 12 |
|---|---|
| The Particle System component, built in. | `ParticleSystem`, a component in the core: no package to add. A world without one pays nothing. |
| Its **main module**: `duration`, `looping`, `prewarm`, `startDelay`, `startLifetime`, `startSpeed`, `startSize`, `startRotation`, `startColor`, `gravityModifier`, `simulationSpace` (local, world), `simulationSpeed`, `useUnscaledTime`, `playOnAwake`, `maxParticles` (1000), `stopAction` (none, disable, destroy, callback), `randomSeed`. | The same names, as the system's own fields. |
| `MinMaxCurve`: a constant, a curve, random between two constants or two curves. | A number, `Range(a, b)`, a function of 0..1, or `Range.curves(a, b)`. A function is sampled into 64 steps when it's set, never called each frame. |
| `MinMaxGradient`: a colour, a gradient, random between two. | A `Color`, a `ColorGradient` (Flutter already has `Gradient`), or `ColorRange(a, b)`. |
| **Emission**: rate over time, rate over distance, bursts (time, count, cycles, interval, probability). | `Emission(rateOverTime:, rateOverDistance:, bursts: [Burst(...)])`. |
| **Shape**: sphere, hemisphere, cone, box, circle, edge, mesh and more; random direction, spherize. | `ParticleShape.sphere`, `.hemisphere`, `.cone`, `.box`, `.circle`, `.edge`. Not a mesh's surface (§5). |
| Velocity, limit velocity (drag), force, colour, size and rotation **over lifetime**, and **by speed**. | The same, over lifetime; by speed for size and colour. |
| **Noise**. | `noise: ParticleNoise(strength:, frequency:, scrollSpeed:)`. |
| **Collision** with planes or the world, bounce, dampen, lifetime loss; `OnParticleCollision`. | Planes only (§7, decision 4), with bounce, dampen and lifetime loss; `onParticleCollision` with a count. |
| **Sub emitters** on birth, death, collision. | The same: another system emits where a particle is born, dies or hits. |
| **Texture sheet animation**: a grid of frames over lifetime. | `textureSheet: TextureSheet(columns:, rows:, ...)`. |
| **Renderer**: billboard, stretched, horizontal, vertical, mesh; a material; sort mode; soft particles in the material. | `renderMode: ParticleRenderMode.billboard` (and the others); `material:`; sorted furthest first when it blends; `softness:` on the system. |
| `Play`, `Stop` (stop emitting, or stop and clear), `Pause`, `Clear`, `Emit(count)`, `Simulate(t)`, `isPlaying`, `particleCount`. | The same, `play()`, `stop(clear:)`, `pause()`, `clear()`, `emit(count)`, `simulate(seconds)`. |
| The `TrailRenderer` component. | `TrailRenderer`, the same (§7, decision 5). |
| Lights, trails per particle, external forces, triggers, custom data. | Not in this stage (§5). |

## 3. Design decisions

### 3.1 Simulated in Rust, set from Dart

- **Dart sets, Rust steps.** A system's settings go to the engine when
  they're set or changed, in one call, not each frame. Each frame the
  world steps every playing system in one native call, after the
  animator and before drawing, as it does for animation. A frame where
  Dart changes nothing makes no particle call at all.
- **Each system is a fixed pool** of `maxParticles`, kept as arrays (all
  positions, then all velocities, and so on), so a frame allocates
  nothing, in Rust or Dart. The allocation tests count both.
- **Curves and gradients become tables** of 64 steps when set. Each frame
  only reads tables: no Dart function is called while particles move.
- **Time**: systems pause with `world.paused` and follow `time.scale`,
  unless `useUnscaledTime`, as Unity's.
- **Random**: each system has its own seed, so a test sees the same
  particles each run (`randomSeed`); new seeds each play by default, as
  Unity's `useAutoRandomSeed`.
- **Where it's emitted from** is its object's matrix in the world, read
  in Rust. A system in world space leaves its particles behind as the
  object moves (dust, smoke); in local space they move with it (a
  torch's flame on a moving cart).
- **One thread at first.** If the bar in §6 isn't met, the systems step
  on more than one, each system on its own.

### 3.2 Drawn as one instanced call per system

- **A particle is an instance** of a quad (or of a mesh, for mesh
  particles), with its position, size, rotation, colour, and its frame in
  a texture sheet. A system is one draw: 1000 particles, one call.
- **Facing**: billboards face the camera; stretched ones stretch along
  how fast they move (sparks, rain); horizontal and vertical ones lie
  flat or stand up (a ripple, a ring on the ground); mesh particles turn
  with their rotation (debris, coins).
- **Blending** is the material's (stage 8): additive for fire, sparks
  and glows, transparent for smoke and dust. A blending system's
  particles are sorted furthest first each frame; additive ones need no
  sort. Among other blended things, a system is placed by its bounds'
  centre, stage 8's rule.
- **Light**: unlit by default, with fog, as most games' particles are.
  `lit: true` lights them as a surface facing the camera, for smoke that
  should darken in shade.
- **Soft particles**: `softness` fades a particle out where it meets
  what's behind it, so smoke doesn't cut a hard line through the ground.
  It reads the scene's depth, so it costs (§1): off at low (decision 6).
- **A material's own shader** (stage 10) works on particles too, and
  reads the particle's age and colour.
- **Bounds**: a system's bounds follow its particles, so one off screen
  isn't drawn. It still steps, so it's right when seen again.
- **No shadows** from particles in this stage.

### 3.3 Tests without a screen

- **The simulation is plain Rust**, tested without a GPU: a burst of 30
  makes 30, a lifetime of 1 s leaves none after it, a seed repeats, a
  plane bounces.
- **Goldens** of the drawn effects (stage 1's golden images), with a
  fixed seed and time.
- **In Dart**, `particleCount` and `isPlaying` read what the engine
  stepped, in `dart test`, without a view.

## 4. Phases

Each phase is one PR or a few, with its lab scene, its guide section,
its tests, and plain words. A phase is done when its PRs are merged with
CI green and it's been seen on the Mac, the iPhone and the Note10+.

### Phase 1: particles appear

- `ParticleSystem` with the main module, emission (rate over time,
  bursts), the shapes, colour and size over lifetime, gravity; local and
  world space; `play`, `stop`, `pause`, `clear`, `emit`, `simulate`,
  `isPlaying`, `particleCount`, `stopAction`.
- `Range`, `ColorGradient`, `ColorRange`, curves as functions.
- Billboards with a material and its texture, transparent or additive,
  sorted; fog.
- Pausing with the world and `time.scale`.
- The lab: "Particles: the basics", a fountain whose settings are the
  scene's sliders and switches, and a burst on a button.

*Done when* the simulation's tests and a golden pass; the allocation
tests find nothing in a frame with systems playing and bursting; 10,000
particles step within §6's bar on the Note10+; and a world without
particles is unchanged.

### Phase 2: the look

- Texture sheets; stretched, horizontal, vertical and mesh particles.
- Velocity, limit velocity (drag), force and rotation over lifetime;
  size and colour by speed; noise; random between two curves.
- Soft particles; lit particles; a material's own shader.
- The lab: "Particles: the look", fire, smoke, sparks, rain and a magic
  swirl, each with its few lines in the scene's text.

*Done when* each effect's golden passes, and the lab's scene holds 60 on
all three devices at low.

### Phase 3: particles and the world

- Emission over distance, and a particle starting with its object's
  speed (`inheritVelocity`).
- Sub emitters on birth, death and collision.
- Collision with planes: bounce, dampen, lifetime loss,
  `onParticleCollision`.
- `TrailRenderer` (decision 5).
- `Quality.softParticles`; `drawShapes`, to see each system's shape with
  the debug lines.
- The lab: "Particles and the world", fireworks, a ball that leaves dust
  and a trail as it rolls, sparks bouncing off the floor.

*Done when* the fireworks' sub emitters and the bounces match their
goldens, and the scene holds 60 on all three devices at low.

### Phase 4: a game, and measuring

- **Sheepdog**: dust under the dog and the sheep as they run, embers as
  a wolf burns away, a puff when a sheep is penned (decision 7).
- Textures from Kenney's Particle Pack (CC0), committed with credits.
- The measurements (§6), the guide's "Particles" section and §22's
  Unity rows, the report.

*Done when* Sheepdog with its effects runs as fast as before on all
three devices, and §6's bars are met.

## 5. Not in this stage

Particles on the GPU (Unity's VFX Graph, for hundreds of thousands),
lights on particles, trails from each particle, collision with the
physics world, triggers, wind zones and external forces, custom data, a
mesh's surface as a shape, shadows from particles, and an editor for
effects. They go in the guide's §21. Collision with the physics world is
the first I'd add if a game asks.

## 6. Measurements

On the Mac, the iPhone 16 Pro Max and the Note10+ (profile builds):

- **Stepping**: 10,000 particles (ten systems of 1000, with colour and
  size over lifetime and gravity) step in under 1 ms a frame on the
  Note10+. With noise on, measured and said.
- **Drawing**: those 10,000 small particles hold 60 on the Mac and the
  iPhone at every level, and on the Note10+ at low.
- **Overdraw**, what costs most on a phone: a screen covered by smoke
  (200 large transparent particles), measured at each level and said in
  the guide, with what to do about it.
- **No garbage** in a frame with systems playing, bursting and changing.
- **Nothing lost** without particles: a world without them draws and
  steps as now.
- **Sheepdog** with its effects at each level as fast as now (phase 4).

## 7. Decisions to make

1. **Simulated in Rust on the CPU** (*recommended*: fast, no garbage,
   tested without a GPU, and it reads where objects are without Dart),
   rather than in Dart (simple, but each frame would loop over every
   particle in Dart, and debug builds would be slow), or on the GPU with
   compute shaders (millions, but the Note10+'s GPU is already what
   limits us, and collisions and sub emitters are hard there).
2. **In the core** (*recommended*: the renderer has to draw them, they
   need no native library of their own, and Unity has them built in),
   rather than a package like physics and sound.
3. **Unity's modules and names, as plain Dart** (*recommended*), with
   curves as Dart functions sampled when set and colours over time as
   `ColorGradient`.
4. **Collision with planes only** (*recommended*: a floor or a wall is
   what most effects need), not with the physics world, which needs the
   physics library and the core to talk natively. Later, if a game asks.
5. **`TrailRenderer` in this stage** (*recommended*: a ball's trail, a
   sword's swipe, a coin's streak; it's a strip of quads drawn the same
   way), or later.
6. **Soft particles off at low** (*recommended*), because reading the
   depth costs passes on the Note10+.
7. **Phase 4 in Sheepdog only**, with the measurements (*recommended*: one
   game proves it, and the measurements matter more than more content),
   rather than all three games, or none (as stage 11 ended).
8. **Textures from Kenney's Particle Pack** (CC0), committed with
   credits (*recommended*).
