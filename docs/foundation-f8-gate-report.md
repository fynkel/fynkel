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
