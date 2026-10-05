# F8 early render admission — 2026-10-03

The Apple viewport now reserves a texture before extracting a world or preparing
its renderer. A full ring returns immediately, retaining the last rendered image.
An admitted attempt extracts the current world; skipped attempts do not create a
backlog of snapshots. This implements the first bounded change from the
[performance diagnosis](foundation-f8-performance-diagnosis.md), not the whole
presentation-scheduling or GPU performance plan.

## Implementation and safety

`IoSurfaceRing::try_render_with` is the shared admission/submission implementation;
`render_with` delegates to it for infallible callers. The viewport's normal,
pre-extracted and resource-ready render paths use that same admission boundary.
A pre-extracted caller still pays for its own extraction before calling the API;
the native world entry point avoids both extraction and preparation on rejection.

The slot lifecycle is now:

`Free → Reserved → Rendering → Ready → Presented → Retiring → Free`

Reserved means CPU preparation, not a submitted image. A guard frees that slot
on a preparation error or unwind. After submission begins, it conservatively
retains ownership if submission panics; the existing viewport panic boundary
marks the viewport broken. A reservation borrows the ring, preventing safe Rust
resize/destruction during recording. Existing generation checks still protect
against old completion callbacks after resize. Presented/Retiring ownership,
buffer count, acquire-wait defaults, quality and resolution are unchanged.

Preparation runs outside the ring mutex. The presenter can acquire other ready
images during preparation; it neither presents nor waits for a Reserved slot.
The last-submitted sequence is tracked separately from reservation identifiers,
so cancelled reservations do not become the newest submitted image. Async mode
polls completion before admission, including full-ring attempts, preventing a
stall when skipped preparation no longer polls the renderer's GPU timer.

One-attempt debug lines, overlays and extra-world inputs are discarded on a
full-ring rejection, preserving their previous lifetime and preventing stale
editor content or accumulating queues. Asset selection, uploads and extraction
resume on the next admitted attempt from the latest source state.

Stage timing samples now describe admitted frames only. The ring CPU callback
metric includes admitted preparation as well as encoding/submission; it must not
be compared directly with the old narrower ring CPU metric. Host trace render
intervals remain comparable. The existing trace occupancy `rendering` column
groups Reserved and Rendering; `submitted` events distinguish actual GPU work.

## Verification

- 19 Apple-platform tests passed; 2 existing benchmark tests remain ignored.
- Added regression coverage for full-ring extraction/preparation skipping and
  current-state refresh, discarded one-attempt inputs, preparation errors,
  panics, presenter access during preparation, cancellation-aware presentation lag, protected slots, resize reuse,
  and Async completion without renderer preparation.
- Updated the outline image test to actually acquire a texture before its third
  render. It previously filled a two-slot ring and asserted preparation stats
  while ignoring the failed render. It now requires a successful submission;
  the existing pixel comparisons remain intact.
- Release Clippy, 8 Flutter game-view tests, 13 Python runner/analyzer tests and
  the iOS release build passed. The final extra-world cleanup assertion also
  passed in the repeated native checks. A final diagnostic-counter correction
  counts submitted frames across cancelled reservation identifiers; the native
  suite and iOS build were rerun afterward. The phone timing captures precede
  that counter-only correction and contain no cancellations.

## Physical-device result

Same iPhone 16 Pro Max, dense-5000 scene, 1320×2868, medium effects, 4× MSAA,
four buffers and 3,000 µs acquisition wait. Each run lasted eight seconds, all
reported thermal samples were nominal, and each exact launched PID was terminated.

| Measurement | Before (two traced runs) | After (two traced runs) |
| --- | ---: | ---: |
| Fresh acquisitions/s, host trace window | 29.4 / 29.7 | 38.5 / 39.2 |
| CPU time in rejected render attempts | 463 / 368 ms | 10.6 / 11.3 ms |
| Total host render CPU time | 2,112 / 2,027 ms | 1,713 / 1,770 ms |
| Median submit → completion | 17.7 / 17.4 ms | 18.2 / 18.0 ms |
| Median render request → acquisition | 27.4 / 28.8 ms | 36.0 / 35.1 ms |

The untraced statistics-window control improved from 29.24 to 36.30 acquisitions/s
(about 24%). Equivalent traced statistics-window rates were 29.29/29.64 before
and 38.30/39.10 after. Instrumentation and run variation therefore matter; do not
present the traced gain as an exact sustained performance improvement.

Rejected attempts increased because the cheaper host loop could attempt more
frames. Their extraction/preparation stage count is now zero, and their total
CPU cost fell sharply. Raw dropped-attempt count alone is consequently a poor
success metric for this change.

The latency increase is real in these traces: a newly requested image reached
acquisition roughly 7–9 ms later at the median. Throughput improved without a GPU
execution-time reduction, but this does not establish improved input latency or
uniform physical display pacing. Keep the admission fix as the removal of measured
waste; the next presentation change must address bounded frame age and pacing
without adding texture queues or weakening lifetime safety. GPU screen-space and
live simulation costs still require the separate work in the diagnosis.

[Results](../benchmarks/foundation/results/2026-10-03-f8-render-admission.json)
include before/after summaries, current source/native library fingerprints and
render-request latency derived by matching each reserved frame to its enclosing
host render call and eventual acquisition. [Raw after captures](../benchmarks/foundation/results/2026-10-03-f8-render-admission-raw.tar.gz)
retain the inputs; baseline raw data is linked from the diagnosis. Acquisitions
are not physical screen scanout measurements. No sustained 60/120 Hz or thermal
qualification is claimed. F8 remains open.
