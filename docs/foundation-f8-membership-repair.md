# F8 visibility membership repair — 2026-10-05

Retain the small-membership repair path. Correctness regressions and all reference
images pass, and two nominal iPhone candidate runs replace nearly all membership
rebuilds with cheaper repairs. This is a hierarchy-update tail reduction, not a
claim of higher whole-game FPS. F8 remains open.

## Implementation and invariants

The sphere hierarchy continues to index the current dense bounds array, not
persistent entity IDs. A record insertion/removal may shift all later indices.
For a small size change, removed tail-index leaves are detached, surviving leaves
are refreshed against every current bound, and new indices get leaves inserted
beside a sibling chosen by bounding-cost growth (height breaks ties). The leaf
mapping is the current array mapping; no stale entity reference survives a frame.

Removed node slots are recycled. Root identity is explicit and traversal no longer
assumes it is node zero. Since recycled slots are not topologically ordered,
dense refits use maintained postorder. A linear metadata pass refreshes subtree
ranges and leaf order for the existing fully-contained-frustum fast path. This
avoids the spatial partitioning work of a full rebuild; it does not skip refitting
current bounds or postpone necessary updates.

Small changes are those no larger than one eighth of the previous record count.
Cumulative size drift beyond one eighth of the last full build also rebuilds, so
the build-cost reference remains meaningful and sparse removal does not retain a
large outdated topology indefinitely. Empty/initial trees and large changes still
rebuild. The existing doubled-cost guard remains. During insertion, height beyond
`2 * floor(log2(record_count)) + 4` triggers a full rebuild; it cannot grow into
an arbitrarily deep chain. These are explicit policy bounds, not universal optimal
thresholds for every game. Further tuning requires measurements.

Diagnostic codes 5 (membership repair) and 6 (depth-guard rebuild) extend the
existing classification; old codes retain their meaning. Empty-to-empty now
reports unchanged. The index is also used for local-light candidates, so overlap
query correctness is tested alongside frustum culling.

## Validation

34 renderer unit tests, 55 shading tests, five Apple unit tests and 24 Python
checks pass. Clippy with warnings denied and iOS release build pass. All 68
reference image dumps are byte-for-byte identical; no tolerance was weakened.

Permanent regressions cover 180 frames of middle insertions/removals (shifting
dense indices), all-survivor movement, missing/nonfinite bounds, node-slot reuse,
parent/leaf mapping and complete leaf-order coverage. Frustum and overlap results
are checked against linear scans. Additional checks cover empty/singleton/large
replacements, bounded depth during ordered growth, and cumulative small-growth
rebuilding. The ignored qualification benchmark remains runnable separately.

Local 6,000-bound benchmark: every frame changes membership by one and moves every
bound. One baseline/candidate pair gives update p50 588.250→192.167 µs and p95
1014.541→199.291 µs (120 samples). This is update-only timing, not game performance.

## iPhone observations

Dense-5000, medium, 1320×2868, eight-second frame/CPU diagnostics. All three runs
report nominal thermal samples. Every run ends with exact-PID termination; the
candidate app is installed and stopped. There is no sustained thermal qualification.

| Run | Rebuilds | Repairs | All updates mean / p95 ms | Preparation p50 / p95 ms | New acquisitions/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| Control | 102 | 0 | 0.312 / 0.751 | 2.595 / 3.353 | 42.77 |
| Candidate | 1 | 97 | 0.178 / 0.229 | 2.552 / 3.124 | 42.22 |
| Candidate repeat | 1 | 87 | 0.164 / 0.216 | 2.405 / 2.943 | 43.62 |

Control rebuild median is 0.701 ms. Candidate repair medians are 0.202 and 0.187 ms.
Only initial construction rebuilds in these candidate captures; cost/depth/large
membership fallback behavior is exercised by local tests. Update p95 improves
about 70–71%. Ordinary dense refits measure 0.150 ms in control, 0.164/0.150 ms in
candidates; repair savings are not being presented as an across-the-board speedup.

Counts differ because these are independent live runs with different scheduling;
they are not same-frame paired samples. Preparation timing improves, but acquisition
rates bracket the control. No overall FPS gain or subjective smoothness pass is
established. Acquisition is not physical presentation, and nominal thermal state
does not guarantee constant clocks. The data support cheaper membership maintenance
and reduced hierarchy tails in this workload.

## Remaining work

Reassess the full CPU/GPU budget after these retained CPU changes; screen-space GPU
work and simulation remain substantial constraints. This batch does not solve the
60/120 FPS target. Sustained thermal behavior, physical presentation smoothness,
cooled reverse-order comparisons for earlier extraction/bounds changes, and the
unavailable second-Mac qualification remain open.

[Results](../benchmarks/foundation/results/2026-10-05-f8-membership-repair.json)
and [raw captures, checks and source](../benchmarks/foundation/results/2026-10-05-f8-membership-repair-raw.tar.gz).
