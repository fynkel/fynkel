# Stage 16 plan: speed

Status: accepted 2026-11-01 ("yes to all"), with the recommended
answer to each decision (§7).
Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).
Stage 15 is done ([report](stage-15-report.md)).

**The direction** (the user, 2026-11-01): make what Fynkel has faster,
deeper and steadier before adding new things. This stage is the first
half: **speed**. The next is each system made deeper and steadier,
judged against Unity's same system and against what the games use. New
things (2D, kept as [a plan on hold](plan-2d.md); path finding; terrain;
cutting meshes) come when a game asks for them.

**The goal.** Unity's frame, built the same, is the bar: on every
device we have, Fynkel draws the Sheepdog field as fast as Unity does,
or the gap is measured, small and explained. Every bar an earlier stage
missed or never measured is measured and met.

## 1. Where it stands

Sheepdog's field, built the same in Fynkel and in Unity 6
(`tools/unity_field`, URP), frames a second:

| | Fynkel | Unity |
|---|---|---|
| Note10+, low | even | even |
| Note10+, medium | 45.3 | 52.1 |
| Note10+, high | 31.5 | 52–53 |
| iPad (6th gen, A10), medium | 30.0 | 39.6 |
| iPad, high | 18.7 | 28.3 |
| iPhone 16 Pro Max | not measured | not measured |

Where the time goes, from stage 14's pass-by-pass GPU timing:
- **Bloom and the finish pass**: on the Note10+ bloom is 2.65 ms and
  the finish (tone mapping, the glow added, onto the view) 2.2; on the
  iPad the finish is 5.8. URP does both in fewer, smaller passes.
- **Ambient occlusion** at high: 11.2 ms on the iPad.
- **Shadows**: 3.8 ms on the iPad at medium.
- **Anti-aliasing on older Apple GPUs** (RN11): soft and lit particles
  fixed, the rest of the cost not measured since.
- **Soft particles** still turn on the whole what's-behind path on
  Android (RN5 ½).

Bars missed or never measured:
- **Stage 15**: 100 tags with `hideBehind` among 3,000 objects, 1.27 ms
  on the Note10+ (bar 1 ms): each raycast tests every object's box. The
  tags never measured on the iPhone or the iPad.
- **Stage 13**: a mesh changed every frame, 1.3 ms on the Note10+ (bar
  1 ms).
- **The iPhone**: its budgets, Sheepdog at 120 fps and high (80 when
  last seen), the runs owed by stages 12 and 13. It was only reachable
  without a cable, and those runs sent no logs.

Smaller costs found in stage 14 and left: one moved object recomputes
every world matrix (WO10 ½); Android's pipeline cache written again
after every pipeline (RN3), a hitch the first time; the iOS layer holds
its lease while it waits for a drawable, so the main thread can wait up
to a second (BR5); a streamed voice copies its file each start (AU7);
world widgets allocating, the frame ring never shrinking, bloom kept
when off, and four more (BR6, BR7, PH6, PH7, RN9, DF13).

And `renderer.rs` is 3,516 lines, one `encode` function of 630, pipeline
making repeated about ten times (RN8): every GPU change above goes
through it.

## 2. How it's measured

- **Every device, by cable**: the Note10+, the iPad (6th gen), and the
  iPhone 16 Pro Max by cable this time, so its logs come through.
  Profile builds; 20 s to settle, a minute measured, as the lab's runs
  do now (`LAB_RATES`).
- **The GPU pass by pass** (stage 14's timing), so each fix says what it
  saved and where.
- **Unity beside it** for the field at each level on each device; and
  for two more scenes Fynkel is used for (decision 2): a crowd of
  animated characters, and a pile of physics bodies.
- **Sustained**: ten minutes, not one, on each phone at its default
  level, as the iPhone heat stage did, so a phone that gets hot and
  slows down shows it.
- **Kept**: each fix gets a benchmark in CI (`fynkel_bench`) where it
  can run on the Mac, so it stays fixed.

## 3. Design decisions

### 3.1 The renderer in passes first

RN8 before the GPU work (decision 1): `renderer.rs` split into passes of
their own (shadows, the scene, what's behind, ambient occlusion, bloom,
the finish), each with its pipelines made in one place. Nothing drawn
changes: every golden stays byte-for-byte the same, and the GPU timing
the same within noise. Then each GPU fix below touches one pass.

### 3.2 The GPU, against URP's frame

What URP does on a phone, pass by pass, is the guide:
- **Bloom**: fewer and smaller steps down and up, the first at a
  quarter of the screen, as URP's mobile bloom.
- **The finish**: tone mapping, the glow and the dither
  in one pass that reads the scene image once; on a tiled GPU, folded
  into the scene's own pass where nothing else needs the image.
- **Ambient occlusion** at high: at half resolution with its blur, as
  URP's (decision 3), its depth from the pass soft particles use.
- **Shadows**: cascades drawn only when what's in them moved or the sun
  turned, and casters culled per cascade.
- **Anti-aliasing on the A10**: the scene pass measured again with 4x;
  each remaining sample leaving the chip found and kept on it.
- **Soft particles on Android** (RN5 ½): the same depth pass as on iOS.

Each change is checked by the goldens: a change in the look is shown
to the user and agreed before it's blessed, as the scene image's 10 bits
were.

### 3.3 The CPU

- **A tree over the objects' world boxes** for `world.raycast` and
  `world.overlap`: refitted as things move, rebuilt when worn. A cast
  then looks at the few objects along its line. This meets stage 15's
  bar and makes every game's casts and taps cheaper.
- **World matrices**: only the moved objects and their children made
  again (WO10's other half).
- **A mesh changed every frame** (stage 13's bar): what's sent and how
  it's set, found by measuring on the Note10+.
- **The small rows** from stage 14 (RN3, BR5, AU7, BR6, BR7, PH6, PH7,
  RN9, DF13), each with its test first.

### 3.4 Budgets for every device

Stage 14 set a budget for each part of the frame on the Mac, the
Note10+ and the iPad. The iPhone gets its own, by cable. Each device's
default level is checked against its budget in the busiest scene, and
the guide's §20 says what each part costs on each device.

## 4. Phases

Each phase is one PR or a few, its tests and benchmarks first, in plain
words. Done when merged with CI green and measured on the devices.

### Phase 1: measure everything (S)

- The iPhone by cable: its budgets, Sheepdog at 120 and high, the owed
  runs of stages 12, 13 and 15.
- The field against Unity at each level on the three devices; the crowd
  and physics scenes against Unity (decision 2).
- Ten minutes sustained on each phone.
- One table of every number in this plan, today's, as the start line.

*Done when* every cell of that table has a number. **Done** 2026-11-02:
[the start line](stage-16-start.md).

### Phase 2: the renderer in passes (M)

- RN8 (§3.1). *Done when* every golden is byte-for-byte the same and the
  GPU timing is within noise on the three devices.

**Done** 2026-11-02. `renderer/` holds preparing a frame, recording it,
each pass as a function of its own (`passes/`) and every pipeline made
in one place (`pipelines/`). All 66 goldens are byte-for-byte the same.
The field, against [the start line](stage-16-start.md) (iPad and iPhone:
the GPU's ms a frame; Note10+: frames a second, the median of three):

| | low | medium | high |
|---|---|---|---|
| Note10+, before → after | 59.4 → 59.5 | 45.1 → 45.8 | 33.1 → 33.0 |
| iPad, before → after | 16.31 → 16.27 | 32.06 → 31.97 | 52.80 → 52.77 |
| iPhone, before → after | 2.10 → 2.22 | 11.72 → 6.51 | 12.54 → 12.48 |

The iPhone's medium is the one that moved. Its GPU reports a frame's
passes as one, and its numbers moved this much between runs in phase 1.
Nothing it draws changed, and it got quicker, not slower.

### Phase 3: the GPU (L)

- Bloom, the finish, ambient occlusion, shadows, anti-aliasing on the
  A10, soft particles on Android (§3.2), one at a time, each measured.

*Done when* the field is within 10% of Unity's frames a second at each
level on the Note10+ and the iPad, and Sheepdog holds 120 at high on
the iPhone, or the gap left is measured and explained pass by pass.

**Progress** 2026-11-02 (`stage-16-gpu`), the field against the start
line, Unity in brackets:

| | low | medium | high |
|---|---|---|---|
| Note10+, fps (median of 3) | 59.4 → 59.6 (59.5) | 45.1 → 47.7 (52.1) | 33.1 → 34.6 (53.3) |
| iPad, fps | 40.0 → 40.0 (59.4) | 30.3 → 32.7 (39.7) | 19.0 → 20.8 (28.5) |
| iPad, GPU ms | 16.3 → 16.3 | 32.1 → 27.2 | 52.8 → 48.1 |

- The iPad's own pick: the A10 and A11 low, the A12 and A13 medium, the
  A14 on high. Sheepdog on the iPad 11 → 42 fps; its memory no longer
  climbs (181 → 191 MB, was 180 → 449).
- The finish: bloom's glow blurred once at a quarter size and read with
  one tap; no tone-mapping curves where nothing glows. iPad 5.9 → 1.9 ms.
- Ambient occlusion's blur reads the distances its first pass worked out:
  iPad 10.7 → 9.15 ms. Its depth is 0.34 of it; the occlusion about 4.8.
- Shadows already take only the casters in each cascade.

The Note10+ now meets the bar at low and medium (−8%). Still short: the
Note10+ at high, the iPad at every level (its scene pass alone is 14.8
ms at low).

### Phase 4: the CPU (M)

- The tree over objects; world matrices; the mesh change; the small
  rows (§3.3).

*Done when* stage 15's 100 tags cost under 1 ms on the Note10+ (and are
measured on the iPhone and the iPad), stage 13's mesh change is under
1 ms there, and every small row is closed with its test.

### Phase 5: budgets, the guide, the report (S)

- Each device's budget; §20 with the numbers; the report with the
  start line and the end line side by side.

## 5. Not in this stage

New features for speed that a game hasn't asked for: levels of detail
(`LODGroup`), occlusion culling, baked lighting and GPU particles. Each
is worth it for a kind of game; when a game's scene needs one, the
measurements here will show it. Also out: the deeper-and-steadier work
on each system (the next stage), and 2D ([on hold](plan-2d.md)).

## 6. Measurements

On the Note10+, the iPad (6th gen) and the iPhone 16 Pro Max, profile
builds, by cable:

- **The field**: within 10% of Unity's frames a second at low, medium
  and high on each device; Sheepdog at 120 and high on the iPhone.
- **The crowd and the pile**: beside Unity's, the gap said.
- **Casts**: 100 tags among 3,000 under 1 ms on the Note10+.
- **Meshes**: one changed every frame under 1 ms on the Note10+.
- **Sustained**: ten minutes at the default level on each phone, the
  frames a second at the end within 10% of the start.
- **Nothing lost**: the goldens the same unless a change was agreed, the
  benchmarks within their bars, the soak's memory flat.

## 7. Decisions to make

1. **The renderer split (RN8) before the GPU work** (*recommended*:
   every GPU fix touches it, and the goldens keep it honest), rather
   than fixing passes in the file as it is.
2. **Two more scenes against Unity, a crowd and a pile**
   (*recommended*: animation and physics are measured against Unity
   too, not only drawing), rather than the field alone.
3. **Ambient occlusion at half resolution, as URP's** (*recommended*:
   a softer look the user sees before it's blessed), rather than full
   resolution made cheaper some other way.
4. **The bar is within 10% of Unity on each device** (*recommended*:
   close enough that no one would pick Unity for speed), rather than
   matching it exactly or beating it.
5. **No new speed features** (levels of detail, occlusion culling) in
   this stage (*recommended*: the user's direction; a game asks for
   them first), rather than adding them now.
