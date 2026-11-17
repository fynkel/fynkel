# Stage 17 plan: deeper and steadier

Status: accepted 2026-11-04 (the order and the recommended answers, §5).
Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).
Stage 16 is done ([report](stage-16-report.md)).

**The direction** (the user, 2026-11-01): make what Fynkel has faster,
deeper and steadier before adding new things. Stage 16 was speed. This is
each system made deeper and steadier, judged against Unity's same system and
against what the games use. New things come when a game asks for them.

**The goal.** Nothing a game has given up or worked around because of the
engine stays that way without a reason written down, and the systems games
lean on most have the tests to prove they hold.

**The order: steadier first, then deeper.** A phase's bar is below it.

## 1. Where this comes from

A survey on 2026-11-04 of what the games call, what they work around, and
the audit rows left open, with its main claims then checked in the code.
How far to trust it:

- **One real game.** Ring Ring Deals (`scamonthephone`) is the only game that
  isn't an example here, so much of "what games need" rests on it.
- **Unity's side** comes from what's known of Unity and the earlier stage
  reports, not a new side-by-side.
- Nothing was run for the survey. Each phase starts by reproducing what it's
  about, and a row that doesn't reproduce is dropped and says so.

## 2. Steadier

### Phase 1: outlines flash

Ring Ring Deals took outlines out: "Fynkel's outline shell glitched on boxes
for a frame now and then, and scribbled on faces up close"
(`packages/office/lib/src/look.dart:3`; commit b4280f9, "flashed a dark
wedge"). It's the only bug here that made a real game give up a feature. No
audit row covers it and nothing has fixed it. The picture tests look at one
frame at a time, so they can't see a flash.

**Bar:** a test that fails on the flash, over many frames, and then passes;
faces close up shown to the user and agreed, since this changes the look;
outlines usable in the office, which the user can then choose to turn back on.

**Done** 2026-11-04. What it was: the game's box is double-walled, an inner
wall exactly on each outer one, so the outline's back faces lay on the box's
front and fought it for depth, which is why it came and went with the camera.
Its normals also cancelled out at each corner, so it never had a ring. The
outline now sits a touch further away, and such a corner's outer walls are
pushed away from the mesh's middle. Checked on a Mac by test, on the Note10+ by
three screenshots and on the iPad by the user's eyes; the iPhone not yet.

Left as they are, agreed with the user:
- That ring is uneven, thicker on some sides, as the push isn't along each
  wall. Accepted for now. Solid meshes don't take this path.
- **Faces up close.** Not a glitch: each small part (glasses, eyes, brows) has
  its own outline, its width in metres, thick beside them and thicker as the
  camera nears. A game keeps outlines off faces, as Ring Ring Deals does. A
  width in pixels would stop it growing, not the lines round each part, so it
  comes if a game asks.

### Phase 2: running out of memory

**Done** 2026-11-04, smaller than planned, agreed with the user.

**Corrected: texture compression is postponed.** This plan called it likely
the biggest saving without measuring it. Measured, it would save almost
nothing today: Sheepdog's textures hold 0.3 MB of GPU memory, and Ring Ring
Deals' 53 models contain no textures. It comes back when a game has large
textures, with its own ADR.

**The warning reaches the game.** `world.onLowMemory` is Unity's
`Application.lowMemory`, told once per world per warning. While the app is
away, its views let go of the images they draw through
(`WorldView.releaseImages`), and the next frame makes them again with the same
picture. On screen they keep them: they'd only be made again at once, with a
slow frame. Tested on a Mac; on the Note10+ a real warning
(`am send-trim-memory`) reached the world, on screen and away, and images stayed
on screen. Away, they already read 0 before the release ran, and why isn't
known, so the release itself is proved by the tests only. The iPad's warning
hasn't been tried: it can't be sent from here.

**What the memory is.** Sheepdog on the iPad, in MB, from `fynkel_measure`:

| | low (1152×1536) | high (1536×2048) |
|---|---:|---:|
| Footprint | 191 | 406 |
| GPU, all of it | 43 | 232 |
| Bare Flutter's GPU memory | 14 | 14 |
| Frames on their way (three drawables) | 20.3 | 36 |
| What's behind | 0 | 72 |
| Shadow maps | 2 | 24 |
| Scene image, ambient occlusion | 0 | 19.5 |
| Meshes | 5.1 | 5.1 |
| Bloom | 0 | 1.7 |
| Textures, sky | 0.5 | 0.5 |
| Not named | about 1 | about 59 |

So low is explained. The biggest part of high is what's behind, 72 MB: the
pond's water reads it, only at medium and up, and it keeps a stored multisampled
depth. Those 59 MB may be the map's view: the report counts one view, and
the map has its own images. That hasn't been checked. Nothing was shrunk: that
waits for the user to see these.

**Found on the way: a view could freeze for good.** Configuring a surface fails
if another thread is submitting to the GPU, and one failure stopped the iOS
layer until its size next changed. Sheepdog has two views, so at high on the
iPad 6 runs in 21 drew one frame and no more; a player would have seen a frozen
picture. It now tries again next frame: 0 runs in 12. Older than this stage. A
new layer's first configure, on the platform's thread, can lose the same race;
that one reports an error rather than freezing, and is left as it is.

### Phase 3: Ring Ring Deals, measured and covered

None of its tests touch Fynkel, and its roadmap still lists a heat and frame
rate check as to do (`docs/ROADMAP.md:238`). `voiceChat`, which it depends on,
has no test in the engine.

**Bar:** the game's frame rate, heat and memory measured on each device, run
as it is, with **no change to its code** without the user's OK; engine tests
for what it depends on and has none, `voiceChat` first; what the numbers show
goes on the list for this stage or the next.

### Phase 4: results that can be trusted

- `a_views_frames_are_timed_on_the_gpu_when_asked` failed once on a Mac
  (#200: 3 frames timed, 4 passes without a clock). It passed 15 of 15 here
  and isn't in the 24 earlier failed runs checked.
- The soak test's "rest" memory read about 190 MB high on #197, likely the
  Mac's state, not proven.
- The Note10+'s GPU clock was held at half for one whole run, cause unknown.

**Bar:** each explained, or made so it can't give a false result. A test that
fails at random is fixed, not rerun until green.

### Phase 5: pause under a dialog

The rest of U2: a game that pauses itself while a dialog covers it can still
be unpaused wrongly, against `pauseWhenCovered`'s doc. The one audit bug left.

**Bar:** a failing test, then fixed. In the same phase, the audit rows already
fixed are marked done: BR2, AU-new, AU5, RN1, BR5, RN3, AU7, BR7, PH6, PH7,
DF13, WO10, RN8, BR10, DC5, and AU6 and AU8 (checked on the phones,
dbdd176c). BR6 and RN9 stay half done (the rest is in
`stage-16-report.md`'s Left open). RN5 is checked first: its code looks
fixed, but `stage-16-report.md` still lists Android's full path (the
playtest audit, 2026-11-16).

## 3. Deeper

### Phase 6: animation

Both main games rely on it and few tests cover it: about 3 test files name
`Animator`. Tests first. Then the gaps, ordered by what a game needs: root
motion, 2D blends, face shapes. Ring Ring Deals times lip sync with its own
`Stopwatch` and loudness envelope (`app/lib/game/office.dart:177`).

**Bar:** what the games use from `Animator` under test, and each gap taken on
judged beside Unity's. Scoped when this phase starts.

### Phase 7: sound

What games do by hand: fading a source, knowing a one-shot ended, pausing
music, automatic ducking (`app/lib/game/sounds.dart:76` ducks by setting one
source's volume).

**Bar:** each with a failing test first, and Ring Ring Deals' `hush` could be
written with what's added.

### Phase 8: loading and saving

Three load errors are silently swallowed in Ring Ring Deals
(`office_room.dart:139`, `app/lib/game/office.dart:88`, `sounds.dart:49`), and
there's no load progress. There's no safe way to save: the game's team file
is written so that a crash could damage it, though it's only a cache
(`app/lib/main.dart:183`).

**Bar:** an unhandled load error can't go unseen; load progress; a save that a
crash can't damage, tested by interrupting it.

### Phase 9: the speed left over

From [stage 16's report](stage-16-report.md): the iPad behind Unity at every
level and the Note10+ 36% behind at high; on the iPad at medium and high, the
CPU and GPU waiting on each other is the first thing to look at. Also the
Note10+ crowd, the pile's step, and Sheepdog at high and 120 on the iPhone,
from cold.

**Bar:** the gap smaller or explained, re-measured on every GPU before it's
merged.

## 4. Left out

- **Path finding.** People in Ring Ring Deals get stuck on furniture, and
  three timers teleport or remove them (`office_room.dart:185`, `:218`, `:249`).
  It's a new system and the game hasn't asked for it. It comes when it does.
- **What no game asks for:** an editor, lightmaps, LOD, gamepads.

## 5. Decisions

Agreed 2026-11-04:

1. **The order** above, with outlines first.
2. **Ring Ring Deals runs on the devices** for phase 3, measured only, with no
   change to its code.
3. **Path finding stays out** of this stage.

## 6. How it's done

As in stage 16: a failing test first; a commit for each crate and package, and
docs separately; a change tuned on one GPU measured on the others before it's
merged; Note10+ runs log the GPU clock and the foreground app; one build at a
time. A change to the look is shown and agreed before it's blessed.
