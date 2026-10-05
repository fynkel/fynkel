# F8 direct record lookup experiment — 2026-10-04

Candidate: replace the per-extraction-world `HashMap<u32, usize>` used during
instance construction with a dense `Vec<Option<usize>>`. Record IDs are already
dense array indices. The temporary table is reset for every main/extra world;
there is no persistent cache or additional authority. Missing entries remain
placeholders (when enabled), duplicate references retain last-wins behavior, and
out-of-range references cannot draw a record. Rendering order is unchanged.

A candidate regression (archived with the reverted experiment) compares the mapping with the previous map
semantics across sparse/unsorted/duplicate/out-of-range references and shrinking,
growing, empty and reused extra-world tables. 30 renderer unit tests and 55
shading tests pass; 68 image dumps match the control byte-for-byte. Clippy with
warnings denied and iOS release build pass. The first control image test command
failed because its dump directory had not been created; the unchanged control
binary passed after creating that directory. Both logs are retained.

## Initial measured pair

Live dense-5000, medium, 1320×2868, eight seconds, with identical frame and CPU
instrumentation. Both runs have nominal thermal samples and exact-PID termination.

| Build | Instance p50 ms | Preparation p50 ms | New acquisitions/s |
| --- | ---: | ---: | ---: |
| Control | 1.1935 | 2.6905 | 39.16 |
| Candidate | 1.0790 | 2.7350 | 32.87 |

Instance construction improves about 10%, but total preparation does not improve
and acquisition rate falls. Simulation totals also rise from 4905.614 to
5290.031 ms; native-step totals rise from 3323.375 to 3583.225 ms. The code change
does not modify simulation. This observation does not identify the source of the
run-wide variation or prove that the candidate caused it. Acquisitions are not
physical display FPS, and short nominal readings are not sustained qualification.

## Repeat and decision

The control repeat initially hit iOS device lock. After the user unlocked it,
control and candidate repeats completed:

| Build | Instance p50 ms | Preparation p50 ms | New acquisitions/s | Thermal |
| --- | ---: | ---: | ---: | --- |
| Control repeat | 1.200 | 2.725 | 39.44 | nominal |
| Candidate repeat | 1.072 | 2.634 | 39.46 | unknown, nominal |

The final run has an unknown thermal window, so it is not an all-nominal thermal
comparison. All acquisitions here are counted directly from the frame trace;
the unknown startup stats window is not silently omitted from a weighted rate.
All completed runs ended with exact-PID termination. The candidate consistently
reduces instance CPU time, but the first throughput pair is worse and the second
tied. These observations do not prove a repeatable regression or gain, nor its
cause. Native-step elapsed totals vary too; no CPU clock or scheduling attribution
is claimed without evidence.

Restore the original lookup; no production change from this experiment is kept.
The preceding extraction improvement and preparation diagnostics remain. Candidate
source, test and traces are archived for reproducibility. The original app is
reinstalled without launching another workload. Restored validation passes 29 unit
tests and 55 shading tests, with all 68 images matching the original baseline. The next measured target is
visibility (roughly 0.7–0.8 ms mean in these traces), with bounds construction,
hierarchy update and queries to be separated before changing their semantics.
The extraction reverse-order comparison and sustained 60/120 FPS remain open.

[Results](../benchmarks/foundation/results/2026-10-04-f8-dense-lookup.json)
and [raw evidence](../benchmarks/foundation/results/2026-10-04-f8-dense-lookup-raw.tar.gz).
