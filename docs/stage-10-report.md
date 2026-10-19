# Stage 10 report: shaders

Phase 5 of [the stage 10 plan](stage-10.md). Phases 1 to 4b are merged
(#126–#130). Phase 5 is #131.

## The result

Stage 10's goal was a game's own look, written in a few lines of WGSL
and driven from Dart: surfaces (water, lava, a hologram), movement
(grass in the wind, a cape), what's behind a surface (a pond's depth, a
glass ball that bends the scene), whole styles (toon, outlines, a screen
effect over everything). Each had to be lit, shadowed and fogged by the
engine with nothing more, and hold the frame rate on all three devices.

**That is done.** Each kind of shader has its lab scene, and two games
use them: Sheepdog's field sways in the wind round a pond, its wolves
burn away when chased off and its sheep flash when bitten; coins'
coins are holograms, and a switch draws the whole game toon. The Mac and
the iPhone hold 60 in every scene at every level, and the Mac 120 when
asked; the iPhone holds 120 in the lab's scenes; the Note10+ holds 60
at low in every lab scene. The gaps: Sheepdog on the Note10+ is still
about 50 fps at low, its gap since stage 8; Sheepdog at 120 on the
iPhone came out at 80 (below), possibly since phase 4, not confirmed;
and reading what's behind costs more than the other kinds, so
Sheepdog's pond doesn't read it at low.

What a game writes:

```wgsl
// assets/shaders/burn.wgsl: a wolf burning away as it runs
struct Values { edge: vec4<f32> }
struct PerObject { burnt: f32 }

fn surface(in: SurfaceInput) -> Surface {
    var s = standard(in);
    let n = noise(in.uv * 5.0) * 0.7 + noise(in.uv * 13.0) * 0.3;
    let amount = per_object.burnt * 1.1 - 0.05;
    s.alpha = step(amount, n);
    let near = 1.0 - smoothstep(0.0, 0.08, n - amount);
    s.emission += values.edge.rgb * near * 6.0;
    return s;
}
```

```dart
final coat = Material(shader: await assets.shader('shaders/burn.wgsl'), alphaCutoff: 0.5,
    values: {'edge': const Color(0xFFFF7A20)});
// when it's chased off: burn this wolf alone, then it's gone
tween(0.0, 1.0, seconds: 0.9, onUpdate: (v) => renderer.set('burnt', v)).then((_) => destroy());
```

## What stage 10 did

| Phase | What | PR |
|---|---|---|
| 1 | Surfaces: a material's own `fn surface` (or `fn unlit`) in a `.wgsl` asset, its `Values` set by name from Dart, four `PerObject` numbers per renderer, textures, time; errors with the shader's own file and line; hot reload | #126 |
| 2 | Movement: `fn vertex` after the skinning, with the material's `reach` so culling and shadows follow it | #127 |
| 3 | What's behind: `depth_behind`, `scene_depth`, `scene_color`, kept only while a reader is in view | #128 |
| 4 | Whole styles: a shader's own lighting (`fn light`), outlines, screen effects in order, the pixel look | #129 |
| 4b | The shader experience: no first-use freeze (`world.shadersReady()`), the guide's §14, `fynkel.wgsl`, `dart run fynkel:shader`, effects with textures and depth, arrays in `Values` | #130 |
| 5 | Sheepdog's and coins' shaders, the measure, the guide's costs, this report | #131 |

The C interface went from ABI 32 to 37. Phase 5 doesn't change it.

## Phase 5

- **Sheepdog** (decision 6):
  - **Grass**: 36 patches of 200 blades, each patch one mesh, one
    material for them all. The blades bend from the foot, more at the
    tip, with gusts running across the field. They're left off the
    minimap and cast no shadow.
  - **A pond** in a dip of the field: the ground turns to sand under
    it (the field's own shader), and the water shows that bed through
    its ripples, darker the deeper it is, foaming at the bank. Nothing
    walks into it: what would slides round it.
  - **Wolves burn away** when chased off, each on its own (its
    `burnt` number), embers glowing at the edge, shadows cut out too.
  - **A bitten sheep flashes** before it goes.
  - At low the pond is still water (below).
- **Coins**: the coins are holograms (unlit, additive, with a rim and
  scan lines). A "Toon" switch on the HUD draws everything in three
  bands with a dark outline, and the next level comes in the style.
  *Known limit:* on a device that hasn't seen the toon shader, restyled
  objects are missing for the frames its pipelines take.
- **The lab's tour** (`--define TOUR=shaders` to its `tool/measured.dart`)
  measures each stage 10 scene at each level in one run, with how long
  its shaders took to be ready.
- **The guide**: §5's materials point to §14; §14 gains "What shaders
  cost"; §20 a line on reading what's behind.

### Found on the way

- **The Note10+ lost its GPU** on the first frame that read what's
  behind. Its Mali driver fails to make (`VK_ERROR_INITIALIZATION_FAILED`)
  a pipeline that reads a multisampled depth texture as depth. The
  engine copied one sample of the scene's depth that way. It reads it as
  floats now, which Mali compiles, and the picture is the same. Phases 3
  and 4b hadn't been run on the phones.
- **Reading what's behind is the costly kind.** The frame takes three
  passes while such a surface is in view. On the Note10+ Sheepdog went
  from 51 fps at low to 40 with the new shaders: 8 of those were the
  pond, about 6 of them the reading itself (the pond drawn as plain
  see-through water got 46). Not keeping the multisampled depth didn't
  help, so it's the passes, not memory. Sheepdog's pond is still water
  at low now, as a game would do for a cheaper level, and it's back to
  50. Making the passes cheaper on tile GPUs is renderer work for later.
- **A covered window gets no frames on macOS**, which had emptied the
  Mac's numbers. A measured window now floats above the others on every
  Space.
- **iPhone runs at 120 depend on the phone's state.** A run started warm
  shows 80 until the phone cools, whatever the scene; after an hour and
  a half of runs it showed 80 at every level, on stage 9's code too,
  still "nominal".
- **On the Note10+, drawing fewer pixels helps only so far.** Sheepdog
  at low (`--define SHEEPDOG_SCALE=` to its measure): render scale 1
  43.9 fps (GPU busy 90 %), 0.75 51.9 (74 %), 0.6 50.9 (the GPU slows its
  clock). Past 0.75 the limit is pacing between the engine and Flutter:
  Flutter's raster misses 16.7 ms on 15–25 % of frames. 0.75 looks
  barely softer; 0.6 visibly.

## Measured

Frames a second asked for: 60, or 120 where said. "Ready" is how long
a scene's shaders took to be ready after it loaded, on a device that
hadn't seen them (a fresh install), at low. The other levels share
most of them: 17–160 ms on every device. On the Mac, with its shader
cache cleared, the scenes were ready 20 ms to 0.6 s after loading, toon
the slowest.

### The lab's scenes

| Scene | Mac 60 / 120 | iPhone 60 | iPhone 120 | Note10+ low / medium / high | Ready: iPhone, Note10+ |
|---|---|---|---|---|---|
| shaders | 60 / 120 | 60 | 120 † | 59.9 / 59.7 / 59.9 | 153 ms, 1.1 s |
| movement | 60 / 120 | 60 | 120 † | 59.5 / 59.9 / 54.7 | 36 ms, — |
| what's behind | 60 / 120 | 60 | 120 | 59.8 / 48.1 / 41.2 | 36 ms, 89 ms |
| toon | 60 / 120 | 60 | 120 | 59.9 / 59.8 / 59.9 | 569 ms, 408 ms |
| screen effects | 60 / 120 | 60 | 120 | 59.9 / 59.9 / 59.9 | 271 ms, 53 ms |

† 80 in the first two minutes of a run started on a warm phone, then
120 (above).

The Note10+'s ready times: the first scene a run shows makes the
engine's own pipelines too, so its 1.1 s is mostly theirs; Android
keeps them from the next run on. Its movement time wasn't recorded.

### Sheepdog, everything on

| | low | medium | high |
|---|---|---|---|
| Mac, 60 / 120 | 60 / 120 | 60 / 120 | 60 / 120 |
| iPhone, 60 | 60 | 60 | 60 |
| iPhone, 120 | 80 * | 80 * | 79 * |
| Note10+, 60 | 50 (still water) | 26 | 21 |

On the Note10+, medium was about 40 before this stage; it picks low
itself.

\* At 120 a frame has 8.3 ms; one that runs over drops the iPhone to its
next step, 80. Sheepdog at high was near that edge before (stage 8 saw
76 in one second, stage 9's crowd 80 at high). The first runs of the
session, on a phone rested and thermally nominal: this branch 79 (79
with the new shaders off too), stage 9's end 120, phase 3's end 118,
phase 4's first commit 120, its outlines commit 80. So phase 4 may have
tipped high over the edge; nothing in the outlines commit costs a frame
without outlines that we could find. Later in the session every version
gave 80, stage 9's and low's too, so those runs can't tell. Open: a
check on a rested phone, this branch against stage 9.

## Tests

- Sheepdog: game 28 (five new), app 10. Coins: 8 (two new). The guide's
  code (`tools/check_guide.py`): 58 blocks.
- The renderer's behind and styles tests pass with the depth read as
  floats.

## Not in this stage (decision 7)

A shader graph, compute shaders and particles, decals, reflections of
the scene, geometry and tessellation shaders, a game's own passes and
render targets, a shader's own variants and keywords. They're in the
guide's §21.

## Next

Animation 2, agreed for after stage 10. Then stage 11, sound. Open:
the iPhone's Sheepdog at 120 (above); on Android, a render scale of 0.75
at low and edge smoothing as a setting, and the pacing that keeps
Sheepdog near 52 on the Note10+.
