# F8 bounds work reduction and hierarchy classification — 2026-10-04

Retain the bounds construction change and hierarchy-update diagnostics. Rendering
correctness checks pass. A nominal phone candidate has lower bounds/preparation
cost than both controls; a cooled repeat is still needed for throughput acceptance.
F8 and sustained 60/120 FPS/thermal qualification remain open.

## Implementation

Before, visibility transformed fallback cube bounds for every record, then replaced
mesh records with their authored bounds. Now a temporary per-world boolean table
marks valid mesh record indices. Only records without meshes compute fallback
bounds; mesh records start unbounded and are populated by the existing authored
mesh logic. Duplicate/invalid indices, missing resources, deforming shaders and
skin bounds keep their behavior. The main world uses its matrix directly instead
of multiplying by identity; extra-world placement remains applied. No persistent
cache or additional state authority is added. The table is cleared for each world.

`SpatialIndex::update` now returns which existing branch it took: unchanged,
initial/membership rebuild, sparse refit, dense refit or cost-triggered rebuild.
The last includes its preceding refit. This does not change hierarchy policy.
A new optional trace record joins that classification to the existing update time.
The analyzer rejects missing/unknown classifications and nonzero reserved fields;
older traces remain readable.

## Correctness

31 renderer unit tests, 55 shading tests, five Apple tests and 24 Python checks
pass. Clippy with warnings denied and iOS release build pass. All 68 dumped images
match the control byte-for-byte, including animated bounds, extra worlds, shadows
and materials. New regressions cover mixed missing meshes, holes, duplicate and
invalid record indices, extra-world placement/reset, nonfinite transforms and
classification reset across actual index branches.

The first version of the new bounds test expected the raw cube radius for an
identity transform. The existing conservative transform deliberately rounds the
radius outward. The test now compares against that exact transformed bound;
production rounding was not weakened. Both test logs are preserved.

## Phone measurements

Live dense-5000, medium, 1320×2868, eight seconds with frame tracing and all-tick
CPU readback. Each completed run ended with exact-PID termination.

| Run | Bounds p50 ms | Prepare p50 ms | New acquisitions/s | Thermal samples |
| --- | ---: | ---: | ---: | --- |
| Control | 0.468 | 3.149 | 29.54 | unknown, nominal |
| Candidate | 0.344 | 2.7965 | 33.69 | nominal |
| Control repeat | 0.497 | 3.269 | 29.17 | nominal |
| Candidate repeat | 0.384 | 3.072 | 30.07 | fair |

The nominal candidate reduces bounds median by 26–31% relative to the two control
runs. Total preparation is also lower. Simulation timings vary across runs, so
these data cannot assign the whole acquisition change to bounds work. The first
control's unknown startup thermal window and final candidate's fair state prevent
claiming two controlled nominal pairs. The final repeat is excluded from throughput
acceptance and device testing stopped. Short acquisitions are not display FPS or
sustained qualification. The candidate app remains installed and stopped.

## What causes hierarchy-update spikes

The first control has 74 initial/membership rebuilds and 177 dense refits:
rebuild median 0.798 ms versus refit 0.167 ms. Rebuilds take 59.492 of 90.038 ms
of index-update time (66%). There are no cost-triggered rebuilds in that recording.
The nominal candidate similarly has 94 rebuilds (median 0.7365 ms) and 212 dense
refits (0.156 ms). The classification reflects executed branches, not guesses from
large timings. With this nonempty scene, rebuilds after initialization indicate
record-count changes. The data do not identify which individual entity caused each
change.

Next, investigate maintaining visibility membership across additions/removals
without rebuilding the entire hierarchy. Preserve record-index mapping, generation
safety, conservative bounds and query equivalence under churn. Do not delay required
updates or weaken the rebuild-quality guard based only on this one workload.
That is a separate implementation task; it has not been changed here.

[Results](../benchmarks/foundation/results/2026-10-04-f8-bounds-work.json)
and [raw evidence](../benchmarks/foundation/results/2026-10-04-f8-bounds-work-raw.tar.gz).
