# Stage 14 plan: make it solid

Status: accepted 2026-10-26 (the user: "go with that order, audit
first"). Phase 1's findings decide what's in phases 2 to 5; the user
sees them before any fix starts. Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 13 is closed
([report](stage-13-report.md)).

Thirteen stages built an engine a game can be written in: drawing,
physics, animation, shaders, sound, particles, meshes made in code. The
last full review was the [hardening pass](hardening.md) on 2026-10-14;
about 250 commits and six areas have come since, none read again by
fresh eyes, and each stage was tested by itself, not with the others.
**Stage 14 adds nothing new.** It makes what's there something a game
can rely on: no known bugs, everything working together, fast on the
three devices and staying fast, and pleasant to write a game with.
Where something isn't well made, it's rewritten.

**The road after it**: stage 15, animation 2 cut down; stage 16,
shipping = 1.0. Cutting meshes (stage 13's phase 4) comes back when a
game asks.

## Rules

As in the hardening pass:

- **A failing test first** for every row that's a bug.
- **Measured** if it's on the frame path: the Mac at least, the Note10+
  and the iPhone where the row says so.
- **The guide**, where a row changes what a game writes, and §23 "What
  changed" for anything that breaks code written before.
- **One PR per phase** or a few; shared-crate touches as their own
  commits. Sizes: S under a day, M a few days, L a week.

## Phase 1: the audit

Read everything again, as the hardening pass did, and write down only
what holds up.

1. **Readers**, one per area, each reading its code whole: the Dart
   framework's core (objects, the world and its loop, components,
   input, assets); the Dart framework's features (animation, particles,
   shaders, cameras, splines, meshes); the bridge and the view (the C
   interface, the render worker, presenting, the Flutter plugin on each
   platform); the renderer and its shaders; the world crate, meshes and
   loading; physics; sound; and the examples, the guide, the tests, the
   tools and CI. Each looks for bugs, costs that grow with the scene
   rather than with what changed, garbage on the frame path, places two
   systems disagree, API that's awkward or inconsistent, and code that
   should be rewritten, and says why for each.
2. **Fresh developers**: new agents given only the guide and the
   packages' public code, each writing a small game from a brief that
   uses many systems at once, as stage 6 did. Their logs say where the
   guide and the API made them stumble.
3. **Checkers** go back over every claim that matters, reading the code
   again and running a throwaway test for each one they can. What
   doesn't hold up is listed at the end, so it isn't raised again.

The result is `docs/stage-14-findings.md`: each finding with where it
is, how it shows, how bad it is (**bug**, **slow**, **awkward**,
**rewrite**), a first idea for the fix and its test. From it come
phases 2 to 5's rows, for the user's OK.

## Phase 2: bugs

Every confirmed bug, worst first. Rows from the findings.

## Phase 3: all of it together

- **One scene with everything on**: a skinned crowd, particles that
  collide, many sounds in 3D, custom shaders, lamps and shadows,
  physics, meshes changed every frame and swept along splines, at each
  quality level, in the lab and as a soak run (memory, garbage, heat).
- **Sheepdog's fences along splines** and a worn path to the pen, stage
  13's game part, so a real game uses the new meshes.
- What breaks where two systems meet, fixed with a test.

## Phase 4: fast, and staying fast

- **A budget per device**: what each system may take of a frame at each
  level, written down, and the frame broken down against it.
- **Curves, not points**: cost against the number of objects, lamps,
  particles, sources and bodies, to find whatever grows faster than it
  should.
- **The benchmarks in CI** on the self-hosted runner, so a slowdown
  fails a PR instead of being found stages later.
- The owed bars: stage 13's mesh change on the Note10+ (1.3 ms against
  1 ms), Sheepdog on the iPhone at 120 and high (80), and the iPhone
  runs owed by stages 12 and 13.

## Phase 5: easy to write a game with

- **One pass over the public API**: names, defaults, error messages that
  say what to do, the guide's every section against the code.
- **Extending it**: own shaders, components and packages, each with a
  clean, documented way in.
- **Fresh developers again**, new agents with the same briefs, to see
  that what phase 1 found is gone.
- The report.

## Not in this stage

Anything new: cutting meshes, animation 2, shipping's work.
