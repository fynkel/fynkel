# F8 core physics-system attribution — 2026-10-04

The previously unassigned core physics-system cost is transform propagation,
not event publication. A bounded live dense-5000 iPhone capture collected 480
samples, each matched to its SDK tick (completed native ID = pre-step SDK ID + 1).

| Work | Median per tick | p95 |
| --- | ---: | ---: |
| Propagate scene transforms before physics | 1.135 ms | 1.361 ms |
| Run physics adapter | 2.972 ms | 3.422 ms |
| Handle rejections/events and read statistics | 0.0011 ms | 0.0033 ms |
| Entire instrumented core physics stage | 4.130 ms | 4.736 ms |
| Entire native step | 6.835 ms | 7.803 ms |

Medians are computed independently, not summed. The core adapter duration
contains the existing physics phases; those must not be added a second time.
The SDK's whole-microsecond system timing truncates fractions: its difference
from the nanosecond core timing can be slightly negative (median -0.291 µs).
This is rounding, not negative scheduler work. No substantial additional
scheduler cost is established by this capture.

The app stopped at its eight-second deadline and exact PID 5041 was terminated
before copying evidence. All thermal samples were nominal. Traces add overhead;
this is attribution, not an FPS comparison, smoothness pass or sustained thermal
qualification. F8 remains open.

## Implementation and validation

`World::set_physics_profiling` enables the core and adapter timers together.
Core samples reset before every step; skipped stages expose no sample. Disabling
profiling clears both profiles. The default path calls no additional clocks.
No gameplay ABI, scene content, transform algorithm or physics setting changed.

The bounded native trace and capture parser now include `propagation_ns`,
`core_adapter_ns`, `publication_ns` and `core_total_ns`. Regression coverage checks
nested timing containment, disabled/skipped/re-enabled stages and explicit zero
rows. Existing archived diagnostics retain their matching older parser/source.

Checks passed: 16 core physics tests, 95 native tests (two native tests remain
ignored), 18 Python diagnostic tests, stripped native check, Clippy with warnings
denied, and iOS release build. The Mac cooked-scene diagnostic confirms identical
transforms and motion state with profiling enabled and disabled. Its propagation
median is 1.212 ms; this Mac figure is not a phone measurement.

## Next work

Optimize transform propagation while preserving incremental change tracking,
hierarchy ordering, invalid handles and dirty-subtree semantics. The current
path repeatedly resolves entities and acquires individual component borrows
while finding dirty nodes and updating their world matrices. Measure changes to
that path against the reference propagation behavior and sparse as well as dense
scenes. Do not replace tracked per-entity writes with a bulk mutation that forces
all downstream consumers to rescan the world.

[Results](../benchmarks/foundation/results/2026-10-04-f8-core-physics-phases.json)
and [raw evidence, source and checks](../benchmarks/foundation/results/2026-10-04-f8-core-physics-phases-raw.tar.gz).

Follow-up: [shared reads during propagation](foundation-f8-propagation.md) reduce
this stage by 24–27% in repeated iPhone comparisons while preserving precise
per-entity notifications. Frame delivery remains effectively unchanged.
