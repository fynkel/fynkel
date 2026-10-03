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
