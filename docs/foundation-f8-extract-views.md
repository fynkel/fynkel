# F8 shared extraction reads — 2026-10-04

Retain the shared read-query implementation in `World::extract_into`. F8 remains
open. The change removes repeated component lookup/borrow work, without adding
another cache or state owner. Overall delivery improvement remains provisional.

## Implementation and correctness

Acquire the world-transform and optional rendering component columns once per
extraction, then resolve each maintained member through the query view. Membership
order remains authoritative for record indices, camera priority ties and the first
environment. Inactive/editor-hidden filtering, interpolation, skin palettes and
owned output snapshots retain their existing behavior. Query lifetimes end with
the extraction; structural changes are observed on the next call.

A permanent regression covers mixed rendering components, light positions,
layers, tied cameras, first environment selection, archetype changes between
calls, disabled volumes and the independence of an earlier output snapshot.
Existing extraction, animation and world tests also pass: 376 release core tests,
plus the separately run ignored extraction qualification. Clippy with warnings
denied, formatting, whitespace checks and the iOS release build pass.

## Measurements

Mac qualification: 10,000 spatial entities, 1,000 render records, 60 samples per
mode. Mutation and refresh work are outside the extraction timer. This is one
local pair, not whole-game performance.

| Mode | Previous median µs | Shared query median µs |
| --- | ---: | ---: |
| Static | 138.35 | 88.63 |
| Sparse | 146.63 | 88.85 |
| Dense | 132.79 | 90.52 |
| Dense churn | 132.02 | 93.27 |

Phone: live dense-5000, medium, 1320×2868, identical quality and workload; eight
seconds with frame tracing and all-tick CPU readback. Every run ends with exact-PID
termination. Instrumentation affects throughput; acquisitions are not physical
screen presentations.

| Run | Extraction p50 / p95 ms | New acquisitions/s | Thermal |
| --- | ---: | ---: | --- |
| Previous | 2.067 / 2.431 | 34.32 | nominal |
| Shared query | 1.584 / 1.842 | 39.72 | nominal |
| Shared query repeat | 1.589 / 1.963 | 38.60 | fair |

The nominal pair saves about 0.48 ms (23%) at extraction median. Preparation
medians are 2.848 and 2.805 ms, respectively. The repeated extraction result is
similar but its warmer thermal state excludes it from a controlled throughput
comparison. No reverse-order control was run after warming. Testing stopped;
the candidate app is installed and stopped.

## Decision and remaining work

The simpler reads, local benchmark and nominal phone pair justify retaining the
stage optimization. Do not claim a repeatable overall FPS gain from one pair.
A cooled reverse-order comparison remains outstanding. Sustained 60/120 FPS,
physical presentation smoothness and thermal qualification remain open.

Next, attribute renderer preparation costs before changing visibility, batching
or asset retention; the latest nominal pair still spends about 2.8 ms there.
Native simulation and GPU effects remain independent constraints. This change
does not reduce fixed-step work, lighting quality, resolution or stress density.

[Results](../benchmarks/foundation/results/2026-10-04-f8-extract-views.json)
and [raw evidence](../benchmarks/foundation/results/2026-10-04-f8-extract-views-raw.tar.gz).
