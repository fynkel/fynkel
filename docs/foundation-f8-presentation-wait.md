# F8 presentation wait policy — 2026-10-03

The presenter now acquires the newest completed image immediately. It uses its
existing bounded wait only when no new image is Ready, and wakes when the first
completion becomes available. It no longer delays an available image while
waiting for a newer submission. The 3,000 µs maximum remains unchanged; zero
still disables waiting. No buffer, resolution, quality, simulation or ownership
settings changed.

## Why this change

The [early admission captures](foundation-f8-render-admission.md) showed that
almost every acquisition already had a completed image but still waited about
3.38 ms. A new zero-wait control retained 38.4 fresh acquisitions/s and lowered
median completion-to-acquisition time from 11–13 ms to 9.1 ms, though interval
tails varied. The narrower policy preserves bounded waiting when it can avoid
repeating an old image, while eliminating waits when useful output already exists.

`IoSurfaceRing::acquire_presented` retains the original deadline across wakeups,
checks for any Ready slot, and waits only while submitted GPU work exists. It
continues to select the newest Ready slot and apply the same Presented/Retiring
rotation. Reserved CPU work is neither presented nor waited for. A newer
incomplete texture cannot displace a completed one. Rust, C and Dart API comments
now describe this policy; the ABI and callable signatures are unchanged.

## Device evidence

Same dense-5000 scene and iPhone 16 Pro Max, full 1320×2868 resolution, medium
quality and four buffers. All four new captures completed their eight-second
windows with nominal thermal samples and exact-PID termination.

| Measurement | Admission fix, old wait policy (two runs) | New policy (two runs) |
| --- | ---: | ---: |
| Fresh acquisitions/s, traced | 38.5 / 39.2 | 43.0 / 39.2 |
| Median acquisition call | 3.483 / 3.494 ms | 0.094 / 0.123 ms |
| Median wait with a completed image available | 3.381 / 3.382 ms | 0 / 0 ms |
| Median render request → acquisition | 36.0 / 35.1 ms | 33.4 / 33.8 ms |
| p95 render request → acquisition | 43.6 / 44.0 ms | 41.8 / 40.9 ms |
| p95 acquisition interval | 41.3 / 38.4 ms | 36.5 / 42.1 ms |

The untraced statistics-window control delivered 42.10 acquisitions/s, versus
36.30 after the admission fix with the old wait policy. The previous control
before either fix was 29.24. These are separate short captures with run-to-run
variation, not a promised sustained gain or a 43 FPS floor. The strongest
conclusion is that unnecessary raster-thread waits are eliminated and measured
image age is lower. Acquisition interval tails did not improve consistently.

The new median request-to-acquisition time still exceeds the 27–29 ms measured
before early admission. This change addresses part of that latency increase;
it does not close the overall latency or smoothness issue. GPU work and host
simulation/preparation remain expensive. The next major target is the measured
screen-space GPU cost, with controlled effect isolation and image regressions.

[Results and fingerprints](../benchmarks/foundation/results/2026-10-03-f8-presentation-wait.json)
include the zero-wait control and before/after analyses. [Raw new captures](../benchmarks/foundation/results/2026-10-03-f8-presentation-wait-raw.tar.gz)
can be reanalyzed with `benchmarks/mobile/analyze_frames.py`. Baseline raw captures
are linked from the admission report. The analyzer now includes
`renderRequestToAcquisitionUs`, matching reservation generation/sequence to the
containing host render call and eventual acquisition; it is not input-to-photon
latency. Some matched acquisitions extend past the last host frame in the window.

## Verification and limits

21 Apple-platform tests passed, including new regressions for immediate Ready
acquisition despite newer GPU work and waking on the first completion instead of
exhausting a five-second test budget waiting for a newer frame. Existing texture
lifetime, resize, admission, cancellation and rendered-image regressions remain
intact. Release Clippy, 8 Flutter game-view tests, 13 Python tests and the iOS
release build passed. The timeline regression checks request-to-acquisition
correlation explicitly.

The observed acquisition counts do not prove physical display presentation,
uniform motion, sustained thermal behavior or either 60/120 Hz qualification.
F8 remains open. The improvement is retained as a measured reduction of
presentation waiting, without increasing queues or reducing visual quality.
