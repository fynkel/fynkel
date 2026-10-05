# F8 qualification — open

Date: 2026-10-03. Base revision: `22229b4f1967f4d07a8f0ed968ba64d7342648d5`,
with substantial uncommitted F5–F8 changes. This is a working-tree qualification,
not certification of that commit alone. Per-run source hashes and commands are
retained in `target/foundation-release/*/report.json`.

**F8 is not complete.** Two runtime qualification obligations remain open:

1. The user has no second Mac, so independent-machine macOS execution remains
   unverified. A repeat on the development Mac cannot substitute for it.
2. Physical iPhone functionality passed, but subsequent measurement exposed poor
   GPU throughput and the user reported excessive heat. Mobile smoothness and
   sustained operation require a measured rendering fix and device requalification.
   See [device observations](foundation-f8-device-observations.md).

## Implemented scope

- Repeatable qualification runner with source/machine identity, retained command
  failures, raw measurements and regression comparison; local native baseline
  comparison passed. CI configuration is present, not remotely executed here.
- Workloads cover native and Dart/AOT registered gameplay through 50,000 entities,
  sparse/native synchronization, extraction, lights, animation bounds,
  transparency/uploads, 10,001-asset editor operations/history, scene load/unload,
  repeated worlds and path/Git package consumption.
- Game-owned save/load reference with stable saved IDs and resolved references,
  complete validation before mutation and atomic restore. This is selected
  gameplay data persistence, not a full execution snapshot or network rollback.
- Legacy SDK umbrella removed; ordinary code imports `gameplay.dart`, native
  integrations `advanced.dart`. Maintained examples and generators migrated.
  This is an intentional source break; F8 did not change runtime ABI 5,
  authoring ABI 6 or viewport ABI 14.
- Architecture/ownership decisions reconciled in ADR-0059 and the roadmap.

Reproduction and scope: [qualification guide](foundation-release.md),
[ADR-0059](decisions/ADR-0059-foundation-release-and-persistence.md).

## Captured checks and limits

The initial runner retained two failures: a viewport timing assertion during a
concurrent build, and missing Dart metrics because progress text preceded JSON.
An isolated workspace retry passed unchanged timing assertions (1,172 tests,
12 ignored). The parser was corrected and covered by permanent tests; fresh Dart
captures passed. Later added shading/history workloads have their own checks.
The failed run remains evidence and is not relabeled successful.

The earlier package qualification recorded SDK 613, authoring 16, generator 14,
Flutter 48, editor API 5 and editor 591 passing tests (one existing editor skip).
The registered gameplay/save reference has six passing tests. Maintained games
passed validation, generated-code checks, analysis, tests and 600-tick headless
runs. Stripped runtime checks and wasm release compilation passed; wasm is a
compile-only portability result. See capture reports for exact command scope.

On the M3 Max, the isolated 50,000-row all-write workload measured native p99
21.691 ms, Dart JIT p99 121.073 ms and Dart AOT p99 108.317 ms. These workloads
are not qualified for a 60 Hz all-row rewrite budget. Editor timings measure
native backend operations, not Flutter large-project UI frame latency. GPU
readback benchmarks and live texture delivery are distinct measurements.

## Apple export and device evidence

Fresh generated game release builds succeeded for macOS and iOS, with bundled
assets. Runtime symbols were present and authoring exports absent in the final
frameworks checked. Builds were development signed; this does not qualify App
Store distribution. Local Mac scene/input/pause checks passed.

The user confirmed iPhone host rotation, clicks, background/resume and relaunch;
the fresh exported game loaded its bundled scene and supported joystick movement,
jump and relaunch. The user subsequently confirmed uneven motion and substantial
heat. Instrumentation found roughly 17–23 new scene textures per second in
example windows despite roughly 100–117 game-loop callbacks. GPU work, not
simulation, dominates that trace. Thermal throttling was not instrumented.

Diagnostic corrections and a 120 Hz presentation regression have been added.
The retained full-resolution sample-zero depth copy improved short iPhone
captures from roughly 36–38 to 43–46 new scene textures per second while
preserving resolution, lighting and MSAA. It adds about 14.44 MiB at the tested
resolution. A permanent GPU regression validates the copied samples. This is
partial progress; sustained 60 FPS and heat remain unqualified. Other shader
experiments were rejected or reverted. A subsequent cooled-device, eight-second isolation run improved delivery from
about 38 to 113–119 new textures per second when screen-space effects were
disabled at unchanged resolution/MSAA. The stage is therefore the main measured
bottleneck; disabling it was diagnostic, not a production fix. The app paused
automatically and was terminated. Further phone testing must remain bounded.

## Release decision

Continue foundation development with these limits visible. Do not close F8 or
advertise production mobile performance on this evidence. ADR-0005's licensing
choice also remains a separate requirement before public publication.

### Dense-scene extension

The [dense-scene device captures](foundation-f8-device-observations.md) extend the
open mobile performance gate. The 500-mover mixed workload delivers about 52–54
new textures/s; the 5,000-mover mixed workload delivers about 8–9 and is dominated
by CPU simulation/callback cost. Both use the original medium effects and full
phone resolution. Short nominal-thermal captures do not qualify sustained 60/120
Hz. The remaining work includes CPU scaling and GPU screen-space optimization,
not only the original starter-scene GPU issue. See
[workloads and runner](../benchmarks/mobile/README.md).

CPU isolation follow-up: caching runtime animation ownership against its ECS
revision reduced the dense-5000 diagnostic to about 66–71 ms simulation per host
frame and improved new-texture acquisition to 12–14/s. The original 8–9/s capture
remains the baseline. See the [measured fix and regressions](foundation-f8-device-observations.md)
and [raw qualification evidence](../benchmarks/foundation/results/2026-10-03-f8-cpu-isolation.json).
This is a verified incremental improvement; F8 remains open.

A subsequent stable-world ownership change ([ADR-0060](decisions/ADR-0060-stable-world-ownership-in-bridge-transactions.md))
raises the same short dense-scene capture to about 14–18 new textures/s. The
controlled all-behaviour Mac fixed-tick median improves from 16.49 to 13.94 ms.
[Evidence](../benchmarks/foundation/results/2026-10-03-f8-stable-world.json) retains
all samples and validation. This remains below the dense-scene target; physics,
lifecycle-query costs and GPU work remain open.

Lifecycle invalidation follow-up removes full script lifecycle scans from unchanged
and pose-only boundaries while preserving native hierarchy/activity notifications.
The controlled 300-script fixed-tick median falls from 13.43 to 10.61 ms; behaviour
work falls from 4.79 to 2.07 ms. The bounded phone capture ranges 17–29 acquisitions/s,
falling toward 17 at the end, with nominal thermal samples. This does not establish
steady or sustained performance. [Evidence and regressions](../benchmarks/foundation/results/2026-10-03-f8-lifecycle-invalidation.json)
retain the locked-device failure and successful retry. Native simulation and GPU
cost remain open; F8 is not qualified.

The physics-input reuse change has now also completed a bounded iPhone diagnostic
at `d39ae672`: about 20–29 new textures/s, overlapping the preceding 17–29 range.
The Mac CPU gain is measured; a clear phone speedup is not established. GPU frame
p50 remains about 17–18 ms, and the dense scene slows as it advances. Thermal
samples were nominal for eight seconds only. [Device evidence](../benchmarks/foundation/results/2026-10-03-f8-physics-inputs-device.json)
does not close sustained performance or F8.

A subsequent AO arithmetic optimization preserves the twelve-sample pattern and
reduces the measured AO stage about 2–3% in repeated bounded phone captures.
Whole-frame GPU timings overlap (approximately 17–18 ms), so no overall FPS gain
or gate closure is claimed. A 600-tick Mac timeline does not reproduce the phone's
late slowdown; phone-specific per-tick isolation remains open. See the
[comparison and regression evidence](../benchmarks/foundation/results/2026-10-03-f8-ao-kernel.json).

Fixed-scene GPU isolation subsequently supports retaining the tiled compute blur:
the final GPU window-p50 summary is 19.13 ms versus 23.87 ms in the following
baseline repeat, with identical scene state and 63 byte-identical comparison
images. The implementation adds about 7.2 MiB at phone resolution, removes the old
fragment blur and its obsolete binding, and passes 111 renderer/Apple tests plus
Clippy. [Evidence](../benchmarks/foundation/results/2026-10-03-f8-tiled-blur-retained.json)
keeps preliminary and final runs separate. The live dense scene still declines
into the teens; no live-game FPS improvement or sustained qualification is claimed.
F8 remains open.

Render membership now ignores pose-only changes. The focused Mac extraction
qualification improves from 1.37 ms to 0.13 ms for 10,000 moving spatial entities.
A subsequent bounded live iPhone capture shows lower early extraction cost
(1.84–2.03 ms) but still declines to 17–20 new textures/s late in the run.
Simulation catch-up and renewed extraction cost remain open; these historical
comparisons do not establish a sustained FPS improvement.
[Device evidence](../benchmarks/foundation/results/2026-10-03-f8-render-membership-device.json).

The subsequent structural-history fix prevents dense value writes from evicting
membership events. It removes the reproduced full-scene rebuild on sparse churn
and keeps late phone extraction near 2.0–2.2 ms rather than 3.1–3.3 ms.
ECS/core validation passed 404 tests. Simulation still reaches approximately
50 ms per host frame, so live throughput and sustained qualification remain open.
[Reproduction and bounded device evidence](../benchmarks/foundation/results/2026-10-03-f8-structural-history.json).

Physics synchronization now reuses a hierarchy traversal buffer rather than
allocating subtree lists for each changed root. A controlled Mac CPU pair shows
6.5% lower physics time and 4.1% lower native tick time; 405 physics/core tests
pass. The bounded live phone capture still settles near 20 acquisitions/s, so
this does not close the performance gate.
[CPU/device evidence](../benchmarks/foundation/results/2026-10-03-f8-physics-traversal.json).

The subsequent common-collider sync changes reduce repeated component comparisons
and reuse validated pose data. A focused 5,000-moving-collider benchmark improves
from 1.81 ms to 1.60–1.61 ms. After unlock, the updated live iPhone release completed
eight seconds at roughly 31–32 acquisitions/s early and 23–26/s late. This is a
historical comparison, not a controlled overall speedup or sustained qualification.
Exact PID 1632 was stopped; F8 remains open.
[Validation and capture](../benchmarks/foundation/results/2026-10-03-f8-sync-reuse.json).

The next dense-scene investigation removed unnecessary asset-owner rebuilds when
unrelated transform traffic overflows ECS history. Sparse readers can recover
retained structural events without ignoring lost relevant values. Bounded phone
captures reduced late mean bridge overhead outside simulation from 1.91–1.97 ms
per tick to 0.033–0.036 ms. The normal release delivered roughly 27–31 new textures/s
and was stopped after eight seconds. This fixes a core scaling issue but does not
qualify sustained 60/120 FPS; F8 remains open.
[Evidence and limitations](../benchmarks/foundation/results/2026-10-03-f8-domain-history.json).

Two further AO arithmetic experiments were rejected: neither improved whole-frame
GPU time in bounded frozen-scene device captures. The retained renderer still
measures approximately 19 ms/GPU frame in that isolated workload. F8 remains open;
this is neither live-game throughput nor sustained qualification.
[Rejected experiments](../benchmarks/foundation/results/2026-10-03-f8-ao-arithmetic.json).

The blur now clips its filter bounds once per output and uses predicated tap
accumulation. In alternating original/candidate eight-second frozen captures,
blur spans measured 4.08–4.17 ms versus 3.72–3.75 ms, and whole-frame window-p50
medians measured 19.06–19.18 ms versus 18.67–18.74 ms. All 63 comparison images
were byte-identical; 97 renderer tests and release Clippy passed, with scalar
coverage extended to real perspective/orthographic projections and tiny edges.
The final live run advanced simulation and stopped normally, but acquired only
about 28 fresh textures/s. This is a small retained GPU improvement, not a
60/120 Hz qualification. [Evidence](../benchmarks/foundation/results/2026-10-03-f8-blur-bounds.json).

Shared world-pose preparation now avoids ancestor allocation, identity inversion
and unused ancestor lookup tables for root-only batches. Focused 5,000-root
preparation medians fell from 817–844 µs to 358–373 µs on the Mac. The same API,
validation and mixed parent/child ordering remain. 433 scene/core/physics tests
and release Clippy passed; the no-trace device runner's empty-environment launch
bug was also fixed with regression coverage. This is a local CPU optimization;
phone captures of the first candidate remained about 28 fresh acquisitions/s.
[Results and verification status](../benchmarks/foundation/results/2026-10-03-f8-root-poses.json).
The final root-only implementation subsequently passed the live phone check:
fresh launch ID, advancing simulation, nominal thermal samples, eight-second
deadline and exact PID termination. Its 31.38 fresh acquisitions/s is a short-run
observation, not an established sustained or causal FPS improvement. F8 stays open.

Physics synchronization now gathers candidate components through one shared ECS
query view, preserving validation, candidate order and lifecycle semantics.
Alternating Mac dense-scene captures measured native steps at 7.53–7.56 ms before
and 7.02–7.05 ms after; profiled sync fell from 2.51–2.54 ms to about 1.96 ms.
410 physics/core tests and release Clippy passed. Live eight-second iPhone
captures completed with nominal thermal samples and exact PID termination;
fresh acquisitions changed from 31.48/s to 32.06/s. This is a retained CPU
improvement, not sustained 60/120 Hz qualification. F8 remains open.
[Evidence](../benchmarks/foundation/results/2026-10-03-f8-sync-view.json).

Shared hierarchy reads and removal of redundant root insertions further reduced
native dense-scene step medians from 7.12–7.16 ms to about 6.74 ms in alternating
Mac runs. Final update order and descendant invalidation remain unchanged.
411 physics/core tests and release Clippy passed, including an already-affected
middle-node regression. Both bounded phone captures completed and their exact
PIDs were stopped; fresh-frame delivery still varied around 26–30/s. F8 stays
open. [Evidence](../benchmarks/foundation/results/2026-10-03-f8-sync-hierarchy.json).

### Correlated frame diagnosis follow-up

See [the ranked performance diagnosis](foundation-f8-performance-diagnosis.md)
for nine controlled iPhone captures, raw traces and the next implementation
acceptance criteria. Both live CPU work and full-resolution GPU work exceed the
120 Hz budget; texture admission/acquisition compounds the loss. Instrumentation
and controlled isolation are complete for this batch. Sustained performance and
physical-display pacing qualification remain open; no production quality or
buffer-count default was changed.

### Early admission implementation

[Early render admission](foundation-f8-render-admission.md) now skips world
extraction and GPU preparation when the texture ring is full. Native lifetime,
error/panic, Async-progress and one-frame-input regressions pass. Two bounded
phone traces reached 38.5/39.2 fresh acquisitions/s; the untraced control reached
36.3 versus 29.2 before. Render-request age at acquisition increased, so throughput
improvement is not a pacing/latency qualification. GPU and scheduling work remain.

### Presentation wait policy

[Ready-image acquisition](foundation-f8-presentation-wait.md) now avoids waiting
for a newer GPU frame when a completed image is available. Native regressions,
Clippy, Flutter tests and the release build pass. Median acquisition calls fell
from about 3.5 ms to 0.1 ms; request-to-acquisition medians improved to 33–34 ms.
Fresh acquisition throughput varied between 39.2 and 43.0/s in traced runs; the
untraced control reached 42.1/s. Interval tails remain variable. This is a partial
latency improvement, not sustained performance or motion-quality qualification.

### Current AO/contact isolation and rejected blur candidate

[Effect isolation](foundation-f8-effect-isolation.md) measured 19.39 ms full-quality
GPU time versus 6.91 ms with both screen-space effects disabled in the same frozen
dense scene. The diagnostic profiles retain all unrelated quality settings.
A white-tile blur shortcut passed pixel comparisons but did not establish a
reliable live-scene delivery improvement; it was removed and the original app
restored. Expanded image/reference tests and diagnostic controls remain. No
production rendering-quality reduction or new performance pass is claimed.

### Rejected half-resolution reconstruction

[Reconstruction experiment](foundation-f8-reconstruction.md) increased frozen GPU
time from 19.37 to 25.12 ms and shortened contact shadows despite passing the old
image tests. The candidate was removed; original source and iPhone app are
restored. A permanent shadow-extent check fails the candidate and passes the
original. No performance gate or production quality change results from this batch.

### Contact ray projection follow-up — 2026-10-04

The [contact projection experiment](foundation-f8-effect-isolation.md#contact-ray-projection-follow-up--2026-10-04)
showed no useful performance gain (19.360 vs 19.417 ms GPU; slightly lower
acquisition throughput). Original shader and phone app are restored. Permanent
perspective/orthographic and MSAA 1/4 contact-shadow coverage remains: 54 rendering
checks pass, and all 67 candidate/control images match. F8 remains open.

### Bounded Metal execution diagnosis — 2026-10-04

The [distance-cache and Metal diagnosis](foundation-f8-metal-diagnosis.md) rejects
an extra 14.44 MiB distance buffer: only a small GPU reduction and worse texture
delivery were observed. Original rendering is restored. Exact-PID Metal recording
and interval analysis now provide per-encoder evidence: AO fragment work, main
scene fragment shading and the compute blur are substantial. The available
hardware counter does not diagnose memory/occupancy, so that attribution remains
open. No new rendering speedup or F8 qualification pass is claimed.

### Retained shared-lighting guards — 2026-10-04

[Lighting work guards](foundation-f8-light-guards.md) skip unused environment
fetches and sun visibility when the shading normal faces away. All 68 compared
images match; 55 rendering and 29 renderer unit checks pass, with Clippy clean.
Repeated frozen GPU timings improve from 19.33 to 18.22–18.47 ms; live GPU timings
improve from 17.14 to 16.54–16.57 ms. Live acquisition throughput does not improve
consistently. This is a retained GPU-cost reduction with no new buffers or quality
reduction, not an FPS, smoothness, thermal or F8 qualification pass.

### CPU follow-up — 2026-10-04

A [fresh 479-tick device diagnostic](foundation-f8-cpu-followup.md) measures a
6.839 ms native step and 4.207 ms physics-system median. The dense fixture includes
5,000 moving colliders, not stationary scenery. A visual-leaf relevance filter
showed no useful synchronization gain in the Mac phase diagnostic and was removed.
CPU Time Profiler attachment failed twice; no internal phone phase attribution is
claimed. Prior lighting improvements remain; F8 remains open.

### Phone physics phase attribution — 2026-10-04

The [bounded native phase trace](foundation-f8-physics-phases.md) now resolves the
CPU follow-up's missing internal attribution: 479 matched ticks show 1.873 ms
synchronization, 0.996 ms solver and 3.185 ms total adapter medians. The complete
physics-system median is 4.344 ms; its additional propagation/event work needs
further attribution. No solver or workload change is justified by this capture.
This diagnostic does not establish an FPS improvement or close F8.

### Synchronization hierarchy reduction — 2026-10-04

[Four live phone captures](foundation-f8-sync-hierarchy.md) isolate collection,
hierarchy, input validation and application costs. Skipping descendant-walk setup
for leaves reduces hierarchy median about 19% (roughly 55 µs per tick), with the
full candidate set and physics semantics preserved. Whole native-step timing does
not improve; no FPS or sustained qualification pass is claimed. Validation and
application remain the larger synchronization costs. F8 remains open.

### Physics entry lookup reuse — 2026-10-04

[Repeated live captures](foundation-f8-entry-lookup.md) support retaining reuse
of the existing entry lookup during physics updates. Application cost drops
16–18% (112–128 µs per tick), with no new cache or changed validation. Whole-step
medians are mixed and p95 does not improve; no FPS/latency qualification is
claimed. Validation/classification, core propagation/events and GPU work remain.

### Validation-path follow-up — 2026-10-04

A [Mac-only validation probe and comparison](foundation-f8-validation-followup.md)
found no consistent total benefit from moving unchanged-entry checks into update
application. The candidate and temporary instrumentation were removed; prior
retained optimizations remain. No phone run or new FPS claim. Next attribution
target is the core propagation/event residual outside the physics adapter.

### Core physics propagation attribution — 2026-10-04

[480 matched live iPhone ticks](foundation-f8-core-physics-phases.md) identify
1.135 ms median transform propagation before physics, versus 0.0011 ms core
event/rejection handling. The instrumented core stage is 4.130 ms median. No
runtime optimization or FPS improvement is claimed; transform propagation is
the next measured target. F8 remains open.

### Shared propagation reads — 2026-10-04

[Repeated live iPhone comparisons](foundation-f8-propagation.md) support retaining
shared query views during transform propagation, with precise per-entity writes.
Propagation medians fall 24–27% (0.27–0.31 ms/tick); native-step medians and p95
improve in both phase comparisons. Without extra tracing, acquisition throughput
is effectively unchanged at 43.43 vs 43.44/s. This is a CPU reduction, not an FPS
or sustained qualification pass. Sparse notifications and matrix correctness have
permanent regression coverage; 25 scene and 375 core checks pass. F8 remains open.

### Redundant dirty-write experiment — 2026-10-04

[The candidate was removed](foundation-f8-dirty-writes.md): propagation became
cheaper, but phone whole-step timings were mixed. Untraced acquisition fell
40.54→36.65/s in one pair and tied at 44.24/44.28 in reverse order. No consistent
end-to-end benefit or consistent regression is established. Prior retained
optimizations remain; expanded setter/raw-write propagation coverage stays.
F8 remains open; further work should return to rendering costs.

### Bilateral blur tile experiment — 2026-10-04

[16×16 and 16×8 candidates](foundation-f8-blur-tiles.md) preserve all 68 reference
images but increase measured blur spans and reduce frozen-scene acquisitions
relative to both 8×8 controls. Both candidates were removed. The original kernel
and prior improvements remain; three additional scalar-reference boundary sizes
are retained. No FPS improvement or F8 completion is claimed.

### AO neighbour reconstruction experiment — 2026-10-04

[The select-before-reconstruct candidate](foundation-f8-ao-neighbour.md) was
removed. Its initial AO-span advantage reversed on repetition, whole-frame GPU
cost was effectively unchanged, and acquisitions were lower in both pairs.
One of 68 reference images had small differences. Restoring the original shader
restores all images exactly; 29 unit and 55 shading checks pass. F8 remains open.

### Metal profiling startup — 2026-10-04

[A new execution baseline](foundation-f8-profiler-gate.md) confirms AO/contact
fragment 5.858 ms and blur compute 3.749 ms median active intervals (overlapping,
not additive). Isolated-effect captures remain incomplete because Instruments
rejects launched PIDs. The new bounded startup gate is covered by 19 Python checks
and a successful manual phone handshake, but a successful gated Metal recording
is still pending. No renderer optimization or FPS gain is claimed. F8 stays open.

### Isolated Metal execution recovered — 2026-10-04

[Three gated exact-PID recordings](foundation-f8-metal-effects.md) succeed after
USB reconnection. Common-window active estimate intervals are 3.531 ms AO-only,
1.934 ms contact-only and 5.988 ms combined; the shared blur remains about 4 ms.
The capture gate gains partial-transfer/cancellation regression coverage and the
analyzer excludes crossing executions from optional time windows. 20 Python
checks, Dart gate regressions, analysis and iOS build pass. No renderer FPS gain
or sustained qualification is claimed; F8 remains open.

### Fixed interior blur candidate — pending acceptance, 2026-10-04

[The candidate](foundation-f8-fixed-blur.md) passes 29 unit and 55 shading checks
with unchanged tolerances; 66/68 images match exactly and two have one-level
colour differences. Frozen GPU measurements improve, but the repeat control is
thermally warmer and live delivery is untested. Candidate remains unqualified in
the working tree pending cooldown confirmation and live comparison. F8 stays open.

### Fixed interior blur live decision — 2026-10-04

[The candidate was removed](foundation-f8-fixed-blur.md) after cooled live testing.
GPU medians improve about 5%, but acquisitions fall 41.025→36.989/s in the first
pair and rise 38.319→40.853/s in reverse order with a warmer control. This does not
establish a consistent end-to-end benefit or regression. Original shader and
release app restored; 29 unit and 55 shading checks pass, with all 68 reference
images exact after restoration. Extra boundary tests stay. F8 remains open.

### Live CPU/frame correlation — 2026-10-04

[A matched 509-frame/480-tick trace](foundation-f8-live-critical-path.md) records
4.92 s simulation and 1.93 s rendering calls over an 8.04 s host window. One-tick
host frames have 14.24 ms median work; two-tick frames 27.54 ms. Snapshot/bridge
bookkeeping and dropped-render work are small relative to simulation and render
preparation. Analyzer identity/catch-up checks pass (21 Python checks total).
No runtime optimization or FPS gain claimed. Next candidate: shared ECS reads
during extraction, with full-frame verification. F8 remains open.

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
