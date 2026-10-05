# F8 AO neighbour reconstruction experiment — 2026-10-04

The candidate was removed. The original AO shader is restored byte-for-byte;
prior retained engine improvements remain. No FPS improvement is claimed.

## Hypothesis and correctness

The depth-normal calculation initially reconstructs the right/down neighbours,
then may overwrite them with left/up reconstructions. The candidate selected
neighbour depth and UV first, then reconstructed exactly one point per axis.
It preserved the depth comparison, ties, clamped texture coordinates, original
unclamped reconstruction UVs and subtraction direction. No samples, resolution,
quality settings or texture formats were changed.

29 renderer unit checks and 55 shading checks passed (one and two manual checks
ignored). 67 of 68 exported images were byte-identical. `volume-wide.ppm` differed
in 41 RGB bytes out of 49,152, with maximum absolute difference 3/255 and summed
difference 50. The test's wider-radius occlusion assertions passed, but output
identity was not established. Restoring the original shader restored all 68
images exactly. No particular compiler transformation is proven as the cause.

## Bounded iPhone measurements

Frozen dense-5000, medium, 1320×2868, GPU tracing enabled. Run order was control,
candidate, candidate repeat, control repeat:

| Build | AO estimate span median | Whole-frame GPU | Acquisitions/s |
| --- | ---: | ---: | ---: |
| Control | 7.311 ms | 18.898 ms | 48.369 |
| Candidate | 6.542 ms | 18.752 ms | 47.116 |
| Candidate repeat | 7.449 ms | 18.867 ms | 47.966 |
| Control repeat | 6.607 ms | 18.873 ms | 49.363 |

Whole-frame GPU values are medians of window p50s excluding the first valid
window; all window values are preserved in JSON. Estimate spans include all
48/48/47/49 samples. Spans can overlap and include scheduling effects; they are
not execution-only Metal intervals and must not be added together. Acquisitions
are weighted by reported window duration and are not display scanout.

The initial estimate-span advantage reverses on repetition. Whole-frame gains
are negligible and acquisitions are lower in both candidate/control pairs.
This does not justify retaining the candidate, particularly with changed image
output. No live test was warranted. The cause of timing variation is not isolated.

All captures reached their eight-second deadline, recorded nominal thermal
samples and had their exact launched PIDs terminated. These are short diagnostics,
not sustained thermal or 60/120 FPS qualification. The last installed/stopped app
is the retained control. The generated release app was rebuilt from restored
source; 29 unit and 55 shading checks pass after restoration.

This experiment also reinforces a measurement limit: small source-level savings
and isolated timestamp spans do not establish lower frame cost. Further AO work
should use execution-only Metal intervals to verify the dominant cost before
pursuing another arithmetic rewrite. F8 remains open.

[Results and image hashes](../benchmarks/foundation/results/2026-10-04-f8-ao-neighbour.json)
and [raw captures, shader candidate, images and checks](../benchmarks/foundation/results/2026-10-04-f8-ao-neighbour-raw.tar.gz).
