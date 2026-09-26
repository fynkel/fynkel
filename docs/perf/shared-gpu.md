# Shared GPU device — the ADR-0030 §4 measurement

**Date:** 2026-09-26 · **Machine:** Apple M3 Max, macOS 26.5.1 · **Build:** release  
**Test:** `platform/apple/tests/shared_gpu.rs`, run with

```sh
caffeinate -du cargo test --release -p fynkel_platform_apple --test shared_gpu -- --ignored --nocapture measure
```

## What it measures

The editor's case, twice: one device per viewport (as before viewport
ABI 10), and one device shared by every viewport (ABI 10).

- A Scene viewport at 1600×1000 draws 400 lit cubes with a sun that casts
  shadows, so the renderer's whole frame runs (shadows, AO, post).
- Then three Inspector previews at 800×480 are added one at a time. Each
  is timed from create until its first frame is on screen.
- Then 240 frames of the Scene view are drawn, with one preview drawing
  beside it every frame.

Memory is the process's physical footprint (`proc_pid_rusage`). It is
what Activity Monitor shows, and on Apple silicon it includes GPU memory.

Each session runs in a fresh process. One warm-up run fills Metal's
shader cache first; then the modes are interleaved (own, shared, shared,
own, own, shared). Every figure is a median. Run to run, the numbers
varied by under 1 ms and 0.5 MB.

## Results

|                                    | a device each | one shared device |
|------------------------------------|--------------:|------------------:|
| Scene viewport, create → on screen |       72.5 ms |           73.1 ms |
| a preview, create → on screen      |       13.0 ms |       **12.0 ms** |
| memory: the Scene viewport         |      +516 MB  |          +517 MB  |
| memory: each preview               |     +55.6 MB  |      **+45.1 MB** |
| memory: all four viewports         |      +683 MB  |      **+654 MB**  |
| Scene frame, preview beside it     |      0.888 ms |          0.881 ms |
| both frames                        |      1.489 ms |          1.482 ms |

## Verdict: sharing stays

- **A second viewport** is on screen in 12.1 ms. The bar was 50 ms.
- **Memory grows less:** 654 MB shared against 683 MB with a device
  each. Each extra viewport costs 10.5 MB less.
- **The Scene view's frame** changes by −0.8 %, and by +0.1 % on a
  second full run. That is noise. The bar was +5 % or less.

## What we learned

- **A device is cheap on Metal.** The `MTLDevice` is the system's, one
  per GPU, whatever the app does. A wgpu device on top of it costs about
  1 ms and 10 MB: its command queue, memory pools and caches. Sharing
  saves that for every viewport, but it was never the big cost.
- **The big cost is the renderer.**
  - Each 800×480 preview still adds about 45 MB. That is its render
    targets, shadow maps and environment, and its own pipelines.
  - The first viewport adds about 517 MB. Most of that is paid once per
    process: Metal itself and the shader compiler.
- **What would save more:** sharing, between renderers, the resources
  that do not depend on a viewport's size. These are pipelines, the
  environment and the uploaded meshes and textures. It would also help to
  give a preview a lighter quality profile (smaller shadow maps, no AO or
  bloom). A shared device is what makes the first possible.
- **Next steps.** The first of these is the "later" step in ADR-0030
  §4. The second belongs with the preview stages (ADR-0030 step 5).
