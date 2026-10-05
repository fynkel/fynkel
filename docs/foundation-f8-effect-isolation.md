# F8 screen-space effect isolation — 2026-10-03

The current dense scene confirms a large GPU cost from the combined AO/contact
pipeline. A conservative blur shortcut was implemented and tested, but rejected
because its frozen GPU timing improvement did not yield reliable delivered-frame
improvement. The original production shader is restored. This batch retains the
repeatable diagnostic controls and stronger regression coverage.

## Controlled effect measurements

One release binary, one settled dense-5000 scene (120 simulation ticks), one
1320×2868 viewport, 4× MSAA, four buffers, and the immediate-Ready presentation
policy. Each profile copies medium and changes only the AO/contact enable flags.
All four frozen runs drew 1,767 visible instances. The other populations, asset
settings, shadows, bloom and resolution were unchanged.

| Profile | Whole-frame GPU time | New texture acquisitions/s |
| --- | ---: | ---: |
| Medium: both effects | 19.39 ms | 49.85 |
| AO disabled, contact shadows retained | 15.69 ms | 59.25 |
| Contact shadows disabled, AO retained | 17.04 ms | 55.59 |
| Both disabled | 6.91 ms | 111.32 |

GPU times are medians of seven per-second p50 windows after excluding startup,
not pooled frame medians. Acquisition rates use all statistics windows. The
combined pipeline adds about 12.48 ms of measured whole-frame GPU time in this
comparison. Removing AO alone saves about 3.69 ms; removing contact shadows alone
saves about 2.35 ms. These savings are **not additive**: the combined-off case also
removes shared depth, resolve and blur work, and GPU scheduling/overlap changes.
The leftover difference cannot be assigned exclusively to blur from this test.
Frozen throughput is GPU isolation, not moving-game qualification.

## Tested and rejected: fully unoccluded blur tiles

The candidate checked all 121 input/halo AO/contact values of each 8×8 output
tile. If both channels were exactly one everywhere, every bilateral result was
necessarily one, so it skipped radial-depth reconstruction and the filter. Other
tiles retained the original filter. It added a workgroup atomic flag and barrier;
it did not reduce resolution, samples, footprint or thresholds.

Correctness checks passed: the 53 shading tests passed for both implementations,
and all 63 exported images were byte-for-byte identical. The scalar-reference GPU
regression was expanded with all-white tiles and isolated AO/contact taps across
tile boundaries and screen edges, under perspective, orthographic and identity
projection. Those cases also pass with the restored original shader and remain
as permanent coverage.

Measured results did not justify retaining the implementation:

| Run | Whole-frame GPU time | Acquisitions/s |
| --- | ---: | ---: |
| Frozen original, first control | 19.39 ms | 49.85 |
| Frozen candidate | 18.15 ms | 48.47 |
| Frozen original, restored control | 19.40 ms | 50.12 |
| Frozen candidate, repeat | 18.10 ms | 49.49 |
| Live candidate, first | 16.91 ms | 36.46 |
| Live original, interleaved control | 17.16 ms | 40.44 |
| Live candidate, repeat | 17.45 ms | 40.97 |

The candidate reduced the frozen blur timestamp median from about 3.00 to
2.49 ms and the frozen whole-frame span by about 6.5%. It did not improve frozen
acquisition throughput, and live results were inconsistent. There is no reliable
live-scene win established here. Retaining a more complicated shader on this
evidence would not serve the performance goal. Its source is archived only as a
rejected experiment, outside the production shader tree.

## Retained tooling, verification and next work

`prepare.py` adds three diagnostic profiles to generated mobile fixtures. The
runner's `--quality` option selects one through the normal quality-profile API;
manual/default captures still use medium. Captures fail if the requested profile
is not acknowledged or cannot be verified against the attached asset store.
The runner also fingerprints bundled framework binaries, including the native
renderer, rather than relying only on the Dart host executable hash.

Checks passed: 14 Python tests, Dart analysis, project validation/cooking, iOS
release builds, 53 baseline and 53 candidate shading tests, 29 restored renderer
unit tests (1 existing ignored), and renderer Clippy with warnings denied. The
original-shader app was reinstalled after the final candidate run and left
stopped. Every new capture completed its eight-second deadline, reported nominal
thermal samples, and had its exact launched PID terminated.

The next GPU implementation should address the full-resolution estimate and
shared filtering pipeline at an algorithm level, with explicit resource budgets
and depth-aware reconstruction that preserves silhouettes and contact edges.
A lower-resolution experiment must pass the existing corner/edge tests without
weakening them; simply reducing resolution or sample count is not an accepted
fix. Success requires full-resolution output, useful whole-frame/delivery gains
in the **live** dense scene, and acceptable frame-age tails. Keep frozen GPU
controls for attribution, but do not use them alone to accept an optimization.

[Results and image fingerprints](../benchmarks/foundation/results/2026-10-03-f8-effect-isolation.json)
and [raw captures plus rejected candidate](../benchmarks/foundation/results/2026-10-03-f8-effect-isolation-raw.tar.gz)
make the comparison reproducible. Short nominal thermal samples do not establish
sustained heat behavior. Texture acquisition is not physical display presentation.
F8 and both sustained performance targets remain open.

## Contact-ray projection follow-up — 2026-10-04

A full-resolution candidate projected the contact ray's origin and direction once
per pixel, then formed each homogeneous ray step by linear interpolation before
the perspective divide. It retained all twelve steps, ray distances, depth
samples, bias, thickness, dither and filtering. No separate rendering pass or
quality profile was introduced.

The frozen dense-scene comparison did not establish a useful benefit:

| Renderer | Whole-frame GPU | Estimate span | Acquisitions/s |
| --- | ---: | ---: | ---: |
| Candidate | 19.360 ms | 7.321 ms | 50.116 |
| Original, same-session control | 19.417 ms | 7.385 ms | 50.370 |

Whole-frame values are medians of window p50s after excluding the first valid
window; estimate spans are medians of trace samples. Acquisitions are weighted
by statistics-window duration. Both captures used the settled dense-5000 medium
scene, full resolution, MSAA 4 and the same tracing settings. Both terminated
their exact launched PID and reported nominal thermal samples. The first attempt
went into the background before producing frames and was correctly rejected by
the runner; it is archived but excluded from this comparison.

The 0.3% whole-frame difference, with slightly lower delivery, is insufficient
evidence of a performance improvement. The candidate was removed; original
source, generated app and the installed iPhone app are restored, with the phone
app stopped. No live performance claim or sustained qualification follows.

A permanent rendering regression now exercises contact shadows with perspective
and orthographic cameras, each with direct single-sample depth and resolved
MSAA depth. It checks that the cube retains ground contact and open floor stays
unshadowed. All 54 shading checks pass (2 existing ignored); all 67 exported
candidate/control images match byte for byte. This adds coverage for future
ray-march changes without retaining an unproven optimization.

These measurements rule out this particular repeated-projection rewrite as a
useful fix. They do not establish a hardware bottleneck such as texture latency
versus occupancy. The next algorithm investigation should reduce depth-sampling
work with a conservative visibility bound, or evaluate AO separately while
preserving full-resolution contact. Extra-pass and hierarchy costs must be
measured before adopting either design. F8 remains open.

[Results](../benchmarks/foundation/results/2026-10-04-f8-contact-projection.json)
and [raw captures, images, source and test log](../benchmarks/foundation/results/2026-10-04-f8-contact-projection-raw.tar.gz).
