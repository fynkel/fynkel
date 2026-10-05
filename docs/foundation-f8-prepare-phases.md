# F8 renderer preparation attribution — 2026-10-04

This batch adds measured CPU phase attribution, not a rendering optimization or
an FPS improvement claim. F8 remains open. The preceding shared extraction query
remains in place; its cooled reverse-order throughput comparison is still pending.

## Implementation and validation

`FrameStats.prepare_phases_us` contains nine nonoverlapping elapsed intervals.
Cumulative monotonic microsecond boundaries are differenced, so their sum equals
`FrameStats.prepare_us`, including rounding. The existing outer Apple preparation
timer also captures return/cleanup and caller costs; its difference from the phase
sum is reported separately. Measurements include waits and upload calls and do
not measure GPU execution. Eight extra clock reads run with existing preparation
timing; file output remains opt-in and bounded. No native/Dart ABI fields change.

Three CSV records carry the nine timings. The analyzer correlates complete phase
groups with their enclosing host render call and requires the existing total-stage
record. It rejects missing/duplicate groups, mismatched intervals, negative times
and phase sums exceeding the outer timer. Old traces with no phase events remain
readable. Readiness retries can contribute to the outer remainder; this capture
uses ordinary preparation.

Validation: 29 renderer unit tests, five Apple unit tests and 22 Python checks
pass. Permanent checks cover phase-total accounting at the real viewport boundary,
missing/duplicate/misassigned/invalid phase groups and outer remainder accounting.
Clippy with warnings denied and iOS release build pass. An older extraction trace
also parses with null preparation correlation.

## Phone observation

First launch was blocked by iOS device lock; its failure is preserved separately.
After the user unlocked the phone, one eight-second live dense-5000 capture
completed at medium quality and 1320×2868, with frame tracing and all-tick CPU
readback. All thermal samples were nominal. Exact PID 6843 was terminated at the
end. 347 preparation calls have complete phase groups.

| Preparation phase | Median ms | p95 ms |
| --- | ---: | ---: |
| Asset retention, setup, warm-up | 0.181 | 0.230 |
| Camera, environment, light collection | 0.002 | 0.004 |
| Visibility bounds and spatial queries | 0.632 | 1.262 |
| Light pool | 0.002 | 0.003 |
| Instance construction | 1.209 | 1.494 |
| Batch layout, uploads, outline | 0.337 | 0.445 |
| Shadow/post/uniforms | 0.016 | 0.022 |
| Overlay | 0.000 | 0.001 |
| Readiness/accounting | 0.198 | 0.238 |
| Outer remainder | 0.001 | 0.002 |

Whole preparation median is 2.655 ms, p95 3.426 ms. Phase medians are not additive.
Using totals, instance construction and visibility consume 677.483 of 946.944 ms
(71.5%) of preparation across this capture. Setup includes a 12.117 ms maximum,
so startup work is not erased from these totals.

Extraction median is 1.517 ms. New-frame acquisitions average 42.21/s, but this is
an instrumented single run, not a controlled before/after comparison and not
physical screen FPS. Nominal short-run thermal samples do not establish sustained
thermal behavior or constant clocks. No additional device run was needed.

## Next target and limits of attribution

Prioritize instance construction, then visibility. Inspection finds a per-frame
hash map from dense record indices to mesh indices, repeated resource lookups and
material clones in instance construction. Visibility builds fallback bounds for
all records, overwrites authored mesh bounds, then updates and queries its sphere
hierarchy. These are concrete candidates for narrower measurement and experiments;
this capture does not isolate their individual contribution.

A direct record-index lookup is a reasonable first experiment because records
already have a dense index. Preserve missing meshes, placeholder mode, extra-world
index resets, order and asset readiness. Measure its phase and full-frame impact
before retaining it. Do not introduce a second authoritative transform cache or
change visibility conservatism, upload budgets, quality, fixed-step work or scene
density to improve a timing number.

[Results](../benchmarks/foundation/results/2026-10-04-f8-prepare-phases.json)
and [raw captures and checks](../benchmarks/foundation/results/2026-10-04-f8-prepare-phases-raw.tar.gz).
