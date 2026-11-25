---
title: "Quality levels"
description: "What each device draws, and how to choose."
---

What a world draws, and how finely, is its `Quality`: whether there's
bloom, ambient occlusion and shadows, and how fine the shadows are. As in
Unity, a game can have one quality or several, and change them as it
likes. Three levels come with the engine, to use as they are or start
from:

| | `Quality.low` | `Quality.medium` | `Quality.high` |
|---|---|---|---|
| `hdr`: drawn into an image of its own (10 bits a colour), then to the screen | no | yes | yes |
| `bloom` | no | yes | yes |
| `ambientOcclusion` | no | no | yes |
| `lampHighlights`: lamps give shiny surfaces a highlight | no | yes | yes |
| `shadows` | yes | yes | yes |
| `shadowCascades`, `shadowResolution` | 1 × 1024 | 2 × 2048 | 3 × 2048 |
| `shadowSoftness` | soft | soft | softer |
| `shadowDistance` | 20 m | 60 m | 100 m |
| `spotShadows`, `spotShadowResolution` | 1 × 512 | 2 × 1024 | 4 × 1024 |
| `renderScale`: pixels drawn per pixel of the screen | 0.75 | 1 | 1 |
| `antiAliasing`: samples a pixel, smoothing edges (MSAA) | 4 | 4 | 4 |
| `softParticles`: particles fade where they meet what's behind | no | yes | yes |

When the game doesn't choose, the device picks a level: high on desktops' GPUs
and Apple's from the A14 (iPhone 12, 2020) on, medium on the A12 and A13, low
on older Apple GPUs (a 2018 iPad's A10 draws a busy scene at 11 fps at high,
40 at low), on Android phones (Mali, Adreno and the rest), on a software GPU
and on one it doesn't know.

`Quality.device` says which. Phones get low because a Galaxy Note10+ (2019)
doesn't hold 60 fps at medium: about 40–50 in a scene with everything ([the
report](https://github.com/fynkel/fynkel/blob/main/docs/android-pacing-report.md)).
A newer phone may: a game can offer medium on its settings page and let the
player choose.

Most games set one quality and keep it; a game with a settings page can switch
at any time, and every view draws the new one from its next frame:

```dart
/// A game's one quality: `Quality(...)` starts from medium's settings;
/// this one has no bloom, and one sharp cascade of shadows.
const myQuality = Quality(name: 'mine', bloom: false, shadowCascades: 1, shadowSoftness: ShadowSoftness.hard);

/// Or levels of its own, for a settings page, each from the engine's.
final myLevels = [
  Quality.low.copyWith(shadowDistance: 25),
  Quality.medium,
  Quality.high.copyWith(ambientOcclusion: false),
];

/// The settings page's choice: one of them, or null to let the device
/// pick. Gives what's drawn.
Quality chooseQuality(World world, Quality? chosen) {
  world.quality = chosen;
  return world.quality ?? Quality.device;
}
```

## Choosing by device

As in Unity, the engine picks one level per
kind of device and the game decides the rest: Unity has a default
quality per platform, and games that want more check the device and
call `QualitySettings.SetQualityLevel`. `SystemInfo` says what the GPU
is, as Unity's does, for a game that keeps its own list of slow ones:

```dart
/// An older iPad's GPU draws high at 19 fps and low at 53 (Sheepdog's
/// field, an A10): low, with fewer samples.
Quality? qualityFor(String gpu) => switch (gpu) {
  'Apple A9 GPU' || 'Apple A10 GPU' => Quality.low.copyWith(antiAliasing: 2),
  _ => null, // the device's own pick
};

void pickQuality(World world) => world.quality = qualityFor(SystemInfo.graphicsDeviceName);
```

`SystemInfo.graphicsDeviceVendorId` (Apple's 0x106B, Arm's 0x13B5,
Qualcomm's 0x5143) and `SystemInfo.graphicsApi` (`metal`, `vulkan`) say
the rest. Other games time a few seconds of play on the first launch,
or leave it to the player's settings page.

## In detail

### Fewer pixels: renderScale

Fewer pixels: `renderScale`, as Unity's render scale. At 0.75 the views
draw about half the pixels and stretch the picture to fit: a little softer,
and much cheaper on a big screen (an iPhone 16 Pro Max is 1320 × 2868).

Low draws at 0.75: on a Galaxy Note10+, Sheepdog at low went from 43 fps to
55. Put it on the settings page beside the levels: `world.quality =
Quality.high.copyWith(renderScale: 0.75)`, or
`Quality.low.copyWith(renderScale: 1)` for low at every pixel. A view's own
`FynkelView(renderScale:)` multiplies it, for one view (a minimap).

### Smoother edges: antiAliasing

Smoother edges: `antiAliasing`, as Unity's anti-aliasing (MSAA): 4
samples a pixel at every level, or 2, or 0 for none. On phones' GPUs the
samples stay on the chip, so 4 costs little: on the Note10+ at low, Sheepdog
gained nothing without them, and its sheep's edges were jagged. A device
that can't draw 2 draws none.

### A camera asks, the quality decides

A camera's bloom and ambient
occlusion ([Bloom and ambient occlusion](/graphics/bloom-and-ao/)) are drawn
only where the quality has them on, so a cheaper quality for older phones
can turn bloom off without touching the cameras.

A debug build says once when a camera asks for one that the quality drawn
leaves out (once a run for each quality, so a test file making a world at
low for each test says it once); for a cheaper quality chosen on purpose,
that's expected, and there's nothing to change.

### Qualities compare by value

Qualities compare by value: `world.quality` gives back what was set, and
`Quality.medium.copyWith(bloom: false) == Quality.medium.copyWith(bloom:
false)`. Give your own a `name` (in the constructor or `copyWith`) for a
settings page or a log.

### Settings out of range

Settings out of range (`shadowCascades` 1 to 3, resolutions powers of
two from 256 to 4096, `spotShadows` 0 to 4, `shadowDistance` above 0) throw
an `ArgumentError`, and the world keeps the quality it had.

### Without hdr or bloom

Without `hdr` or bloom, the scene is drawn straight to the screen in one
pass: the cheapest.
