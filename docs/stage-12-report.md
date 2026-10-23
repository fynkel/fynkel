# Stage 12 report: particles

[The stage 12 plan](stage-12.md). Phases 1 to 3 are merged (#138, #139,
#140). Phase 4, Sheepdog's effects and the measurements, is this PR.

## The result

Stage 12's goal was particles as Unity's Particle System has them: a
component on an object that emits, with Unity's modules and names,
simulated natively and drawn in one call per system, so that an effect
takes a few lines.

**That is done.** `ParticleSystem` has Unity's main module, emission,
shape, the over-lifetime modules, noise, texture sheets, the render
modes, soft and lit particles, collision with planes, sub emitters, and
emission over distance; `TrailRenderer` sits beside it. Each phase has
its lab page: "Particles: the basics", "Particles: the look" and
"Particles and the world". Sheepdog uses them: dust at the dog's and
the sheep's feet as they run, embers off a wolf as it burns away, and a
puff with sparkles where a sheep comes home. **Sheepdog with its effects
runs as fast as without them on all three devices, at every level**,
and §6's bars are met. The one finding to keep in mind is what big
see-through particles cost on a phone (below).

What a game writes:

```dart
class Sheep extends GameObject {
  late final Dust _dust;

  @override
  void onLoad() {
    _dust = add(Dust(game.particles, lift: 0.4, from: 2));
  }

  @override
  void onUpdate() {
    _dust.pace(velocity.length); // dust while it runs, none at a walk
  }

  void _comeHome() {
    round.add(Puff(Vec3(position.x, 0.4, position.z), round.game.particles));
  }
}
```

where `Dust` is a `GameObject` with one `ParticleSystem`: a puff every
0.2 m by `rateOverDistance`, thrown back a little by `inheritVelocity`,
soft where it meets the grass (examples/sheepdog/lib/effects.dart).

## What stage 12 did

| Phase | What | PR |
|---|---|---|
| 1 | Particles appear: a CPU simulation in Rust, each system a fixed pool; the main module, emission with bursts, shapes, size and colour over lifetime, local or world space, stop actions; one instanced draw per system, sorted when see-through; `world.particleStepTime` | #138 |
| 2 | The look: `ParticleCurve` (Unity's MinMaxCurve), velocity, force, limit velocity, rotation, noise, size and colour by speed, texture sheets, stretched, horizontal, vertical and mesh particles, soft and lit particles, a material's own shader with `particle_age()` and `particle_random()` | #139 |
| 3 | The world: emission over distance, inherit velocity, collision with planes and `onParticleCollision`, sub emitters, `TrailRenderer`, `Quality.softParticles`, `debug.drawShapes` | #140 |
| 4 | Sheepdog's dust, embers and puffs with Kenney's Particle Pack; the screen of smoke in the lab; the measurements | this |

The engine's C interface went from ABI 38 to 41. Phase 4 changed
neither the engine nor the ABI.

## Measured

Profile builds, 2026-10-23. The Mac's display runs at 60 Hz; the
iPhone 16 Pro Max and the Note10+ at their 60 fps default.

**Sheepdog, with and without its particles** (`SHEEPDOG_PROBE=noparticles`
turns them off), the median of 40 one-second windows:

| | low | medium | high |
|---|---|---|---|
| Mac | 60 / 60 | 60 / 60 | 60 / 60 |
| iPhone 16 Pro Max | 60 / 60 | 60 / 60 | 60 / 60 |
| Note10+ | 52.9 / 52.9 | 23 / 23 | 23 / 21 |

The Note10+ at low is the median of three runs each, alternated,
because one run to the next moved by up to 8 fps either way (with:
52.9, 58.9, 50.9; without: 54.9, 52.9, 51.9). Its medium and high are
where they were before particles; low is the level a Note10+ plays at
(`Quality.device`). The first Mac run after a build had one slow second
with particles (about 40 fps) and none on the next, which is the
pipelines being made the first time; on the iPhone, single slow seconds
came both with and without, and didn't come back when run again.

**§6's bars:**

- **Stepping 10,000** (ten systems of 1000): the Note10+ 0.34 ms a
  frame (the bar was 1 ms), the iPhone 0.17 ms, the Mac 0.14 ms, as
  the lab's page reports it (`world.particleStepTime`, which includes
  Dart's side). The world alone on the Mac is 0.025 ms (phase 1).
- **Drawing 10,000**: 60 on all three at each one's own level (low on
  the Note10+), which is the bar; the other levels weren't measured.
- **Overdraw**, the screen of smoke (puffs two to three metres across,
  covering the whole screen, `LAB_SMOKE=` the count, `LAB_FULL=true`):

  | puffs | Mac | iPhone 16 Pro Max | Note10+ |
  |---|---|---|---|
  | 200 | 60 | 60 at every level | 60 at low; 12 at medium and high |
  | 500 | | 60 at low, 40 at high | 60 at low |
  | 1,000 | | about 20 at medium or high | 28 to 48 at low |
  | 2,000 | | | 27 at low |

  At low the Note10+ draws 810 × 1507 pixels (`renderScale` 0.75) with
  soft particles off; at medium and high, 1080 × 2009 with them on, and
  200 puffs take it from 60 to 12. The iPhone draws 990 × 1886 at low
  and 1320 × 2514 above it. Big see-through particles are the one thing
  in this stage that can sink a phone; the guide's §13c says what to do:
  a few hundred big puffs on screen at most, smaller and shorter-lived,
  and low's fewer pixels where it matters.
- **No garbage**: the Dart allocation tests (phases 1 to 3) play, burst,
  collide and trail with nothing allocated, and the Rust test steps and
  draws with nothing allocated on the calling thread. Phase 4 added no
  engine code.
- **Nothing lost without particles**: a world without them skips the
  step and the extraction, and every golden from before stage 12 is
  the same.
- **The lab's pages** at low: "the look" Note10+ 59.9, iPhone 60, Mac
  60; "the world" Note10+ 59.9, Mac 60, iPhone 60 (full screen).

## Found on the way

- **A scene buffer grew when a busier frame landed on it.** The render
  worker rotates three scene buffers, and the extraction pushed
  particles into whichever it had, so a burst on one frame grew that
  buffer later than the others. The alloc test caught it once in 40
  runs. The extraction now reserves room for every system's pool and
  every trail before filling (#140).
- **The soak test's malloc can sit at two levels** about 1 MB apart
  without a leak; its halves are now compared by their mean (#140).
- **The smoke didn't cover the screen at first.** The lab's view is
  part of the screen beside its panel, and the puffs were behind the
  floor's edge, so the first numbers were for two thirds of a panel.
  `LAB_FULL` and the smoke in front of the camera made it the whole
  screen.
- **The basics page ignored `LAB_QUALITY`**, so its first runs "at each
  level" were all at the device's own. It reads it now; the table
  above gives each number at the level it was really drawn at.
- **One run is not a measurement on the Note10+.** Sheepdog at low
  moves by up to 8 fps from one 40-second run to the next, cool or
  not; with and without particles had to be alternated and run three
  times each to say they're the same.

## Not measured

Cut on 2026-10-23 to move on: the screen of smoke on the Mac above 200
and at each level, the iPhone at 1,000 at each level, and drawing
10,000 at medium and high on the phones. Stage 14, shipping, measures
the games again.

## Not in this stage

Particles on the GPU (Unity's VFX Graph), lights from particles, trails
from each particle, collision with the physics world, triggers, wind
zones and external forces, custom data, a mesh's surface as a shape,
shadows from particles, and an editor for effects. They're in the
guide's §21. Collision with the physics world is the first I'd add if a
game asks. Coins and Pocket Hole have no particles yet (decision 7: one
game proves it).

## Next

Stage 13, animation 2 cut down: 2D blends, look-at and foot IK. Then
stage 14, shipping = 1.0.
