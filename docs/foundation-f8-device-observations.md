# F8 physical-device observations — 2026-10-03

User-reported results on the paired iPhone:

- Release host: rotating cubes, HUD/scene interaction, landscape rotation,
  background/foreground and closing/reopening worked.
- Fresh generated F8 Release Game: bundled scene loaded; joystick movement,
  jumping and reopening worked.
- The user noticed possibly uneven cube movement while the HUD reported
  120 FPS. This is an unresolved motion-quality observation, not a confirmed
  camera bug and not a smoothness pass.

Code inspection establishes that `FynkelGameView` computes FPS from game-loop
callback count divided by elapsed wall time. This is not a count of distinct
frames displayed by the device. Rendering has separate dropped/skipped counters;
the Apple texture bridge can return the newest completed image independently
of game-loop cadence.

The starter Mover writes transforms in fixedUpdate at the default 60 Hz.
The native prepare-gameplay-tick boundary captures previous transforms before
those writes for render interpolation. The generated starter camera is static.
These facts do not identify the cause of the observed unevenness.

Follow-up qualification must correlate fixed ticks and interpolation fractions,
render submissions/completions and texture acquisitions over time on the phone,
including held joystick movement and jumps. Check interval tails and repeated
images, not just an average loop FPS. Preserve fixed-step gameplay semantics;
do not hide the observation by increasing the simulation rate or adding arbitrary
camera smoothing. Verify any identified fix with a regression test and another
physical-device observation.

The user has no second Mac. That separate qualification criterion remains open.

## Follow-up trace and heat report

The user repeated movement/jump testing and confirmed uneven movement, then
reported that the iPhone 16 Pro Max became very hot. No device temperature or
thermal-state telemetry was collected, so thermal throttling is not established.
The app was stopped; a device process inventory confirmed no Runner process.
Further device execution waits for the user to report that it has cooled.

The diagnostic build wrote per-second timing windows to its sandbox. The trace
contains 94 windows. Example steady windows show 99–117 loop callbacks, only
17–23 new texture acquisitions, and 82–94 ring-full drops. GPU frame spans
were roughly 40–60 ms in these windows, versus sub-millisecond typical simulation
work. This establishes a GPU-throughput bottleneck in this device run. Pass
spans overlap on Metal and must not be added together or interpreted as isolated
costs; an effect-isolation capture is still needed.

Implemented diagnostic corrections:

- Captures now include acquired/repeated textures, presentation lag, acquire
  waits and Swift texture counters.
- Swift counters are read/reset under the same lock as raster/completion writes.
- Frame statistics expose the report duration and texture acquisition rate;
  the starter HUD distinguishes loop rate from new scene textures per second.
- A permanent SDK regression covers 240 frames of 120 Hz presentation of a
  60 Hz kinematic cube, including separate rotation and translation commits.
  All eight projection tests and eight game-view tests pass.

Renderer experiments were not accepted as fixes. Half-resolution reconstruction
failed existing corner-occlusion image checks and was removed without weakening
the assertions. Precomputed sample rotations passed all 53 shading tests but
showed no demonstrated speedup in the local phone-resolution benchmark
(approximately 4.14 ms baseline vs 5.20 ms experimental median GPU span in
separate short runs). That shader change was also removed. The original shader
and full-resolution lighting remain intact. A new ignored GPU benchmark retains
the phone-resolution workload for subsequent optimization.

The mobile performance/heat issue remains open. Neither the instrumentation
changes nor passing interpolation tests constitute a rendering-performance fix.

## Cooled-device effect isolation

The user confirmed cooldown before this run. The diagnostic rendered for four
seconds with `medium`, then four seconds with ambient occlusion and contact
shadows disabled, at the same 1320×2868 viewport and 4× MSAA. It automatically
paused after eight seconds; subsequent windows report zero rendered frames.
The app was then explicitly terminated. No production quality defaults changed.

Ignoring startup and the mixed transition window:

| Configuration | Loop callbacks/window | New textures/window | Median GPU frame span | Ring-full drops/window |
| --- | ---: | ---: | ---: | ---: |
| Medium | 120 | 38 | 26.05–26.18 ms | 82 |
| Screen-space effects disabled | 120 | 113–119 | 5.74–5.95 ms | 0–4 |

Windows are approximately one second. These texture counts measure compositor
acquisition, not physical screen scanout. Pass-timing overlap is not needed to
infer the result: removing the stage substantially improves whole-frame time
and delivery rate. This identifies the screen-space stage (depth prepass,
ambient/contact estimate and blur collectively) as the principal bottleneck in
this scene. It does not yet isolate which shader/pass dominates or establish a
sustained thermal limit. The prior hot-run slowdown is consistent with a thermal
effect, but no thermal-state telemetry was collected to prove throttling.

Next: measure the estimate/blur/depth costs separately, optimize the expensive
work while retaining the established image tests, then compare short identical
on-device runs before any sustained thermal qualification. Disabling the effects
is a diagnostic comparison, not the shipped fix.

## Retained depth-read optimization

Separate depth/estimate/blur timestamp boundaries and a single-sample comparison
showed substantial cost in repeatedly reading multisampled depth. The retained
change copies exactly sample zero into a full-resolution R32Float texture once
when the screen-space stage runs with MSAA. The original scene antialiasing,
12-sample lighting estimate, 4×4 blur and internal resolution remain unchanged.
Single-sample scenes keep reading their original depth directly. The extra
attachment costs four bytes per internal pixel: 14.44 MiB at 1320×2868.
It follows existing resize, quality-switch and renderer teardown ownership.

Short iPhone captures at medium quality measured roughly 36–38 acquired textures
per second / 27 ms median GPU frame time before, and 43–46 / 22–23 ms after.
These are short runs, not thermal-controlled repeated trials or sustained
qualification. The test automatically paused after eight seconds and was stopped.
This is a measured improvement, not a 60 FPS or heat-resolution claim.

A later experiment caching camera distance as well as depth used more bandwidth
and was slower on both devices; it was removed. An algebraic blur experiment
passed image checks but was not retained. The final shader keeps the original
lighting calculations. Only the compact depth copy and opt-in stage tracing
remain from this optimization work.

Validation: the full renderer suite passed (including all 53 shading/image
regressions), as did the Apple platform suite and strict renderer Clippy checks.
The new GPU regression writes different depths into all four MSAA samples and
checks the production copy's exact sample-zero result at 1×1 and odd 5×3 sizes.
No image thresholds were weakened. The built-in pipeline count is now 31 because
of the added depth-copy pipeline; the public viewport ABI is unchanged.

Remaining: further screen-space performance work, a user-observed motion check
of the retained implementation, and bounded thermal-state measurements before
any sustained heat qualification. The second-Mac obligation remains unavailable.


## Dense mobile workloads (2026-10-03)

The user's requested density priority now has a repeatable
[workload ladder and bounded app](../benchmarks/mobile/README.md). The existing
stress generator required two real repairs: rigid bodies lacked required
`constraints`, and manually authored rig joints lacked F5 stable
`AnimationBinding` keys. Regression tests compare those keys to the actual GLB
skin joint indices. All three presets now validate/cook/render/simulate on Mac,
with zero missing assets/placeholders/rejected components. Bare preflight does
not execute Dart behaviours; physical app captures do.

Physical iPhone 16 Pro Max release captures, medium quality, 1320×2868, MSAA4,
AO/contact shadows/shadows/bloom/grading retained:

| Workload | Native movers / bodies / Dart behaviours / rigs | Visible instances during capture | New texture acquisitions/s after startup | GPU frame window p50 | Simulation window p50 |
| --- | --- | --- | --- | --- | --- |
| dense-500 | 500 / 50 / 30 / 20 | 181–256 | about 52–54 | about 18–19 ms | about 2.8–3.0 ms |
| dense-5000 | 5,000 / 500 / 300 / 200 | 1,728–1,902 | about 8–9 | about 26 ms | about 103–110 ms |

Both completed their eight-second app deadline and the runner terminated only
the exact PID returned by its launch (421 and 436 respectively). The larger run's
app timer fired at 8.110 seconds because the UI thread was busy; the external
runner also stops the exact process before copying logs. All device thermal
samples during rendering were nominal. Startup samples had unknown pressure and
no frames; they are retained but excluded from the steady-window ranges. These
short captures do not establish sustained performance, temperature or scanout
pacing. No visual movement confirmation was requested for these automated probes.

The larger scene is CPU-bound in this capture: behaviour-phase p50 approximately
67–71 ms per host frame, native-phase p50 approximately 36–38 ms. It runs four
fixed ticks per host frame and still misses deadlines. Behaviour time includes
SDK callback/transaction overhead, not just user Dart arithmetic. Source inspection
confirms every Wobble writes one transform per fixed tick through the ordinary
transactional callback path. The exact costly operation has not yet been isolated;
do not attribute all this time to Dart or remove transaction guarantees based on
aggregate counters. Next isolate behaviour count, fixed-tick/native systems and
transaction costs at constant scene density, alongside the known GPU screen-space
cost. Lowering physics frequency or disabling gameplay is diagnostic only, not a
performance fix.

Evidence: [Mac preflight](../benchmarks/foundation/results/2026-10-03-f8-mobile-preflight.json),
[500 capture](../benchmarks/foundation/results/2026-10-03-f8-mobile-device-500.json),
[5,000 capture](../benchmarks/foundation/results/2026-10-03-f8-mobile-device-5000.json).
The 2,000 preset has Mac preflight only. Six Python workload/runner tests and 23
Flutter controller/game-view tests passed; the game-view suite (8 tests) was
rerun after adding the thermal-summary assertion. Package and generated-app
analysis passed, and both diagnostic iOS release builds passed. F8 stays open.

## CPU isolation and animation ownership fix (2026-10-03)

A Mac AOT probe keeps dense-5000 unchanged and runs exactly one fixed tick per
sample, varying only enabled Wobble callbacks (0/30/300). Forty warmup ticks and
120 measured ticks per fresh world separate catch-up multiplication from cost
per tick. Sampling found repeated full AnimatorRun resource scans at callback
transaction publication, including read-only/empty lifecycle transactions.

The resource owner now caches the union of running-animation asset IDs against
an opaque ECS component stamp. The stamp includes world identity, entity
membership and AnimatorRun mutations. Transform/name edits do not invalidate it;
runtime playback writes, component removal, destruction and world replacement do.
Queued effects and live audio voices are still observed every boundary, so
pending crossfades and playback lifetimes cannot be skipped by this cache.
Publication ordering, transaction atomicity and gameplay APIs are unchanged.

Serialized Mac AOT comparison (median microseconds per single fixed tick):

| Enabled Wobbles | Total before | Total after | Behaviour phase before | Behaviour phase after |
| --- | ---: | ---: | ---: | ---: |
| 0 | 22,609 | 14,216 | 13,480 | 4,978 |
| 30 | 23,797 | 14,799 | 14,081 | 5,339 |
| 300 | 27,804 | 17,782 | 18,537 | 8,264 |

The native phase remains approximately 9–10 ms/tick in this comparison. These
are local AOT measurements, not iPhone timings or hard cross-machine thresholds.
An earlier after-build capture overlapped compilation and is excluded.

The rebuilt iPhone release app, same dense-5000 content and medium effects,
completed another bounded diagnostic. Post-startup acquisitions were about
12–14/s (previously 8–9/s); simulation window p50 fell to approximately 66–71 ms
per host frame (previously 103–110 ms). Behaviour phase fell to about 30–33 ms from
67–71 ms, with four fixed ticks still being executed per host frame. All rendering
thermal samples were nominal. The app stopped at its deadline; the runner then
terminated exact launch PID 472 before copying logs. Faster simulation advances
further during the same wall time, so the camera/physics pose is not identical
between phone captures; the single-tick Mac experiment controls that variable.

Core/native checks: 463 passed, zero failed, three ignored. The expanded regression
also passed separately, covering 600 unchanged-boundary synchronizations, playback
state changes, destruction, world replacement, and queued crossfade references
without an AnimatorRun revision. Six Python tests passed; iOS release build passed.
The follow-up sampler still shows native physics work and memory copying around
transaction ownership transfers. Those costs need further isolation; the current
fix does not qualify dense scenes at 60/120 Hz. GPU screen-space work remains open.

[Raw samples, source/binary hashes and device log](../benchmarks/foundation/results/2026-10-03-f8-cpu-isolation.json).

## Stable native world allocation (2026-10-03)

The follow-up CPU sample showed memory movement around the owned-session bridge.
`WorldSlot` and `GameplaySession` previously transferred an inline `World` at every
begin/finish/abort. They now transfer a `Box<World>` allocated once by the native
host. Prepared-resource finalization preserves that same allocation. The ownership
contract stays exclusive and transactional; there is no unsafe borrowed host
pointer or bypass API. [ADR-0060](decisions/ADR-0060-stable-world-ownership-in-bridge-transactions.md)
records the Rust session signature change and guarantees.

Controlled Mac AOT medians, same dense-5000 scene and fixed-tick sequence; before
already includes the animation ownership optimization:

| Enabled Wobbles | Total before (µs) | Total after (µs) | Behaviour before (µs) | Behaviour after (µs) |
| --- | ---: | ---: | ---: | ---: |
| 0 | 13,836 | 11,276 | 5,032 | 2,694 |
| 30 | 13,898 | 11,522 | 5,249 | 2,921 |
| 300 | 16,487 | 13,939 | 7,828 | 4,874 |

Native simulation remains about 8.5–9.0 ms per fixed tick. That work has not been
optimized by changing world ownership.

The same full-resolution medium dense iPhone app delivered roughly 14–18 new
textures/s after startup, versus 12–14 in the preceding capture (original 8–9).
Simulation window p50 ranged approximately 46–64 ms per host frame. Tick counts
vary with catch-up, so the fixed-tick Mac experiment is the controlled CPU
comparison; the phone scene advances farther in equal wall time as it gets faster.
All rendering thermal samples were nominal. The app reached its bounded deadline
and exact launch PID 488 was terminated before log retrieval. No sustained or
physical-display pacing claim follows from this eight-second probe.

The existing core/native/assets suites passed 506 tests with zero failures and
three ignored tests. The owned-session suite then passed all four tests, including
the new regression that checks allocation identity through 100 commits, explicit
abort, failed edits, final-validation poisoning and refused poisoned entry. The
45 affected SDK tests, all-target Clippy with warnings denied, and iOS release
build passed. No change to gameplay data, callback ordering, fixed frequency,
scene density, visual quality or transaction semantics was used to earn the gain.

Next candidates: native physics synchronization/solver work and repeated lifecycle
queries; retain the single-tick isolation to distinguish actual workload from
catch-up amplification. GPU screen-space cost is still independently over budget.
F8 remains open.

[Raw CPU/device data, source and binary hashes](../benchmarks/foundation/results/2026-10-03-f8-stable-world.json).

## Lifecycle invalidation (2026-10-03)

Empty transactions and pose-only edits previously dirtied the SDK behaviour
lifecycle on every boundary. The SDK now invalidates it for script attachment,
enabled-state or removal changes, or a nonempty/reset native callback delta.
That delta now tracks native names, membership, hierarchy and activity, so native
reparenting and activation still reach scripts without an unconditional scan.
Relevant journal overflow resets safely; unrelated pose churn does not.

Controlled Mac AOT single-fixed-tick medians for the same dense-5000 workload:

| Enabled Wobbles | Total before (µs) | Total after (µs) | Behaviour before (µs) | Behaviour after (µs) |
| --- | ---: | ---: | ---: | ---: |
| 0 | 11,742 | 8,546 | 2,789 | 29 |
| 30 | 11,700 | 8,763 | 2,939 | 233 |
| 300 | 13,431 | 10,615 | 4,795 | 2,070 |

Native simulation remains approximately 8.45 ms per fixed tick. Physics frequency,
scene density, gameplay and visual settings are unchanged.

The first phone launch was blocked by its locked screen; that failed attempt is
retained. After the user unlocked it, the bounded eight-second run completed and
the runner terminated exact launch PID 601 before retrieving logs. New-texture
acquisition ranged 17.42–28.55/s, versus approximately 14–18/s previously. It peaked
in the second window and declined to 17.42/s in the last; this is not a steady
29 FPS result. GPU frame window p50 remained about 17.4–18.2 ms. Native simulation
per host frame grew with catch-up and advancing workload. The camera reached more
visible geometry (1,735 to 2,227 instances), so the phone captures are not identical
pose comparisons. All rendering thermal samples were nominal; neither the cause
of the late slowdown nor sustained thermal stability is established.

Permanent regressions cover bounded bridge work with hundreds of disabled scripts,
native reparent/activity callbacks exactly once, destruction and relevant versus
unrelated journal overflow. Core/native: 465 passed, zero failed, three ignored.
The full SDK suite passed 615 tests with production changes; after adding the final
reparent regression, the adoption suite passed all 10 tests. Dart analysis,
all-target core/native Clippy with warnings denied, and the iOS release build passed.

Next work is to isolate native physics synchronization/solver cost and continue
GPU screen-space optimization. F8 remains open, including sustained performance
and the unavailable second-Mac qualification.

[Raw samples, failed/successful device attempts, hashes and validation](../benchmarks/foundation/results/2026-10-03-f8-lifecycle-invalidation.json).
