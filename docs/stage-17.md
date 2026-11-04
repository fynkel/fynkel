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

### Phase 2: running out of memory

- **No low-memory response.** Nothing in `packages` or `crates` hears the
  system's warning, so a phone under pressure can simply kill the game.
- **Textures aren't compressed.** The render crate has no ASTC, ETC2 or BC
  formats; Unity compresses textures on phones. Fynkel 1 made textures 4–5
  times smaller this way (ADR-0061), so this is likely the biggest saving,
  though it hasn't been measured here. It wants its own ADR: how it fits the
  way assets load now, which formats on which GPU.
- **Not a leak, but a lot.** Followed for 30 minutes on 2026-11-04, the iPad
  at high goes 129 → 425 MB in the first 7.5 minutes and then only to 431, and
  the Note10+ settles at 213. So this phase is about the size, not a leak:
  what fills those 300 MB on the iPad isn't known, and finding out comes first.

**Bar:** the warning reaches the game and the engine frees what it can make
again, tested; a texture-heavy scene measured before and after on each
device, with frame rate no worse; compressed textures shown to the user and
agreed, since they change the look.

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
DF13, WO10, RN8, BR10, DC5.

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
