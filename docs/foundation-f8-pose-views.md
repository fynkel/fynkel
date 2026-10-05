# F8 shared world-pose preparation reads — 2026-10-05

Implementation and local correctness checks are complete. A subsequent all-nominal
candidate/control/candidate sequence supports retaining the targeted CPU saving,
without establishing a repeatable full-frame gain. The earlier warmer candidate
remains excluded from acceptance. F8 remains open.

## Change

`prepare_world_poses` now acquires one immutable ECS query view for transforms,
parents and propagated-transform presence. Optional columns preserve the distinction
between stale/foreign handles and live entities missing components. The validated
current transform and requested pose travel with each sorted proposal, avoiding
repeat ECS and requested-map lookups. Reads have batch lifetime only; no persistent
cache or secondary state owner is added.

Duplicate requests still use the last pose. Validation still precedes all writes;
parents precede children and prepared ancestors override current ancestor poses.
Scale preservation, singular-parent rejection, cycle detection, finite/quaternion
validation and deterministic error/order behavior remain. Empty batches return
before acquiring component columns. Native systems still publish through the same
atomic gameplay edit API; this does not bypass validation or mutation tracking.

## Validation

27 scene tests and 376 core tests pass, as do Clippy with warnings denied and the
iOS release build. Permanent regressions cover missing local/world transforms,
stale and recycled handles, foreign-world handles, null handles, failed batches
leaving valid targets clean, ancestors requiring local but not propagated poses,
and empty batches not borrowing unrelated component storage. Existing tests cover
updated ancestors, request ordering, duplicate targets, scale and atomic rejection.

The 5,000-root-pose qualification (300 samples after 40 warmups) improves from
median 343.334 µs / p95 389.625 µs to 106.541 / 112.584 µs. This is one local
microbenchmark pair, not a phone or full-game speedup.

## Phone observations and limitation

The first install failed with a CoreDevice connection reset. After the user
reconnected USB, control and candidate eight-second dense-5000 captures completed
with frame tracing and all-tick CPU readback, medium quality and 1320×2868.

| Build | Native tick median ms | LookAtCamera median ms | New acquisitions/s | Thermal |
| --- | ---: | ---: | ---: | --- |
| Control | 7.1915 | 0.735 | 38.21 | unknown, nominal |
| Candidate | 7.088 | 0.638 | 38.80 | fair |

Both runs ended with exact-PID termination. The candidate app is stopped. The
warmer candidate is not a valid controlled throughput acceptance pair; no mobile
speedup is claimed from these rows. The user has been asked to cool the device
before a repeat. Acquisitions are not physical display presentations, and these
short runs do not qualify sustained thermal behavior or 60/120 FPS.

## Normal-thermal repeat sequence

The connected phone completed three more eight-second captures, in reverse order
followed by one candidate repeat. All eight thermal samples in every run were
nominal; exact PIDs 10678, 10681 and 10682 were terminated before copying evidence.
"Cooled" directory names identify the requested repeat; normal thermal state was
verified by the app, not by an additional user report of physical temperature.
Medium quality, 1320×2868, workload and diagnostic options were unchanged.

| Build / order | Native tick median ms | LookAtCamera median ms | New acquisitions/s | Thermal |
| --- | ---: | ---: | ---: | --- |
| Candidate first | 7.385 | 0.6745 | 37.52 | nominal |
| Control second | 7.202 | 0.736 | 39.06 | nominal |
| Candidate repeat | 7.020 | 0.628 | 39.95 | nominal |

Camera-facing work is 8.4% and 14.7% lower than the intervening control. Other
stages vary between runs, and full-frame acquisition rates are lower in the first
candidate and higher in the repeat. These short diagnostics establish a targeted
saving, not a repeatable overall throughput improvement or sustained qualification.
No visual quality, simulation rate or transaction guarantees were reduced.

## Current decision

Retain the shared-read implementation on the strength of correctness coverage,
the local microbenchmark and lower camera-facing stage cost in both normal-thermal
candidate runs. Do not claim a full-game FPS gain. Current physics medians remain
4.177–4.374 ms per tick, and prior GPU traces remain well above the 8.33 ms budget
for 120 FPS. Next focus is the measured physics synchronization and GPU screen-space
costs; this conversion optimization does not resolve either bottleneck.

[Results](../benchmarks/foundation/results/2026-10-05-f8-pose-views.json),
[original raw evidence](../benchmarks/foundation/results/2026-10-05-f8-pose-views-raw.tar.gz)
and [normal-thermal repeat evidence](../benchmarks/foundation/results/2026-10-05-f8-pose-views-cooled-raw.tar.gz).
