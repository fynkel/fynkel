# F8 lighting work guards — 2026-10-04

Retained as a small measured GPU-cost reduction. The shared lighting shader now
avoids environment-map sampling when the frame uses constant ambient lighting,
and avoids sun visibility/shading work when the final shading normal faces away
from the sun. Rendering quality, samples, buffers and public APIs are unchanged.
This does not establish a delivered-FPS improvement or complete F8.

## Why the change is valid

The previous environment path evaluated irradiance, prefiltered specular and the
BRDF LUT before mixing their result with constant ambient. The frame's IBL flag
is uniform and written as zero or one by `prepare.rs`. With zero, that texture
work cannot contribute. The guard preserves the existing expression when IBL is
active, including sky rotation and intensity. It also preserves intermediate
blend values if the flag's representation changes later.

The sun's BRDF already returns zero when its shading normal faces away from the
sun. Previously cascaded-shadow PCF and contact visibility were evaluated first.
The new guard applies the same normal/light test before visibility sampling.
It uses the normal **after** normal mapping and front/back-face handling; the
geometric normal remains the normal used for shadow-map receiver bias.
Built-in PBR and authored lit shaders both use this shared function.

## Bounded iPhone comparison

Same dense-5000 fixture, medium profile, 1320×2868, MSAA 4 and tracing settings.
Frozen runs settle 120 ticks and show 1,767 visible instances. Live runs continue
simulation, so they are not identical geometry snapshots. Each run lasts eight
seconds and terminates its exact launched PID.

| Run | GPU frame time | Acquisitions/s, all windows | Acquisitions/s, excluding first |
| --- | ---: | ---: | ---: |
| Frozen guarded, first | 18.465 ms | 47.12 | 51.32 |
| Frozen original control | 19.329 ms | 49.62 | 49.87 |
| Frozen guarded, repeat | 18.224 ms | 51.00 | 51.44 |
| Live guarded, first | 16.543 ms | 41.80 | 42.50 |
| Live original control | 17.142 ms | 42.33 | 43.55 |
| Live guarded, repeat | 16.571 ms | 42.12 | 42.89 |

GPU values are medians of per-second p50 windows after excluding the first valid
window, not pooled frame medians. Acquisition rates are weighted by window
duration. Both acquisition summaries are retained: the first candidate frozen
window delivered only 17.9/s, so reporting only a post-startup value would hide a
real startup cost in that capture. The exact cause of that startup difference
was not isolated.

The frozen controls support about a 4.5–5.7% GPU reduction. Live GPU time is about
3.3–3.5% lower, but live delivery is slightly below the original control, not an
FPS win. These short runs do not establish sustained performance or heat relief.
All thermal samples were nominal. The initial connection-reset installation
failure is archived and excluded from timing comparisons.

Median render-request-to-acquisition ages were 33.09/33.33 ms for the two live
guarded runs versus 33.57 ms for the control. Their p99s were 51.33/50.51 ms versus
50.03 ms. Acquisition interval tails varied between runs; smoothness is not
qualified. Acquisition does not measure physical display scanout.

This is retained because repeated frozen and live measurements support reduced
GPU work, the implementation is two direct guards in the existing shader, and it
adds no resource or image-quality cost. Unlike the rejected reconstruction and
distance-cache experiments, it introduces no new passes, buffers or fallback
algorithm. It is not being accepted as meeting the overall FPS goal.

## Regression and validation

All 68 compared images are byte-identical, covering ambient/IBL, shadow
cascades, materials, normal maps, contact shadows, and authored shader lighting.
A new grazing-sun test exercises normal-map bands that face the sun while the
mesh normal faces away. Original and guarded shaders pass. Deliberately using
the geometric normal in the guard fails with zero lit bands, confirming the test
protects the distinction that makes this optimization safe.

55 rendering checks and 29 renderer unit checks pass (2 and 1 existing ignored,
respectively); renderer Clippy with warnings denied passes. The iOS release app
builds, and the retained app is installed and stopped on the iPhone. A local
copy for future controls is `target/f8-light-guards-retained.app`; earlier
`f8-reconstruction-baseline.app` contains the pre-guard shader.

The next work remains AO/blur cost and the CPU/presentation work limiting live
frame delivery. The main-scene GPU reduction does not remove those costs. F8
and sustained 60/120 Hz qualification remain open.

[Results and fingerprints](../benchmarks/foundation/results/2026-10-04-f8-light-guards.json)
and [raw captures, shader versions, images and regression logs](../benchmarks/foundation/results/2026-10-04-f8-light-guards-raw.tar.gz).
