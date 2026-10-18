# Stage 9 report: animation

Phase 5 of [the stage 9 plan](stage-9.md). Phases 1 to 4 are merged
(#120–#122, #124, with the lab's asset fix #123 between). Phase 5 is
this PR.

## The result

Stage 9's goal was a character that walks, runs and waves, and blends
from one to the next, from a few lines of Dart. Both kinds of animated
model had to work: models whose parts move whole, and skinned models that
bend with a skeleton. A sword had to go in a hand, the upper body and the
legs had to play different clips, and many characters had to hold the
frame rate on all three devices.

**That is done.** The Kenney characters (parts that move) and KayKit's
Knight and Rogue (skinned) play their clips in the garden, coins and the
lab. Sheepdog's dog and sheep are skinned animals now. 100 characters
hold 60 fps on the Mac and the iPhone, and 120 when asked. The Note10+
holds 60 at low with 30. The one gap: the coins player doesn't jump,
because coins has nothing to jump over. Its clips walk, sprint, pick up
and cheer.

What a game writes, all of it in Dart:

```dart
final farmer = add(await assets.instantiate('character.glb'));
final animator = farmer.animator!;
animator.play('idle');
// in onUpdate: idle, walk and sprint mixed by speed, the feet keeping pace
animator.blend('move', const ['idle', 'walk', 'sprint'], thresholds: const [0, 2, 5], at: speed);
// a wave from the torso up, the legs walking on
animator.layer(1, mask: 'torso').crossFade('emote-yes', loop: false);
animator.onEnd = (_) => animator.layer(1).stop(duration: 0.15);
farmer.find('arm-right')!.add(sword); // a part is an object: the sword follows the hand
```

## What stage 9 did

| Phase | What | PR |
|---|---|---|
| 1 | Clips: the loader reads glTF animations; `Animator` with `play`, `crossFade`, `stop`, `onEnd`, events at a time, `speed`; the clips sampled in Rust once a frame; the garden's character walks where you tap | #120 |
| 2 | Skinned models: skins up to 256 joints, bent on the GPU (shadows too), culled from the joints; KayKit's Knight and Rogue in the lab | #121 |
| 3 | Blending: layers (masked, additive, with a strength), a 1D blend, crossfades from whatever is shown | #122 |
| — | The lab app bundles the KayKit models | #123 |
| 4 | The lab's crowd (1 to 100 characters), measured on the three devices | #124 |
| 5 | Coins and Sheepdog animated; the guide's §13 "Animation"; this report | this PR |

The C interface went from ABI 30 to 32. Each phase has its lab scene,
its part of the guide, and its tests: poses checked against numbers
worked out by hand, golden images of the Knight at every quality level,
and allocation tests showing that steady frames make no garbage.

## Phase 5

- **The coins player** has its own clips (#124's branch, merged here). It
  idles, walks and sprints in a blend by its speed. It holds the item it
  carries in its right hand on layer 1, masked to that arm, over the
  walk's swing. It picks coins up, and cheers at the last one, on layer 2
  from the torso up, while the legs walk on.
- **Sheepdog's dog and sheep** (decision 4: your pick, option 1). They
  come from Quaternius's
  [Farm Animals](https://opengameart.org/content/farm-animals-pack) pack,
  which is CC0. The dog is its pug and the sheep its sheep, converted from
  the pack's `.blend` files to glTF in Blender and otherwise unchanged
  (`examples/sheepdog/assets/CREDITS.md`). Each is about 270 KB, with 24
  joints and six clips.
  - Each animal's legs are a blend of idle, slow walk, walk and run by
    its speed.
  - A sheep hops (its jump clip) into the pen, and falls (its death clip)
    as a wolf takes it.
  - The pack's models are a few metres long, so the game shrinks them
    (0.2 for the sheep, 0.28 for the pug) and turns them round to face
    the way the game's objects do.
  - The wolves stay as shapes: the pack has no wolf.
  - A new test checks the dog stands idle, a sheep chased by the dog
    runs, and a sheep that comes home hops.
- **The guide's §13 "Animation"**: playing clips, skinned characters,
  blends and layers, *A character in a game* (a farmer that walks, runs
  and waves, eased and turned toward its goal), and *Many characters*
  (phase 4's numbers, what a character costs, and stopping far-off ones).
  It grew out of §12's "Animating a model". The old §13–21 are now
  §14–22, and the references to them across the repo are updated.
  §20's "isn't there yet" list has an Animation line: morph targets, 2D
  blends, root motion, IK, retargeting, and clips made in code
  (decision 5).

### Found on the way

- **A restarted round freed the models and loaded them again.** A new
  round in Sheepdog destroys the old one first, so the old round's sheep
  let go of the sheep model before the new round's sheep asked for it. It
  was read from the file again each round. The game itself now holds both
  models (`assets.preload`) for as long as it runs.
- **Widget tests and models.** In a Flutter widget test the clock is
  fake, so the real file reads behind `assets.instantiate` don't finish
  on their own. A test that waits for `loaded` inside `tester.runAsync`
  waits for ever, because an object only appears on a world frame.
  Sheepdog's app test alternates short real waits with frames until the
  game has loaded.
- **The emulator filled up** on #124's CI. `tools/test-mobile.sh` now
  deletes each test binary after it runs (in #124).

## Measured

Phase 4's measure is the stage's: the table is in
[the plan](stage-9.md#phase-4-many-characters-and-the-measure) and in
§13 of the guide. The budget holds on all three devices. The iPhone's 80
fps at high is high quality's own cost, not the characters'. Each
character costs about 160 KB of the engine's memory, with meshes and
textures shared.

Sheepdog wasn't measured again on the devices. It has nine animals, and
the crowd shows 30 holding 60 fps even on the Note10+ at low. But
Sheepdog on the Note10+ was already short of 60 before this stage (stage
8's accepted gap). A run on each device is easy if you want the numbers.

## Tests

- Sheepdog: game 23 (one new), app 10.
- Coins: 6. The guide's code (`tools/check_guide.py`): 54 blocks.
- Phases 1–4's tests are unchanged and pass in their PRs' CI.

## Not in this stage (decision 5)

Morph targets, 2D blends, root motion, IK, retargeting between
skeletons, and clips made in code. They're in the guide's §20.

## Next

Stage 10, shaders: surfaces, movement and whole styles. A material's own
vertex code goes after the skinning, which phase 2 left room for.
