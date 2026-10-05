# F8 isolated GPU execution and capture hardening — 2026-10-04

Three gated, exact-PID Metal recordings now succeed after the user reconnected
the iPhone by USB. This completes the previously blocked isolated-effect capture.
No production shader or quality-default change is retained, and no FPS gain or
F8 completion is claimed.

## Connection diagnosis

Before reconnection, CoreDevice confirmed the exact launched PID and executable,
and both relevant release app signatures contained `get-task-allow=true`.
Instruments rejected both numeric-PID attachment and a diagnostic unique-name
attachment. Direct Instruments launch then reported that the device was not
connected. After reconnection, CoreDevice reported a wired connected tunnel,
Instruments listed the phone online, and all three numeric-PID attachments worked.
This supports a device/profiler connection problem; it does not establish an
Apple-internal root cause. No process-name fallback, broad process recording or
arbitrary startup delay was added to the production runner.

## Comparable measurements

One release binary and identical native framework hashes; frozen dense-5000,
1320×2868, medium-derived profiles. Only the existing diagnostic AO/contact flags
change. The default medium profile is unchanged.

The trace includes startup/loading. Encoder command buffers and Instruments frame
numbers are not shared across this renderer's passes, so they cannot reliably
identify complete scene frames. Instead, report both the full trace and a common
trace-relative middle window, 3.5–7.0 seconds. This window was selected from the
first AO-only timeline before inspecting the combined result. The analyzer keeps
only encoder/channel executions wholly within the window, excluding all pieces
of crossing executions instead of clipping them into falsely shorter samples.
It is a bounded diagnostic window, not proof of a thermally steady workload.

Median active GPU intervals in the common window:

| Profile | Estimate / Fragment | Bilateral blur / Compute | Main scene / Fragment |
| --- | ---: | ---: | ---: |
| AO only | 3.531 ms (75) | 3.968 ms (56) | 4.297 ms (54) |
| Contact only | 1.934 ms (112) | 3.991 ms (107) | 4.356 ms (99) |
| Both, normal medium | 5.988 ms (83) | 3.793 ms (83) | 5.299 ms (83) |

Parentheses show matched encoder/channel execution counts. GPU channels overlap;
these values must not be added into frame time. Different profiles also change
scheduling and other measured pass durations, so subtracting these rows does not
isolate a pure instruction cost. Profiler overhead means these captures are not
unprofiled frame-rate measurements. Full-trace values, raw tables, profile
acknowledgements, binary hashes and exclusions are retained in JSON and archives.
Full `.trace` bundles remain under the three `target/f8-metal-usb-*` directories.

All three app workloads reached their eight-second deadline and had their exact
PIDs terminated. Thermal samples were nominal for these short runs only. The
last run selected medium with both effects; the app is stopped.

## Retained tooling improvements

- Extracted the app start gate into a small independently testable Dart helper.
  It validates capture IDs, consumes matching signals once, and preserves timeout
  and lifecycle cancellation. It now tolerates an incomplete JSON file while the
  device copy is still writing it; incomplete or malformed data never releases
  the scene, and waiting remains bounded by the original deadline.
- Added a standalone Dart regression runner covering valid one-shot release,
  invalid IDs, stale filenames, mismatched payloads, non-object JSON, partial
  transfer completion, permanently malformed transfer timeout and cancellation.
- The fixture generator copies the helper alongside the diagnostic main file.
- Failed recorder startup now preserves its exit code after bounded cleanup.
- Added optional execution-window filtering to the Metal analyzer, with regression
  coverage for crossing executions, inclusive boundaries, invalid bounds and
  empty matches. Whole-trace analysis remains the default.

20 Python checks pass, the Dart gate regressions pass, Flutter analysis reports
no issues, and the iOS release build succeeds. The three real recordings also
validate the positive app/runner handshake end to end.

## Decision and next work

The shared full-resolution filter remains roughly a 4 ms active compute cost
with either effect alone. AO-only estimation is also substantially more expensive
than contact-only estimation in this fixture. This reinforces that another small
contact-ray arithmetic rewrite will not address the full screen-space cost.

Prioritize an algorithm-level AO/filtering candidate with explicit image-quality
and GPU-memory budgets, preserving full-resolution contact detail. Keep the
existing silhouette/corner/contact regressions and compare both frozen attribution
and live, unprofiled delivery before accepting it. Disabling effects remains a
diagnostic, not a shipped performance fix. Memory stalls, occupancy and shader
instruction-level causes are still unmeasured; do not claim them from these traces.

[Results](../benchmarks/foundation/results/2026-10-04-f8-metal-effects.json)
and [raw evidence](../benchmarks/foundation/results/2026-10-04-f8-metal-effects-raw.tar.gz).
