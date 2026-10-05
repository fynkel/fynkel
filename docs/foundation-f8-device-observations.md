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

## Physics synchronization inputs — Mac only (2026-10-03)

Optional per-system CPU probe readback identifies physics at 5.78 ms of an 8.42 ms
native tick (120 measured ticks). Sampling also shows substantial ECS-to-physics
synchronization work. Synchronization now carries its already-read components and
validated transform into application instead of reading and decomposing them
again. Bodies still apply before nested colliders. This increases retained scratch
buffer memory with the affected population; collision rules, solver settings and
simulation frequency are unchanged.

Controlled 300-tick AOT runs, 40 warmup ticks, all 300 behaviours, no compilation,
in before/after/after/before order yielded native medians 8.697 / 8.267 / 8.365 /
8.972 ms. This supports approximately 5–7% less native work in this workload.
An initial exploratory comparison overlapped compilation and was inconclusive;
all runs are retained. No new phone measurement is claimed.

The core/physics release suites passed 401 tests (zero failed, one ignored), then
the added child-collider regression passed separately. It covers parent-body
creation/removal independent of entity order while changing transform and collider
offset in the same synchronization. Release all-target Clippy with warnings denied
and analysis of the generated Dart probe passed. The earlier debug test compile
was cancelled; it is not counted as a passing check.

[Raw timings, source/binary hashes and archived sampler](../benchmarks/foundation/results/2026-10-03-f8-physics-inputs.json).
F8 stays open; device validation, remaining native costs and GPU effects remain.

## Physics synchronization — iPhone follow-up (2026-10-03)

Rebuilt release at `d39ae672`; the native framework hash differs from the prior
lifecycle capture. The dense-5000 app completed its eight-second deadline and
exact launch PID 662 was terminated before retrieving logs. New-texture acquisition
ranged 19.66–29.10/s, versus 17.42–28.55/s in the previous capture. These ranges
overlap substantially: this short run does not establish a clear phone speedup
from the Mac-verified optimization. The last four windows were about 20–21/s,
with the same slowdown as the scene advances. Visible instances grew from 1,733
to 2,229. All rendering thermal samples were nominal.

GPU frame p50 remained approximately 17–18 ms. Native simulation also remains
costly; the per-host-frame measurements include changing numbers of fixed ticks.
No sustained-performance, physical scanout, or heat-resolution claim follows.
F8 remains open. Next investigation should separate the advancing scene's physics
cost from fixed-tick catch-up and continue the independent GPU effects work.

[Device records, build log hashes and native binary hash](../benchmarks/foundation/results/2026-10-03-f8-physics-inputs-device.json).

## Fixed-tick timeline and AO sample arithmetic (2026-10-03)

A 600-tick Mac CPU run (after 40 warmup ticks, all 300 scripts, no rendering)
does not reproduce the phone's later slowdown. After the initial window, native
60-tick-window medians stay near 8.2–8.5 ms, and physics stays near 5.5 ms. This
separates advancing simulation cost from host-frame catch-up on this Mac; it does
not identify the cause on the phone or exclude CPU/GPU power contention there.
[Raw timeline](../benchmarks/foundation/results/2026-10-03-f8-cpu-timeline.json).

The AO shader now stores the same twelve golden-angle hemisphere points and
rotates them with one sine/cosine pair per pixel. It previously recalculated each
sample's trigonometry. Resolution, sample count, blur, effect strength and scene
workload are unchanged. A numerical regression compares every sample across
1,024 dither rotations against the original f32 formula within 3e-6 per coordinate.
The existing renderer suite passed 95 tests (zero failures, five ignored); the new
regression passed separately, and renderer all-target release Clippy passed.

Bounded iPhone runs compared the previous build, candidate, previous build again,
and candidate again. Median of post-first-second window p50s (not pooled frames):

| Run | AO stage (ms) | GPU frame (ms) |
| --- | ---: | ---: |
| Prior physics-inputs capture | 11.109 | 17.808 |
| Precomputed kernel | 10.844 | 17.158 |
| Old-build control repeat | 11.127 | 17.426 |
| Precomputed-kernel repeat | 10.820 | 17.600 |

The AO stage improves approximately 2–3%; whole-frame timing overlaps and no
clear overall acquisition gain is established. All new captures completed their
eight-second deadline with nominal rendering thermal samples; exact launch PIDs
669, 674 and 676 were terminated before log copying. The last installed build is
the candidate. No sustained performance, physical-display scanout or resolved heat
claim follows. F8 remains open. Further GPU work and phone per-tick isolation are
still required; the CPU timeline alone does not explain its slowdown.

[All device records, timing summaries, source/binary hashes and validation](../benchmarks/foundation/results/2026-10-03-f8-ao-kernel.json).

## Phone CPU and presentation isolation (2026-10-03)

The bounded benchmark now optionally buffers per-host-frame CPU totals, tick count
and the last native tick's system timings. It writes them at stop. No SDK or engine
scheduling API changed. Readback was approximately 2–3 µs median per host frame,
outside the simulation timer. It does not capture every native tick, and frame
cost divided by ticks still includes frame overhead. An analyzer regression
prevents zero-tick stale data from being counted as a fresh tick.

At the normal 3,000 µs presentation wait, early windows performed about 1.3 ticks
per host frame; late windows performed about 3.2. The last native tick's median
rose from about 7.3–7.9 ms to 8.2–8.4 ms, while physics settled near 5.4–5.5 ms.
Thus much of the growth in per-frame simulation time is catch-up amplification,
not a threefold increase in individual tick cost. Earlier ticks in each host
frame remain unsampled; this does not fully explain the remaining overhead.

A second capture disabled the wait only through a benchmark build option. The
reported acquisition wait became zero, but late acquisition remained around
19–20/s and catch-up remained about three ticks per frame. Removing that wait does
not resolve this slowdown; engine defaults remain unchanged. Both runs reached
the bounded deadline, with nominal rendering thermal samples. Exact launch PIDs
680 and 682 were terminated before evidence copying. The currently installed
benchmark is the zero-wait diagnostic; it is stopped.

Seven benchmark/analyzer tests, Dart analysis and both iOS release builds passed.
Further simulation and GPU cost reduction is still required. F8 remains open;
this does not establish sustained throughput, physical scanout or thermal stability.

[Raw samples, per-second summaries and validation](../benchmarks/foundation/results/2026-10-03-f8-phone-cpu-isolation.json).

## Rejected squared-distance blur experiment (2026-10-03)

Tested replacing each blur tap's square root and radial interval comparison with
squared bounds. Taps, depth tolerance, output resolution and quality were unchanged.
All 96 renderer tests passed (zero failed, five ignored), renderer Clippy passed,
and all 63 captured shading images were byte-identical before/after.

The first launches were blocked by the locked phone and are preserved as failures.
After the user unlocked it, baseline and candidate completed bounded captures;
exact PIDs 739 and 745 were terminated before log retrieval. Both used medium,
normal 3,000 µs acquisition wait, and no per-frame CPU readback. All rendering
thermal samples were nominal. Median of post-first-second window p50s:

| Build | AO stage (ms) | GPU frame (ms) |
| --- | ---: | ---: |
| Baseline | 10.840 | 17.193 |
| Squared blur candidate | 10.869 | 17.201 |

There is no measured benefit. The candidate was reverted; the earlier precomputed
AO kernel stays. Retain this negative result to avoid repeating an unproductive
change. F8 remains open. Reducing repeated per-tap depth reconstruction is a
separate candidate requiring implementation and measurement; it is not delivered
by this experiment.

[Rejected shader, comparisons, build hashes and device records](../benchmarks/foundation/results/2026-10-03-f8-blur-squared-rejected.json).

## Tiled compute blur experiment (2026-10-03)

Implemented an 8×8 compute blur sharing an 11×11 halo of reconstructed radial
depths and AO values. It preserves the sixteen taps, full resolution, depth
threshold and RG output precision. Portable storage requires RGBA8 instead of
RG8 for the blur target, adding 7,571,520 bytes at 1320×2868. All 96 renderer tests
passed (zero failed, five ignored), and 63 comparison images were byte-identical.
The candidate iOS release built. Clippy flagged the temporary one-element pass
loop; this unretained candidate did not pass that check.

The candidate's bounded phone capture had median-of-window-p50 GPU time 18.461 ms
and AO time 10.877 ms. A subsequent original-build control was slower still:
19.078 ms GPU, 11.834 ms AO. Both slowed relative to the earlier 17.2 ms control,
so this comparison does not establish either a gain or a regression from the
compute change. Nominal thermal readings do not establish stable clocks. Exact
launch PIDs 762 and 802 were terminated before log copying. The original build
was restored by the control capture and is stopped.

The compute candidate was archived and fully reverted; the earlier AO-kernel
improvement remains. No extra compute path or target-memory cost was retained.
A benchmark-only frozen-scene option is now available for subsequent GPU
isolation: 120 fixed settling ticks, then timeScale 0, with unchanged rendering.
It is clearly labelled, off by default, and has passed Dart analysis; it has not
yet been measured on the phone. It must never be counted as live-game throughput.

[Candidate patch, correctness checks and both device captures](../benchmarks/foundation/results/2026-10-03-f8-tiled-blur-experiment.json).
F8 remains open; sustained performance and the cause of phone run-to-run drift
are unresolved.

## Retained tiled blur after fixed-scene comparison (2026-10-03)

Revisited the archived compute candidate using the fixed-scene diagnostic. Each
run settles exactly 120 ticks, then captures with timeScale 0; every measured
window has zero fixed ticks and 1,767 visible instances. Medium settings, native
resolution, normal 3,000 µs acquisition wait and the same assets are retained.
The final implementation also removes the obsolete estimate binding of the blur
output and deletes the fragment-blur entry point. There is one production blur
path. The 8×8 tile shares an 11×11 halo of radial distances and RG estimates while
preserving the original sixteen taps, threshold and output precision.

Median of window p50 GPU times after 1.5 seconds, and median texture acquisitions:

| Fixed-scene build | GPU frame (ms) | AO stage (ms) | New textures/s |
| --- | ---: | ---: | ---: |
| Baseline | 23.749 | 13.178 | 41.98 |
| Initial tiled candidate | 22.285 | 11.942 | 43.95 |
| Baseline repeat | 22.806 | 12.757 | 42.89 |
| Final tiled implementation | 19.133 | 10.614 | 50.95 |
| Following baseline repeat | 23.870 | 12.435 | 41.95 |

This supports retaining the GPU optimization. These are renderer-isolation
results, not live-game FPS; the earlier unretained experiment remains archived.
The memory tradeoff is two additional bytes per internal pixel for portable
RGBA8 storage (7,571,520 bytes at 1320×2868), plus 1,936 bytes of shared memory per
workgroup. GPU allocation accounting includes the larger output. No quality,
resolution or sample-count reduction was used.

The final live-scene release was also built and captured with frozenScene=false,
normal acquisition wait and no CPU readback. It completed with roughly 14–28
acquisitions/s after startup, declining later as before. GPU frame window p50
remained around 17–18 ms; AO around 10.1 ms in most windows. No overall live-game
throughput gain is established; simulation and catch-up remain bottlenecks.
All new captures reached their bounded deadline with nominal thermal samples;
exact launch PIDs 850, 875, 885, 898, 921 and 923 were terminated before retrieval.
The normal moving-scene final app is installed and stopped.

Final validation: 111 renderer/Apple tests passed, zero failed, seven ignored;
all-target release Clippy passed for both crates. All 63 captured shading images
are byte-identical to the original blur baseline. A permanent GPU/scalar-reference
regression covers 1×1, 1×17, 7×3, 8×8, 9×17 and 23×19 targets, including partial
tiles, screen edges, depth discontinuities and output channels. Frozen and live
iOS release builds passed. F8 remains open: eight-second diagnostics do not prove
sustained performance, physical scanout or thermal stability.

[Source/binary hashes, all comparisons and validation](../benchmarks/foundation/results/2026-10-03-f8-tiled-blur-retained.json).

### Render membership avoids pose-driven rescans (2026-10-03)

Mac sampling found repeated membership checks during extraction. The render list
now consumes the existing ECS structural journal: component additions/removals
and entity lifetime changes. Transform, visibility and other component values
are still read fresh for every snapshot. No additional cache or API was added.

The existing extraction qualification (10,000 spatial entities, 1,000 render
records, 60 samples per mode) measured dense-motion median extraction at
1.370 ms before and 0.134 ms after. A following baseline repeat was 1.363 ms;
the candidate repeat was 0.129 ms. This is about 90% less extraction time in
this focused workload, not a whole-frame FPS claim. Static/sparse timings varied;
no improvement is claimed for those modes. No phone measurement was run for
this change.

Permanent regressions cover value-history overflow, fresh poses and visibility,
owned snapshots, component removal/readdition, inactivity, missing transforms,
destruction and replacement entities. The value-churn regression fails on the
original implementation. The earlier synchronization test now checks the fresh
rendered position while expecting zero membership candidates on movement.
Core release tests: 373 passed, zero failed, one manual qualification ignored;
the extraction suite including that qualification passed separately (four tests).
All-target release Clippy passed. F8 remains open.

[Measurements and source hashes](../benchmarks/foundation/results/2026-10-03-f8-render-membership.json).

### Render membership: live iPhone follow-up (2026-10-03)

The updated normal moving-scene iOS release built and completed its eight-second
diagnostic, with CPU readback disabled and the normal 3000 µs acquisition wait.
PID 1004 was terminated before retrieval. All sampled thermal states were nominal.

After startup, early window-p50 extraction measured 1.84–2.03 ms versus
3.18–3.30 ms in the previous live capture. New texture acquisitions were
28–31/s early and 17–20/s late, versus 26–28/s early and 14–18/s late previously.
These historical sequential runs support further investigation, not a controlled
overall speedup estimate. GPU window p50 remained about 16.5–17.3 ms.

The late slowdown persists: simulation window-p50 grows to 42–51 ms, and
extraction rises again to 3.14–3.27 ms. A possible cause of the latter is
structural churn combined with journal overflow during multi-tick catch-up;
this has not yet been measured. Next: instrument membership refresh/reset work
and correlate it with tick counts before choosing another production change.
F8 remains open; no sustained 60/120 FPS or thermal qualification is claimed.

[Raw capture, comparison windows and bundled binary hashes](../benchmarks/foundation/results/2026-10-03-f8-render-membership-device.json).

### Structural history survives dense value traffic (2026-10-03)

A controlled reproduction confirmed that one temporary entity created/destroyed
before dense transform writes forced all 10,000 spatial entities through render
membership again: the general change journal had lost the structural event.
The shared ECS change API now retains structural events in a separate bounded
history, using the same revisions and writer barrier. Membership-only readers
use this history; component-value readers retain their existing semantics. This
adds up to 16,384 compact revision/entity entries per world, allocated as needed.
Structural-history overflow still explicitly requests a safe rebuild.

The extended extraction qualification measures dense movement plus churn at
1.368 ms before and 0.159 ms after (0.136 ms on repeat), with membership candidates
falling from 10,000 to one. Permanent coverage checks component changes, entity
generations, independent consumers, value and structural overflow, bulk writes,
foreign/initial cursors and outstanding writer guards. ECS/core release suites
including the manual qualification: 404 passed, zero failed or ignored. Release
all-target Clippy and the iOS release build passed.

The normal live iPhone capture completed eight seconds with nominal thermal
samples; exact launch PID 1031 was terminated before retrieval. Late extraction
window-p50 stayed at 2.04–2.19 ms, versus 3.14–3.27 ms in the preceding capture.
This supports eliminating the late membership rebuild spike. However simulation
still reaches 49–54 ms per host frame and acquisition throughput falls into the
teens. These are sequential bounded runs, not a sustained FPS qualification.
F8 remains open; the next priority is remaining simulation/per-tick cost.

[Raw device capture, microbenchmark samples and validation](../benchmarks/foundation/results/2026-10-03-f8-structural-history.json).

### Physics hierarchy traversal reuses scratch storage (2026-10-03)

Sampling identified per-root subtree list allocations in physics synchronization.
The traversal now uses one retained stack and reads child lists by shared borrow.
It still visits descendants through already-affected children and processes the
resulting set in deterministic entity order. No simulation settings or workload
population were reduced.

The final Mac CPU comparison ran sequentially with builds/tests finished, using
300 Wobble behaviours, 40 warmup ticks and 300 measured ticks per fresh world.
Median physics time fell from 5.522 to 5.164 ms (6.5%), native tick time from
8.266 to 7.923 ms (4.1%), and total tick time from 10.408 to 10.043 ms (3.5%).
Initial runs overlapped builds/tests and are retained as exploratory data only.

A permanent regression changes an ancestor's body while its child is independently
dirty, verifying that an unchanged grandchild collider follows body attachment
and removal. Physics/core release suites: 405 passed, zero failed, one manual
extraction qualification ignored. All-target release Clippy and iOS build passed.

The live iPhone release completed its eight-second deadline; exact PID 1034 was
terminated before retrieval and all thermal samples were nominal. Post-startup
acquisitions were about 30–32/s early and 20–22/s late. Late simulation window-p50
was about 38–40 ms compared with 49–54 ms in the preceding run. These sequential
captures do not establish a controlled overall FPS gain. GPU frame p50 remained
about 16.4–17.1 ms, and F8's sustained performance gate remains open.

[CPU samples, raw device capture and validation](../benchmarks/foundation/results/2026-10-03-f8-physics-traversal.json).

### Physics affected-list experiment rejected (2026-10-03)

A fresh Mac CPU sample identified ordered-set maintenance among remaining sync
costs. Two retained-vector variants were tested against the previous hierarchy
buffer build with the same full workload, 300 behaviours, 40 warmup ticks and
300 measured ticks. Neither established a consistent improvement. In the final
sequence physics medians were 5.283 ms baseline, 5.448 ms candidate, 5.494 ms
control, and 5.290 ms candidate repeat. The affected-set change was reverted.

The earlier hierarchy-buffer optimization remains. The hierarchy regression now
also checks subsequent clean synchronization and retirement of a destroyed leaf
collider. Restored physics tests and all-target release Clippy pass. No new phone
run was warranted; the previous measured app remains stopped. F8 stays open.

[Rejected experiment and validation](../benchmarks/foundation/results/2026-10-03-f8-physics-candidates-rejected.json).

### Native physics phase timings (2026-10-03)

Optional `Physics::set_profiling` / `step_profile` diagnostics now separate sync,
commands, characters, solver, writeback and events. The phases do not overlap;
total includes timing/bookkeeping overhead. Profiling is disabled by default and
performs no clock reads while disabled. Samples clear on disable and correctly
zero unexecuted phases on empty-world steps. Stripped runtimes return no sample.

The headless `physics_profile` example loads the cooked scene with its assets,
warms 40 ticks and measures 300 ticks with profiling disabled and enabled in
fresh worlds. Final transforms and body/character motion states match exactly.
The final profiled run's phase medians were sync 2.621 ms, solver 0.796 ms,
writeback 0.165 ms and events/statistics 0.090 ms; inner physics total 3.676 ms.
These independently calculated medians need not sum exactly. Core propagation
before physics and event publication after physics are outside this total.

This is a Mac native-only scene: no Dart callbacks, rendering or catch-up. It
identifies synchronization as the largest measured native physics phase; it does
not establish a phone FPS improvement. The next optimization target is gathering
and applying physics synchronization inputs. No phone run was made this turn.

Physics/core tests: 406 passed, zero failed, one manual qualification ignored.
Stripped physics: 10 passed. All-target release Clippy and the final benchmark
check pass. F8 remains open.

[Per-tick samples, equality check and validation](../benchmarks/foundation/results/2026-10-03-f8-physics-phases.json).

### Nested collider checks reuse gathered inputs (2026-10-03)

The nested-collider unchanged check reread WorldTransform, Collider and Layer
and decomposed scale after sync had already gathered and validated them. It now
uses the existing SyncEntry. Body-home/type lookup still happens after body
creation/removal, preserving same-tick hierarchy behavior. No cache was added.

A focused qualification moves one body with 5,000 child colliders, with scene
propagation outside the timer and no solver. Across 20 warmup/60 measured
iterations, synchronization medians were 1.462 ms candidate, 1.874 ms baseline,
and 1.486 ms candidate repeat: approximately 21–22% less time for this path.
The general native dense-scene runs did not improve: sync medians were
2.664–2.672 ms candidate versus 2.634–2.635 ms baseline/control. This is a targeted
nested-collider improvement, not a whole-game or phone FPS claim.

Permanent coverage verifies collider-handle preservation plus updated friction,
collision membership, scaled bounds and relative pose, followed by collider
removal. Existing same-tick body creation/removal and descendant tests pass.
Physics/core release suites: 407 passed, zero failed, two manual qualifications
ignored; the nested qualification passed separately. All-target release Clippy
passed. No phone run was made for this change; F8 remains open.

[Focused samples, full-workload comparison and validation](../benchmarks/foundation/results/2026-10-03-f8-nested-inputs.json).

### Common collider sync reuses pose and comparisons (2026-10-03)

The unchanged check now tests the world pose before comparing other components,
so moving objects avoid comparisons that cannot make them unchanged. Entry
updates compare collider shape once. Collider placement consumes the already
validated world pose instead of decomposing the matrix again; joined colliders
still use their relative transform, and refits compute the world pose lazily.

A focused benchmark moves and rotates 5,000 unparented colliders, warms 20
iterations and records 60 synchronization samples. It verifies all candidates
were processed and that a collider reached the final position. Sync medians were
1.612 ms candidate, 1.809 ms baseline and 1.596 ms candidate repeat, an 11–12%
reduction. An initial harness draft failed to dirty stateless scene propagation;
its idle timings were discarded, and motion/candidate assertions were added
before recording the reported runs.

Full native dense-scene comparisons remain noisy: synchronization medians were
2.884 ms baseline, 2.579 ms candidate, 2.752 ms control, and 2.716 ms candidate
repeat. Whole-tick timings did not consistently improve, so no overall FPS gain
is claimed. The new regression checks stable collider identity and correct pose
with nonuniform scale and a rotated, nonzero offset. Physics/core suites passed
408 tests; the final physics suite and focused qualification passed separately.
All-target release Clippy and the iOS release build passed.

The first phone install attempt lost its connection. Retry installed the app,
but iOS denied launch because the phone was locked; no PID or new performance
capture was obtained. Unlock requested; bounded device validation is pending.
F8 remains open.

[Benchmarks, regression validation and device attempts](../benchmarks/foundation/results/2026-10-03-f8-sync-reuse.json).

### Common sync reuse: unlocked iPhone follow-up (2026-10-03)

After the user unlocked the phone, the normal moving-scene release completed
its eight-second diagnostic. CPU readback and frozen-scene mode were disabled;
acquisition wait remained 3000 µs. Exact launch PID 1632 was terminated before
retrieving the capture. All sampled thermal states were nominal.

After startup, acquisitions were about 31–32/s early, then 23–26/s late. Late
simulation window-p50 was 29–31 ms; extraction 1.93 ms; GPU frame p50 about
16.5 ms (17.1–17.2 ms earlier). The prior live capture ended near 20 acquisitions/s
with roughly 38–40 ms simulation windows. These captures occurred hours apart
and include intervening retained changes, so they do not establish a controlled
overall speedup or attribute it to one change. Catch-up and GPU cost remain
substantial. Eight nominal-thermal seconds do not qualify sustained 60/120 FPS.
F8 remains open; the updated phone app is installed and stopped.

[Raw capture and historical comparison](../benchmarks/foundation/results/2026-10-03-f8-sync-reuse.json).

### Every-tick phone timing isolates frame remainder (2026-10-03)

The updated frame-level CPU capture (PID 1637) still showed modest last-native-tick
growth while per-frame work increased with catch-up. To avoid assuming the last
tick represented all ticks, the SDK now supports opt-in `FrameTimes.collectTickTimes`.
It reads system timings after callback transactions close and publishes immutable,
owned samples per frame. Samples clear even on zero-tick frames. Collection is
disabled by default and does not add gameplay objects or callbacks.

An initial diagnostic behaviour attempted this query inside a gameplay transaction.
The query was rejected and no tick samples were recorded; the completeness check
rejected that capture (PID 1670). That probe was removed; transaction rules were
not relaxed. Final capture PID 1686 contains complete frame-matched tick samples.
All three exact PIDs were terminated, with eight-second deadline stops. Valid
captures sampled nominal thermal state; this does not prove sustained stability.

The final capture's late first/last catch-up native steps remain about 7.6–8.0 ms.
The frame native timer minus the sum of *all* its native step timings grows from
roughly 40–47 µs per tick early to about 1.99–2.06 ms late. Physics remains roughly
5.0–5.2 ms per tick. Readback itself is about 1 µs median. The unexplained remainder
includes SDK/bridge/frame work, not an independently timed engine system. This
identifies where to instrument next: bridge step, snapshots, diagnostics and event
draining. It does not yet identify a specific slow call or justify changing tick
scheduling. No FPS improvement is claimed.

Validation: five SDK timing tests, eight Python workload tests, Dart analysis,
and diagnostic/normal iOS release builds passed. The normal release disables both
CPU readback flags. F8 remains open.

[Analysis and raw-capture reference](../benchmarks/foundation/results/2026-10-03-f8-all-ticks.json).

The normal release was reinstalled after the diagnostics and left stopped.

### Sparse resource tracking survives dense pose history (2026-10-03)

Opt-in per-tick SDK timing now separates subscriptions, the native bridge step,
observation sweep, snapshot copy/index, diagnostics and event draining. Collection
is latched at frame entry and remains disabled by default. Samples are owned and
immutable. Stage clocks are included in native timing; system readback is outside
native/behaviour timing. Means accompany medians so periodic spikes remain visible.

The unchanged-scene capture (PID 1742) isolated the overhead to the bridge step:
average bridge time outside native `step` grew from 0.84–0.93 ms early to 1.91–1.97 ms
late. Individual spikes were roughly 5–6 ms. Snapshot/index and event processing
remained in the tens of microseconds; frame setup contributed only a few microseconds.

A regression reproduced unnecessary resource-owner resets: one spawned owner,
followed by 20,000 unrelated transform writes, overflowed general change history.
Asset tracking then rebuilt all live owners despite losing no relevant values.
ECS history now remembers the last discarded nonstructural write per component
domain and stores component identity with structural events. On general-history
overflow it can merge retained structural events with retained selected values.
Lost selected values, unenumerated selected bulk writes, missing structural history,
invalid cursors and outstanding writers retain their existing reset/barrier behavior.
History remains bounded; loss metadata has one entry per written component type.

With this change, capture PID 1752 reduced mean bridge overhead outside native
`step` to 0.024–0.047 ms across the run, including 0.033–0.036 ms late. Native step
medians were somewhat higher (late 8.0–8.2 ms versus 7.7–7.8 ms); these sequential
captures do not isolate CPU-frequency, heat or instrumentation effects. They show
removal of the bridge spikes, not a controlled overall FPS percentage.

The normal release (PID 1800; both CPU readback flags disabled) completed eight
seconds at approximately 27–31 new textures/s, including 29–31/s late. All three
captures reported nominal thermal state and exact launched PIDs were terminated.
No scene, tick-rate, resolution or graphics-quality reduction was made. GPU cost,
catch-up and sustained 60/120 FPS remain open. The normal app is installed and stopped.

[Stage analysis, regression evidence and raw captures](../benchmarks/foundation/results/2026-10-03-f8-domain-history.json).

Validation also covers SDK runtime-resource ownership and typed assets. The first
expanded SDK invocation found an old globally installed CLI; rerunning with the
workspace `FYNKEL_CLI` passed all 15 tests. The stalled debug Rust build was stopped
and validation moved to the release profile; no product code or test was disabled.

Final checks: 406 ECS/core release tests passed (one existing manual qualification
ignored), the native animation/resource-ownership test passed, 15 SDK tests passed,
nine Python checks passed, Dart analysis passed, and release Clippy passed with
warnings denied. The callback regression now checks recoverable activity changes
and separately requires reset for a lost name value. Diagnostic and normal iOS
builds passed; `git diff --check` passed.

### AO arithmetic experiments rejected (2026-10-03)

Four eight-second frozen-scene captures tested two shader simplifications at the
same resolution, quality and settled state. Exact PIDs 2550, 2556, 2565 and 2574
were terminated; all sampled thermal states were nominal. The normal scene is
still live; freezing was limited to GPU attribution.

Selecting the depth neighbour before world reconstruction reduced the AO-stage
window-p50 median from 10.808 ms to 10.374 ms (control 10.848 ms), but whole-frame
GPU time stayed at 19.178 ms and acquisitions at approximately 51/s. Of 63 image
comparisons, one differed in 41 colour channels, by at most 3/255. The change was
rejected because it did not improve total GPU frame time.

Projecting the hemisphere basis once per pixel instead of each sample preserved
all 63 comparison images exactly. It also failed to improve performance: AO
11.076 ms, whole GPU frame 19.508 ms, approximately 50 acquisitions/s. This change
was rejected too. Both candidates passed 53 shading tests (two existing ignored
qualifications); renderer release Clippy passed with warnings denied. The first
baseline image dump lacked its destination directory; after creating it, the
baseline suite passed. The original shader is restored.

These are sequential short diagnostics, not sustained qualification. GPU pass
spans overlap and cannot be summed. A frozen GPU frame near 19 ms still exceeds
both the 16.67 ms/60 FPS and 8.33 ms/120 FPS budgets before gameplay is considered.
The next investigation should separate depth prepass/resolve, AO estimation and
blur on the physical device before choosing a larger renderer change. No quality
reduction or production performance change is retained from these experiments.

[Measurements, image comparisons and raw captures](../benchmarks/foundation/results/2026-10-03-f8-ao-arithmetic.json).

The restored normal release was rebuilt, reinstalled, and left stopped.

### Live-state confirmation and device GPU stage breakdown (2026-10-03)

The user reported that the prior frozen runs did not move. Those runs intentionally
paused simulation for GPU isolation; the benchmark also intentionally pauses at
its eight-second deadline. The app now prominently distinguishes LIVE TEST,
FROZEN GPU TEST and TEST COMPLETE. Each stats row includes its simulation tick and
mode. The user confirmed visible movement in the new live run before it ended.
PID 2666 advanced from tick 56 to 476 across the reported windows and was stopped.

An opt-in GPU file trace now makes the existing stage timestamps retrievable from
the physical phone. Separate depth-resolve timestamps have a per-frame presence
flag so skipped passes cannot expose stale values. The live trace's median spans
were depth 0.575 ms, resolve 0.845 ms, estimate 5.902 ms, blur 4.196 ms and total
screen-space 10.192 ms. These spans overlap; they are not additive or exclusive
shader costs. Tracing is diagnostic overhead and is off by default.

A 16x16 blur tile experiment preserved all 63 image comparisons and passed the
expanded edge/depth-discontinuity regression. Frozen captures PID 2679 (8x8) and
2683 (16x16) measured blur at 4.135 and 4.087 ms respectively, with whole-frame
window-p50 medians of 19.137 and 19.090 ms. This is not a meaningful speedup;
the original 8x8 shader and dispatch were restored. All three completed runs
sampled nominal thermal state and their exact PIDs were terminated.

The runner now gives each launch a unique ID echoed by the app, and uses a unique
GPU trace filename. Missing/mismatched IDs and missing/malformed trace samples
fail qualification, preventing old successful logs from covering a failed launch.
The final live build installed, but iOS blocked the final nonce verification while
the phone was locked; no PID was launched in that attempt.

Checks: 97 renderer release tests passed (five existing ignored), 11 Python tests
passed, release Clippy with warnings denied passed, Flutter analysis and release
build passed. F8 remains open; no production rendering speedup is claimed.

[Stage results and raw captures](../benchmarks/foundation/results/2026-10-03-f8-gpu-stages.json).


After unlock, PID 2757 ran but the strict freshness check rejected its log:
this Flutter iOS host did not expose the launch ID through Dart's
`Platform.environment`. The protocol now copies a one-use capture-request JSON
into the app sandbox before launch; the app reads, deletes and echoes its ID.
Native GPU tracing continues to use its native environment. No freshness check
was weakened. The Python capture tests pass, Flutter analysis passes and the
normal release rebuilt successfully.

The first request-based run (PID 2772) verified its ID but paused after 1.05 seconds
on `lifecycle-inactive`; it remains marked interrupted. The retry (PID 2788)
completed its eight-second deadline, matched the fresh ID, advanced from tick 57
to 477 across reporting windows, and returned 31 GPU samples. All reported thermal
states were nominal. Median GPU spans: depth 0.563 ms, resolve 0.831 ms, estimate
5.902 ms, blur 4.180 ms, total screen-space 10.158 ms (overlapping spans). All three
exact PIDs were terminated. Final verification is complete; the live diagnostic
build remains installed and stopped. F8 performance qualification remains open.

## Blur bounds and predicated accumulation — 2026-10-03

The retained 8×8 compute blur now clips the 4×4 filter rectangle once per output,
reuses the shared-memory base and depth threshold, and selects accepted samples
without a per-tap branch. Resolution, effects, sample count, radial threshold,
row-major accumulation order and workgroup dimensions are unchanged.

Four alternating frozen-scene captures support retaining this small improvement:

| Run | Exact stopped PID | Blur median span | GPU frame window-p50 median | Fresh acquisitions/s median |
| --- | ---: | ---: | ---: | ---: |
| Original | 2904 | 4.167 ms | 19.060 ms | 51.00 |
| Candidate | 2915 | 3.723 ms | 18.736 ms | 51.99 |
| Original control | 2918 | 4.080 ms | 19.182 ms | 50.99 |
| Candidate repeat | 2919 | 3.751 ms | 18.668 ms | 52.00 |

All completed their eight-second deadline and matched their launch IDs. Timed
windows reported nominal thermal state. The repeat also emitted a zero-frame
startup window with unknown thermal state and no GPU/acquisition measurements;
it is preserved in raw evidence and excluded from timing medians. Pass spans
can overlap; these are not additive exclusive shader costs or sustained results.

All 63 rendered comparison images were byte-identical. The scalar regression now
covers 12 dimensions under identity, perspective and orthographic reconstruction,
including one-pixel rows/columns, 2×2/3×3 images, partial workgroups and depth
breaks. Its original one-byte quantization tolerance is unchanged. All 97 renderer
release tests passed (five existing ignored); release Clippy passed with warnings
denied. The expanded scalar test passed again after correcting deprecated camera
constructors to the same API used by the engine.

The final normal live build (PID 2921) completed at 8009 ms, advanced from tick 54
to 474 across reporting windows, returned 32 GPU samples and was stopped by its
exact PID. Thermal readings were nominal. Its blur span median was 3.764 ms;
fresh acquisitions were about 27.93/s, so this does not establish a live-game FPS
gain or meet either sustained target. The moving test remains installed and
stopped. F8 remains open.

[Results and raw captures](../benchmarks/foundation/results/2026-10-03-f8-blur-bounds.json).

## Root world-pose preparation — 2026-10-03

The shared scene conversion used by physics writeback and native placements now
avoids work that roots do not need: ancestor-cycle tracking does not allocate
until a parent exists, roots use their requested world position/rotation directly,
and all-root batches skip the unused prepared-ancestor lookup table. The same
API still deduplicates with last-request-wins, orders parents before children,
validates every output and publishes nothing during preparation. Mixed batches
retain their prepared ancestor transforms. Scale, including valid zero/negative
local scale, is preserved. Cycle, singular-parent and invalid-pose checks remain.

In the manual Mac release probe (5,000 roots, 40 warmup batches, 300 measured
batches), original/control medians were 817/844 µs; final/repeat medians were
373/358 µs. Full dense-scene native-only runs showed much smaller and variable
changes. The final candidate/control unprofiled step medians were 7.503/7.607 ms;
profiled writeback medians were 136/178 µs. These do not establish a controlled
whole-game speedup. Each native run also verifies identical simulation outcomes
with profiling enabled/disabled; this is not a cross-build state comparison.

The initial phone baseline exposed a runner bug: `devicectl` rejects an empty
`--environment-variables {}` argument when tracing is disabled. The runner now
omits that option when there are no variables. The added regression failed before
the fix, and all 11 Python tests passed afterward. The failed attempt launched no
PID and remains in the evidence. A normal untraced baseline then completed under
PID 2978, and the first candidate under PID 2991; exact PIDs were stopped. Their
fresh-acquisition medians were 28.14/28.15 per second, with nominal thermal readings.
This earlier candidate omitted root matrix work but still populated its unused
lookup table. The final root-only lookup removal built and installed, but its
first phone launch was blocked by Locked without launching a PID.

Final source checks: 433 scene/core/physics release tests passed, four manual
qualifications ignored; the manual root probe passed separately. Release Clippy
for all three crates and all targets passed with warnings denied. The new
regression covers duplicate root requests, invalid final poses, scale preservation
and side-effect-free failure alongside existing mixed hierarchy/cycle tests.

[Results and raw captures](../benchmarks/foundation/results/2026-10-03-f8-root-poses.json).

After unlock, the final live build completed under PID 3040, matched its launch
ID and advanced from tick 57 to 476 across reporting windows. It stopped at its
eight-second deadline; the runner terminated that exact PID. Thermal samples were
nominal. Median fresh acquisitions were 31.38/s and the median of GPU-frame window
p50s was 16.361 ms. Simulation/behaviour/native window-p50 medians were
10.436/2.226/8.024 ms respectively; these are per-frame summaries, not per-tick
measurements, and their medians cannot be added. The higher acquisition rate in
this short run is encouraging but does not isolate the effect of the pose change
from run-to-run variation. The live app remains installed and stopped; F8 stays
open and sustained 60/120 Hz targets remain unqualified.

## Shared physics synchronization reads — 2026-10-03

Physics synchronization now reads candidate inputs through one shared ECS query
view, resolving each candidate's identity once. Previously every component read
repeated identity resolution, component lookup and borrow acquisition. The query
uses the existing internal ECS API; its scope ends before physics ownership
components change. Empty candidate sets do not construct a view. Candidate order,
validation, nested-body resolution and removal handling remain unchanged.

The focused 5,000-moving-collider Mac probe measured sync at 1.571 ms before and
1.425 ms after. Alternating dense-scene native-only runs give stronger evidence:

| Run | Unprofiled native step median | Profiled sync median |
| --- | ---: | ---: |
| Original | 7.559 ms | 2.539 ms |
| Candidate | 7.046 ms | 1.956 ms |
| Original control | 7.528 ms | 2.507 ms |
| Candidate repeat | 7.021 ms | 1.959 ms |

Each mode warms 40 ticks and measures 300. This is about a 7% native-step reduction
in this Mac workload, not an overall game FPS improvement. Every native run also
verified equal profiled/unprofiled simulation outcomes. No quality or gameplay
workload reduction was made.

Live iPhone captures completed under PID 3046 (original) and PID 3049 (candidate),
with matching launch IDs, ticks 58 through 478 across reporting windows, nominal
thermal samples, eight-second deadlines and exact PID termination. Median fresh
acquisitions were 31.48/s and 32.06/s. Native CPU window-p50 medians were 8.124 and
7.365 ms; simulation medians were 10.500 and 9.737 ms. These are per-frame window
summaries, not per-tick measurements or pooled frame percentiles. Short sequential
runs do not establish sustained FPS, and GPU/frame-delivery costs still matter.

410 physics/core release tests passed (three existing manual qualifications
ignored), including a new mixed-membership regression covering inactive objects,
missing/restored transforms and destroyed/replaced entities. Release Clippy for
both crates/all targets passed with warnings denied. The release app remains
installed and stopped. F8 remains open.

[Results and raw captures](../benchmarks/foundation/results/2026-10-03-f8-sync-view.json).

## Physics hierarchy traversal — 2026-10-03

Synchronization now gathers parent/child data through a shared ECS query and
inserts only live descendants into the affected set; the initial roots are already
present. It still visits already-affected children, filters stale descendants and
applies final physics changes in entity order. Empty candidate sets skip the view.

The focused 5,000-moving-collider Mac test measured 1.444 ms before and 1.242 ms
after. Alternating native-only dense-scene runs measured unprofiled step medians
at 7.157/7.119 ms for original/control and 6.739/6.736 ms for candidate/repeat.
Profiled synchronization medians were 1.984/1.955 ms versus 1.536/1.555 ms. This
supports a roughly 6% CPU step improvement in this Mac workload. Each mode warms
40 ticks and measures 300; profiling-on/off simulation outcomes remain equal.

411 physics/core release tests passed (three existing manual qualifications
ignored), and release Clippy for both crates/all targets passed with warnings
denied. The new hierarchy regression changes a root body and a non-physics middle
node simultaneously while leaving the leaf transform untouched; the grandchild
still follows body attachment/removal. The direct-sync test checks effective
parent-relative placement, because Rapier updates cached world collider poses in
the solver phase. Existing lifecycle and invalid-transform checks also pass.

Live iPhone captures PID 3058 (original) and PID 3065 (candidate) both completed
the eight-second deadline with matching IDs, advancing ticks and nominal thermal
samples; their exact PIDs were stopped. Native CPU window-p50 medians were
7.325/6.946 ms, simulation 9.692/9.407 ms, GPU frame 16.813/16.548 ms, and fresh
acquisitions 26.46/30.01 per second. These are short sequential observations and
medians of per-frame window summaries, not sustained or per-tick measurements.
Frame delivery still varies substantially. The candidate live app remains
installed and stopped. F8 stays open.

[Results and raw captures](../benchmarks/foundation/results/2026-10-03-f8-sync-hierarchy.json).

## Correlated dense-scene diagnosis

The [2026-10-03 frame diagnosis](foundation-f8-performance-diagnosis.md) correlates
host work, native texture state transitions and Flutter raster timing across nine
bounded captures. Normal live delivery was 29.4–29.7 new textures/s; half-resolution
live rendering reached 58.2; frozen half-resolution rendering reached 118.6.
Eight buffers improved throughput but increased queue latency. This establishes
separate CPU, pixel-dependent GPU, and buffer coordination constraints. It is a
prioritized diagnostic result, not a production performance fix or an F8 pass.

2026-10-04: bounded live dense-5000 physics-phase capture completed 479 ticks,
matched to all SDK tick samples. Synchronization median 1.873 ms; solver 0.996 ms;
adapter total 3.185 ms. App reached its eight-second deadline and exact PID 4879
was terminated. Thermal samples were nominal, with no sustained qualification
claim. [Detailed evidence](foundation-f8-physics-phases.md).

2026-10-04: four bounded synchronization captures completed with 474/480/478/479
matched ticks. Repeated leaf hierarchy optimization lowers hierarchy medians
0.287→0.232 and 0.294→0.239 ms. Whole-step medians do not improve. All runs ended
at the deadline, their exact PIDs were terminated, and thermal samples were
nominal. [Full comparison](foundation-f8-sync-hierarchy.md).

2026-10-04: four bounded entry-lookup comparisons completed with 480/480/479/480
matched ticks. Application medians fall 0.688→0.576 and 0.733→0.604 ms; whole-step
results are mixed. All runs reached the deadline, recorded nominal thermal state
and had their exact PIDs terminated. The retained candidate is installed and
stopped. [Detailed comparison](foundation-f8-entry-lookup.md).

2026-10-04: core physics-phase diagnostic captured 480 matched ticks. Median
propagation 1.135 ms, adapter 2.972 ms, event/rejection handling 0.0011 ms. The app
reached the eight-second deadline; exact PID 5041 was terminated. Thermal samples
were nominal. [Evidence and limits](foundation-f8-core-physics-phases.md).

2026-10-04: retained shared propagation reads after two live phase comparisons
(1.148→0.833 and 1.135→0.864 ms/tick). Separate captures without phase tracing or
all-tick readback deliver 43.428 vs 43.444 acquired frames/s. All six runs reach the
deadline and exact PIDs are stopped. First control has one unknown startup/thermal
window; other thermal samples are nominal. [Evidence](foundation-f8-propagation.md).

2026-10-04: eight bounded redundant-dirty-write captures completed and exact PIDs
were stopped. Local propagation costs fell, whole-step timing was mixed, and
untraced acquisition regressed once but tied in reverse order. The candidate was
removed; the last installed/stopped app is the retained control. [Evidence and
limits](foundation-f8-dirty-writes.md).

2026-10-04: four bounded frozen-scene blur-tile comparisons completed. Original
8×8 controls measured 3.726/3.791 ms blur spans and 47.996/48.242 acquisitions/s;
16×16 and 16×8 measured 4.424/4.475 ms and 44.211/46.338/s. All thermal samples
were nominal, deadline stops completed and exact PIDs were terminated. Larger
tiles were removed; the original app is installed and stopped. [Evidence](foundation-f8-blur-tiles.md).

2026-10-04: four bounded frozen AO-neighbour comparisons completed with nominal
thermal samples and exact PID termination. Candidate acquisitions 47.116/47.966
versus controls 48.369/49.363 per second; no useful whole-frame GPU reduction.
Candidate removed; original retained app installed and stopped. [Evidence and
limits](foundation-f8-ao-neighbour.md).

2026-10-04: one valid Metal baseline and six failed PID-attachment attempts;
all exact launched PIDs stopped. Gated failures withhold the scene start signal.
The final medium-quality manual handshake check releases the scene and completes
the eight-second deadline; it is not a Metal capture. Phone app stopped.
[Details and remaining profiler limitation](foundation-f8-profiler-gate.md).

2026-10-04: user reconnected USB; three gated exact-PID Metal captures completed
(AO-only, contact-only, normal medium). All used 1320×2868, recorded nominal
thermal samples, reached the bounded deadline and stopped their exact PIDs.
Normal medium was selected last; app stopped. [Comparison and limitations](foundation-f8-metal-effects.md).

2026-10-04: four frozen fixed-blur comparisons completed and exact PIDs stopped.
Candidate GPU medians 17.658/17.643 ms versus controls 19.143/19.203 ms; no reliable
acquisition improvement established. Final control reported fair thermal state,
so testing stopped and cooldown confirmation was requested. Original control app
is installed/stopped; candidate acceptance awaits live testing. [Evidence](foundation-f8-fixed-blur.md).

2026-10-04: user confirmed cooldown. Four live fixed-blur comparisons completed
(control/candidate/candidate/control), all deadline-stopped and exact PIDs
terminated. First three runs nominal, final control fair. GPU spans improved,
but acquisitions were mixed and the reverse control was warmer. Candidate
removed; original app installed/stopped and generated release rebuilt. [Decision](foundation-f8-fixed-blur.md).

2026-10-04: one bounded live frame/all-tick capture completed with nominal thermal
samples and exact-PID termination. 509 host frames and 480 ticks matched; late
frames increasingly execute two catch-up ticks. Original renderer remains
installed/stopped. [Attribution and limits](foundation-f8-live-critical-path.md).

## 2026-10-04 — shared extraction query

Retained shared read-query extraction after 376 release core tests, Clippy,
local qualification and iOS release build. Nominal phone pair: extraction p50
2.067→1.584 ms; new acquisitions 34.32→39.72/s (instrumented, not display FPS).
Repeat extraction 1.589 ms, but thermal fair: stopped testing and excluded the
repeat from controlled throughput acceptance. Cooled reverse-order comparison
and sustained performance remain open. No quality/workload reduction.
See [extraction evidence](foundation-f8-extract-views.md). F8 remains open.

## 2026-10-04 — renderer preparation attribution

Added nine disjoint CPU phase timers and checked host-frame correlation. One
unlocked iPhone diagnostic completed with nominal thermal samples and exact-PID
termination. Instance construction p50 1.209 ms; visibility 0.632 ms; batching and
uploads 0.337 ms. Instances plus visibility account for 71.5% of preparation total.
29 renderer, five Apple and 22 Python tests pass; Clippy and iOS release build pass.
No FPS improvement or sustained qualification claimed. Next measured experiment:
direct record-index lookup in instance construction. Extraction reverse-order
comparison remains pending. See [phase evidence](foundation-f8-prepare-phases.md).

## 2026-10-04 — direct record lookup experiment rejected

Tested a dense array instead of the per-world mesh-record hash map. Instance p50
improved 1.1935→1.079 ms and 1.200→1.072 ms, but acquisitions fell 39.16→32.87/s
in the first pair and tied 39.44→39.46/s in the repeat. Final candidate includes
an unknown thermal window. No repeatable whole-frame gain established; restored
the original source and app. 29 restored unit tests, 55 shading tests and all 68
reference images pass/match. Candidate source/test/evidence archived. F8 remains
open. See [experiment](foundation-f8-dense-lookup.md).

## 2026-10-04 — visibility cost attribution

Added checked nested CPU visibility timing. One eight-second nominal iPhone
capture: bounds construction 54.9%, index update 40.5%, queries 4.6% of visibility
time; medians 0.419/0.160/0.035 ms. Index-update p95 0.784 ms; rebuild/refit cause
not yet isolated. 29 renderer, five Apple and 23 Python checks pass, plus Clippy
and iOS build. Exact PID stopped. No runtime speedup or sustained qualification
claimed. Next: bounds construction and hierarchy-update attribution. F8 remains
open. See [visibility evidence](foundation-f8-visibility-phases.md).

## 2026-10-04 — bounds reduction and hierarchy update classification

Retained bounds reduction: compute fallback spheres only for meshless records;
main-world identity multiplication removed. 31 renderer + 55 shading + five Apple
+ 24 Python tests pass; 68 images identical, Clippy/iOS build pass. Nominal candidate
bounds p50 0.344 ms versus controls 0.468/0.497; preparation also lower. Initial
control has an unknown thermal window; final candidate fair, so stopped and left
cooled repeat/throughput qualification open. New hierarchy classification identifies
initial/membership rebuilds (~0.798 ms) versus dense refits (~0.167 ms); no
cost-triggered rebuilds in first control. Next: membership churn without whole-tree
rebuilds, preserving correctness. F8 remains open.
See [bounds evidence](foundation-f8-bounds-work.md).

## 2026-10-05 — visibility membership repair retained

Small membership changes now repair leaves, refresh all current bounds, reuse
node slots and rebuild traversal metadata without spatial repartitioning. Large
or cumulative changes, cost growth and excessive depth retain full-build paths.
34 renderer + 55 shading + five Apple + 24 Python tests pass; 68 images identical;
Clippy and iOS build pass. Three nominal captures: control 102 rebuilds versus one
initial build per candidate, with 97/87 repairs. Index p95 0.751→0.229/0.216 ms.
Acquisitions 42.77 versus 42.22/43.62/s: no overall FPS gain claimed. Exact PIDs
stopped. F8 and sustained qualification remain open.
See [membership evidence](foundation-f8-membership-repair.md).

## 2026-10-05 — shared pose reads, mobile comparison pending

Implemented shared ECS reads and carried proposal inputs in world-pose preparation,
with unchanged atomic publication. 27 scene + 376 core tests, Clippy and iOS build
pass. Local 5,000-root conversion median 343.334→106.541 µs. Device control native
median 7.1915 ms / lookAtCamera 0.735 ms; candidate 7.088 / 0.638 ms, but candidate
thermal fair. Stopped exact PID and requested cooldown; no mobile gain accepted.
Implementation remains in working tree pending cooled comparison. F8 remains open.
See [pose-read evidence](foundation-f8-pose-views.md).

## 2026-10-05 — shared pose reads retained after normal-thermal repeats

All-nominal candidate/control/candidate eight-second sequence completed, with
exact PIDs stopped. LookAtCamera medians 0.6745 / 0.736 / 0.628 ms support an
8–15% targeted saving. Native ticks 7.385 / 7.202 / 7.020 ms and acquisitions
37.52 / 39.06 / 39.95 per second do not establish repeatable whole-frame gain.
Retained on local correctness and targeted CPU evidence; earlier fair candidate
remains excluded. No quality/workload/rate reductions. F8 and sustained thermal
60/120 FPS qualification remain open. Physics synchronization and GPU screen-space
costs remain the larger priorities. See [pose-read evidence](foundation-f8-pose-views.md).
