# ADR-0061: Textures are cooked GPU-compressed (ASTC)

**Status:** Accepted 2026-10-06 (the user accepted it as written; Phase 4; docs/foundation.md 5.5)  
**Date:** 2026-10-06  
**Related:** ADR-0040 (background cook), ADR-0056 (residency and budgets), [renderer.md](../renderer.md) § material maps, [documents.md](../documents.md) § The cook

## Context

Every texture is cooked as raw RGBA8 (or RGBA16F for an HDR sky) with its
full mip chain (`TEXR` formats 1–3). A 4096² colour map is 85 MB on the GPU
and on disk, a 2048² map 21 MB. The GPU reads every one of those bytes
whenever it samples, which costs memory bandwidth: on the iPhone the GPU is
the bottleneck (docs/foundation-f8-throughput-observations.md).

Unity, like every engine, ships textures **block-compressed** in a format
the GPU samples directly: ASTC on iOS and Apple Silicon Macs, BC (DXT) on
Windows, ETC2/ASTC on Android. Its texture importer has a `Compression`
setting (None / Low / Normal / High quality) and a `Max Size`. Its iOS
default is ASTC 6×6.

What we have measured on this machine (2026-10-06):

- **The GPUs support it.** wgpu 30 on the M3 Max reports
  `TEXTURE_COMPRESSION_ASTC`, `_ASTC_HDR`, `_BC` and `_ETC2`. Every Apple
  GPU since the A8 (iPhone 6) and every Apple Silicon Mac decodes ASTC LDR.
  Intel Macs do not have ASTC; neither might an older iOS simulator.
- **An encoder exists and is fast.** `ctt-astcenc` 0.6 (vendored ARM
  astcenc, MIT/Apache-2.0/Zlib + Apache-2.0, by a wgpu maintainer) builds on
  arm64 with NEON. On a 1370×1148 colour map from the reference game:

  | block | size | vs RGBA8 | PSNR | time (medium, parallel) |
  | --- | --- | --- | --- | --- |
  | 4×4 | 1.5 MB | 4× smaller | 42.5 dB | 109 ms |
  | 6×6 | 0.7 MB | 9× smaller | 35.5 dB | 98 ms |
  | 8×8 | 0.4 MB | 16× smaller | 31.7 dB | 140 ms |

  (`intel_tex_2`, the other candidate, has ASTC stubbed out with
  `unimplemented!()`.)

Only the renderer reads cooked texture payloads (`Assets::texture` →
`residency.rs` → `upload_rgba8`); the editor's previews go through the
renderer. So the change has two ends, the cook and the upload, and nothing
in between.

## Decision

### 1. The cook compresses (one payload for every Apple target)

LDR textures are cooked as **ASTC**, every mip level compressed on its own
(a level smaller than a block is padded, as ASTC allows). New `TEXR`
formats, the header otherwise unchanged:

| format | contents |
| --- | --- |
| 4 / 5 | ASTC 4×4, sRGB / linear |
| 6 / 7 | ASTC 6×6, sRGB / linear |
| 8 / 9 | ASTC 8×8, sRGB / linear |

A level is `ceil(w / bx) · ceil(h / by)` blocks of 16 bytes.

*Amended 2026-10-06, while implementing:* WebGPU (and wgpu) requires level 0
of a compressed texture to be a whole number of blocks across and down.
Metal does not, but we go through wgpu. So a texture whose size is not a
multiple of its block is **resampled at cook to the nearest multiple**
(Catmull-Rom, in linear light for sRGB): 1024² at 6×6 becomes 1026², a
0.2 % stretch the UVs absorb (they span the texture, not texels). 4×4 and
8×8 leave every power-of-two texture of 8 px or more as it is. Unity
scales non-power-of-two textures for compressed formats in the same way.
`info` records the cooked size next to the source size.

*Amended 2026-10-06, after the showcase comparison:* a texture whose
shorter side is **under 32 px stays RGBA8** whatever its setting. The
showcase robot's material is one 2×2 image; fitted to a 6×6 block it
wrapped differently and the robot's pattern visibly moved, for a saving
of bytes. At 32 px and up, the stretch to whole blocks is a few percent
at most. HDR images
stay RGBA16F (ASTC HDR can follow; the sky is one texture). The encoder
runs at astcenc's `medium` preset in every cook profile, so what you test
in the editor is byte for byte what ships. The cache version goes 12 → 13;
the image importer's version does not change (the import is the same; the
cook encodes).

### 2. A sidecar setting, like Unity's Compression

`settings.compression` on an image's sidecar (and on a model's sidecar,
for its embedded textures): `none` (RGBA8, as today) | `low` (8×8) |
`normal` (6×6) | `high` (4×4).

**Default:** `normal` (6×6) for an sRGB map, `high` (4×4) for a linear one.
A linear map is almost always data — normals, metallic/roughness,
occlusion — where block error shows as lighting error, and 4×4 keeps it
small. This is Unity's iOS default for colour maps and its usual advice
for normal maps.

The Inspector shows the setting and the cooked size next to it, as Unity
does.

### 3. The renderer uploads blocks, or decodes them where it must

The renderer requests `TEXTURE_COMPRESSION_ASTC` when the adapter offers
it and uploads the blocks as they are (`bytes_per_row` = blocks across ·
16). On an adapter without ASTC (an Intel Mac) the renderer decodes the
payload to RGBA8 before upload, with a pure-Rust decoder (`astc-decode`,
Apache-2.0) so the runtime on a device carries no C++ encoder, and reports
it once (a notice, not an error). The
picture is the same; only the memory saving is lost.

GPU accounting (ADR-0056) counts the compressed bytes, so a texture budget
holds 4–16× more textures.

### 4. Not in this ADR

- **BC for Windows/Linux.** No such target exists yet. When one does, the
  release cook for it encodes BC7 from the same source; the format codes
  above leave room.
- **Max Size** (Unity's per-texture downscale for a platform). Useful for
  phones; a separate, small follow-up if wanted.
- **Freeing the CPU payload after upload** (foundation 5.5's second half).
  It is a residency change (ADR-0056), not a format one.

## Consequences

- A project recooks once (cache 13). Cooking a large project takes longer
  the first time: ≈ 0.1 s per megapixel of texture with every core. ADR-0040
  keeps the editor responsive while it runs; the cache makes it one time.
- Texture memory and disk drop 4× (linear maps) to 9× (colour maps). The
  GPU reads fewer bytes per sample; on a bandwidth-bound phone that is
  frame time. How much is to be measured on the iPhone, not assumed.
  *Measured 2026-10-06* (docs/foundation-f8-throughput-observations.md):
  on dense-500 with 1024² maps on every material, the cooked textures
  went from 130 MB to 28 MB and the app from 163 MB to 56 MB, and the
  frame rate and GPU time did not change (90 fps with every effect, 120
  without AO and contact shadows, either way): that scene's GPU time is
  the screen-space passes, not texture reads.
- Compression is lossy. 6×6 on a colour map is visible only on close,
  high-contrast detail; `high` or `none` per texture is the escape hatch.
  The renders the tests compare (showcase, reference game) change, so their
  references are regenerated once and reviewed by eye.
- One native dependency is added to the cook only (`ctt-astcenc`, C++
  built by `cc`, in `fynkel_import`); the renderer adds the pure-Rust
  `astc-decode` for its fallback.

## Steps

1. `TEXR` formats 4–9 in `fynkel_cooked` (sizes, encode/decode, tests).
2. The cook encodes (`compression` setting, defaults, cache 13); the
   validator and the sidecar schema know the setting.
3. The renderer requests ASTC, uploads blocks; the fallback decodes.
4. The Inspector shows the setting and the cooked size.
5. Renders regenerated and reviewed; iPhone capture of the dense scene,
   before and after, at native resolution with every effect on.
