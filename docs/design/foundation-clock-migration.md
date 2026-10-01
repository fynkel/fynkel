# Canonical clocks and timed gameplay (F1.2)

**Date:** 2026-09-30  
**Decision:** [ADR-0047](../decisions/ADR-0047-identity-clocks-frame-order-and-readiness.md)  
**Evidence:** `main` working tree against `87e3dec8`; not a release certification.  
**Ledger:** [foundation roadmap](../foundation-roadmap.md).

## Runtime contract

The native FrameClock owns accepted real elapsed, accepted gameplay elapsed,
completed simulation elapsed, fixed-step planning and interpolation remainder.
The native frame call receives real delta and the requested scale separately.
Scale is sampled once at frame begin; callbacks changing it affect the next
frame. Negative/non-finite host delta counts as zero. Scale must be finite and
nonnegative. Gameplay delta is `min(realDelta * scale, maxFrameDt)`; only the
world clock clamps it. Flutter no longer truncates real delta or exposes a
second `FynkelGameView.maxFrameDt` setting. Configure the cap on World.create.

| Timeline | Advancement | Readers |
| --- | --- | --- |
| Real | Accepted valid host delta, without gameplay scale/clamp | Time.realElapsed; explicit real-clock waits, invokes and tweens |
| Gameplay | Accepted scaled/clamped delta at frame begin, including zero-tick frames | Time.elapsed in every phase; default waits, invokes and tweens; shader globals.time |
| Simulation | Completed ticks times the native fixed delta | Time.simulationElapsed; fixed callbacks; physics and simulation animation; explicit simulation-clock routines |

Time.delta remains fixedDelta during fixed callbacks and gameplay delta in
variable callbacks. Time.unscaledDelta is sanitized real host delta.
Time.elapsed never switches timelines according to callback phase and never
adds interpolation alpha. Scene replacement retains world timelines; a fresh
world starts at zero. Absolute host timelines use f64; the existing native
simulation accumulator/physics fixed delta remain f32. Simulation elapsed
comes from the native clock, including native steps outside the Dart wrapper.

FramePlan reports gameplay-clamp exclusion as clampedSeconds and simulation
catch-up exclusion as droppedSeconds. Neither is subtracted from already
accepted gameplay elapsed. The ABI returns real/gameplay deltas, absolute
elapsed and frame number together, so Dart caches a native result instead of
reconstructing or accumulating another clock.

## One timed gameplay API

WaitForSeconds, invoke, invokeRepeating, generic tweens and transform tweens
accept the same TimeClock selection; gameplay is the default:

```dart
yield const WaitForSeconds(0.5); // gameplay
yield const WaitForSeconds(0.5, clock: TimeClock.real);
yield const WaitForSeconds(0.5, clock: TimeClock.simulation);

invoke(updateHud, 0.1, clock: TimeClock.real);
world.tween(0.0, 1.0, 0.5, setOpacity,
    clock: TimeClock.real, ease: Ease.linear);
```

Deadlines/start times read the chosen service at creation. A routine created
in update starts at that frame's already-advanced gameplay time; it does not
consume the same frame's delta again. Waits and tweens share a one-microsecond
deadline tolerance for floating-point fixed-step values. Simulation routines
resume after completed fixed ticks; real/gameplay routines resume after
variable update. Default zero-second invokes still resume next host frame;
explicit simulation invokes resume next tick. Invalid durations are rejected.
The behaviour system's private accumulated coroutine clock is removed.

TestTime exposes the same named timelines. Tests drive accepted gameplay/real
frames with advanceFrame and completed simulation with advanceTick separately.
Its constructor can explicitly seed elapsed; legacy tick-seeded construction
initializes the gameplay starting value once, without deriving later elapsed
from tick progress.

## Editor Step and host acceptance

World.stepFrame runs exactly one fixed sequence, including input, fixedUpdate,
native simulation, contacts/afterPhysics and fixed-clock continuations. It does
not begin another host frame, run variable callbacks or advance real/gameplay
time. It works at timeScale zero and requires a completed frame boundary.
The Flutter editor Step control uses it instead of manufacturing a normal
fixed-duration host frame. Buffered fixed input edges are consumed once.
After the fixed sequence, it calls the additive native
`fynkel_world_snap_interpolation` operation before presentation. This replaces
previous local poses with completed poses so rendering and projected anchors
show the stepped state, including descendants, at zero or fractional alpha.
The accumulator and clocks are unchanged; normal interpolation resumes on the
next tick. Runtime ABI 3 and wire layouts are unchanged.

Gameplay pause (timeScale zero) still accepts host frames, captures input and
runs variable callbacks; real-clock UI routines continue. Muting the host loop
(editor inspection Pause, view setPaused or a hidden view under its policy)
suspends host-frame acceptance entirely. Real elapsed means accepted host time,
not an OS wall-clock timer. Resuming a muted Flutter ticker re-primes its delta;
time spent with no accepted host frames is not retroactively simulated.
This preserves the existing host suspension policy. Full host/frame transaction
integration and its pause policy qualification remain at F2.3/F7.

## Timing consumer audit

| Consumer | Result |
| --- | --- |
| Dart public time and behaviour contexts | Native accepted gameplay/real snapshots; explicit native simulation elapsed |
| Waits, invokes and world/behaviour/transform tweens | Absolute deadlines/start times on TimeClock; no private coroutine accumulator |
| Physics, characters and native animation/controllers | Completed simulation ticks and the immutable fixed delta; clip phase, speed and crossfade state remain relative playback state |
| Native rotator/oscillate/lifetime/emitter systems and headless sample motion | Simulation-bound processing; existing component-relative phase/countdowns remain simulation state |
| Shader global time | Accepted gameplay elapsed; transform interpolation remains simulation-based |
| Audio transport | Device-sample playback position remains transport state; command processing remains simulation-bound |
| Frame instrumentation, residency budgets and host run-for duration | Real platform stopwatch measurements, not gameplay deadlines |
| Test clocks | Explicit frame/tick driving on the same named timelines |

This does not migrate transactional gameplay state, transform write lanes,
interpolation reset, render-only overrides, scene preparation or resource
readiness. Relative animation/audio transport state is not a second world clock.

## Breaking bridge and verification

Runtime ABI is now **3**, with schema fingerprint `0x52ca2a9cfe616eff`.
The native begin-frame call takes double delta and scale; FramePlan carries
clock results and StepResult carries completed simulation elapsed. Native C,
Rust callers, generated Dart bindings/contracts, SDK lock, test project lock
and binary fixtures migrate together. Authoring ABI 5 and Apple viewport ABI
12 have unchanged own layouts; their libraries still need rebuilding against
the new runtime. Old prebuilt runtime entries remain absent from the SDK lock.

Actual-world regressions cover catch-up drops versus gameplay deadlines,
creation-frame timing, pause/scale changes, zero-tick and invalid frames,
explicit real/simulation routines, editor Step, input edges, scene replacement,
TestTime, native ABI clock results and shader time. Flutter checks that a
2-second hitch reaches real elapsed unchanged while gameplay accepts only its
clamped share, and that editor Step advances only simulation elapsed.

The [clock qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-clocks.json)
records commands, source hashes, toolchain, machine and outcomes. All 15
commands pass: 820 Rust tests (6 ignored), 351 SDK, 16 authoring, 39 Flutter
and 91 focused editor tests; all analyses, strict Clippy, formatting and
generated-contract checks pass. The probe confirms .25 seconds of accepted
gameplay elapsed agrees with the wait after a capped catch-up frame.

Broader F0/F8
qualification, example document/lock upgrades, AOT captures and the remaining
F1 identity/transform/bounds work are separate gates.
