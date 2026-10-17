# Stage 9 plan: animation

Status: accepted 2026-10-17 ("let's go"), with the recommended answers to §4. Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 8 is done
([report](stage-8-report.md)), and both phones draw straight to the
screen ([Android](android-surface-report.md), [iPhone](ios-surface-report.md)).

A Fynkel game can have characters, but they stand still: a model shows
its rest pose, and the loader says so ("animations are skipped"). The
Kenney character in the garden and coins has 27 clips in its file (idle,
walk, sprint, pick-up…), unused.

**The goal.** A character walks, runs, jumps and waves, and blends from
one to the next, from a few lines of Dart:

```dart
final hero = add(await assets.instantiate('hero.glb'));
final animator = hero.animator!;           // the model's clips
animator.play('idle');
// later, in onUpdate:
animator.crossFade(speed > 0.1 ? 'walk' : 'idle', duration: 0.2);
```

Both kinds of animated model work: models whose parts move whole (the
Kenney characters: "node" animation), and skinned models, whose mesh
bends with a skeleton (most characters, Mixamo's and Quaternius's). A
sword goes in a hand with `hero.find('hand.R')!.add(sword)`. Upper body
and legs play different clips. Each is in the guide and tested, and many
characters hold the frame rate on all three devices.

**The road after it** (agreed 2026-10-17): stage 10 shaders (surfaces,
movement, whole styles), stage 11 sound, stage 12 shipping. Then 1.0.
This stage leaves room for stage 10: a shader's own vertex code runs
after skinning, on the bent vertices.

## 1. What there is today

- **Models** (`crates/fynkel_load`): glTF read into shared meshes,
  materials and textures, and a node tree. `spawn_model` makes every node
  an object, in Rust, under the model's root, and Dart wraps each in a
  `GameObject` as the copy is added (`_readParts`). (This plan first said
  Dart wrapped them only when asked; it doesn't. So that a skeleton's 65
  joints cost Dart little each frame, a frame's clips make Dart forget
  only the kept transforms of the parts they move, and Dart reads one
  again only if the game asks.)
- **Skipped with a warning**: animations, skins, morph targets, cameras.
- **The renderer** (`forward.wgsl`): per vertex, locations 0–3; per
  instance, 4–10. Locations 11–15 are free (the hardening plan freed them
  for this stage). Bind groups 0–3 are all in use.
- **Tweens** move any property from code (stage 4): they stay the way to
  animate things made in code. This stage is about clips that come in a
  model's file.

What Fynkel 1 taught (its ADRs 0032 and 0033, read again for this plan):

- **Sampling is cheap.** 200 rigs cost 0.3 ms a frame on the Mac and
  0.6 ms on the iPhone, in Rust. The cost and the bugs were around it:
  a state-machine file format, blend-tree assets, an editor, codegen,
  and the lifecycle of all that (an animator removed and added again
  mixing old and new state; a scan every frame that cost 8 ms).
- **The state machine as a file was a mistake for us.** It shipped with
  no interruptions and needed a second ADR the same day. In a code-first
  framework, the game's code is the state machine: `if (grounded) …`.
- **Joints as ordinary objects worked**: the clip writes their
  transforms, the renderer reads their world matrices, and attaching
  things to a hand is just a child.

## 2. How each phase is done

As in stage 8: one PR or a few per phase, each with its lab scene, its
guide section, its tests (golden images where a picture is the answer,
poses checked against numbers elsewhere), and plain words.

- **The frame budget.** 60 fps is the default (`targetFrameRate`); the
  Mac and the iPhone must hold it with 100 animated characters on screen
  and 120 when the game asks, the Note10+ 60 at low with 30.
- **No hidden cost.** A world with no animator pays nothing; a model
  with no skin draws as today.

## 3. Phases

### Phase 1: clips, and models whose parts move

- **The loader** reads a model's animations: each channel moves a node's
  position, rotation or scale, with glTF's step, linear and cubic keys.
  Clips are the model's, shared by every copy, like its meshes.
- **`Animator`**, a behaviour each animated model gets when it's
  instantiated (`hero.animator`), as Unity adds one:
  - `play(name)`, `crossFade(name, duration:)`, `stop()`;
  - `speed`, `loop` (per clip: a walk loops, a jump plays once), the
    clip's `time` and `length`, `isPlaying(name)`, `clips` (the names);
  - `onEnd` (a jump that ended) and events at a time in a clip
    (`at('walk', 0.5, footstep)`: for stage 11's footsteps).
- **In Rust**: once a world frame, before the render list, each animator
  samples its clips and writes the nodes' transforms. Dart only says what
  to play; nothing crosses the boundary per joint.
- The game's code sets a node's transform after the clip, in
  `onLateUpdate`, as in Unity (a head that turns to look at you).
- **Shown with** the Kenney character in the garden: idle, walk where
  you tap, pick-up.

*Done when* a clip's pose at given times matches numbers worked out by
hand (each key kind, looping, the end), a crossfade halfway is halfway,
and the garden's character walks.

### Phase 2: skinned models

- **The loader** reads skins (joints, inverse bind matrices) and each
  vertex's four joints and weights. Up to 256 joints a skin.
- **The renderer bends the mesh on the GPU.** Each frame the joints'
  matrices go into one buffer for all the skinned meshes drawn; each
  vertex mixes its four. The shadow pass bends it the same way, so the
  shadow walks too. Skinned meshes get their own pipelines; others keep
  theirs.
- **Culling** from the joints' positions, so an arm swung out isn't cut
  off at the screen's edge, and a character out of view isn't drawn.
- **Attaching**: joints are objects, so a sword is a child of the hand
  and follows it.
- **Stage 10's door**: the vertex stage is written as "skin, then the
  rest", so a material's own vertex code (wind, wobble) goes after the
  skin.
- **Shown with** a CC0 skinned character (decision 4) in the lab, and the
  Mixamo hero on this Mac only (never committed).

*Done when* golden images of a skinned character at given poses match,
lit and shadowed, at every quality level; the sword stays in the hand;
and the debug build warns about a skin over 256 joints.

### Phase 3: blending

- **Layers**: a second clip on part of the skeleton, from a joint down
  (`animator.layer(1, mask: 'spine')`: wave while walking), over the base
  or added to it.
- **A 1D blend** for moving at any speed: idle, walk and run mixed by one
  number (`animator.blend('move', ['idle', 'walk', 'run'], at: speed)`),
  each clip's pace matched so feet don't slide. Unity's blend tree,
  as code.
- **Crossfading from anything**: a crossfade starts from the pose shown,
  mid-blend or mid-fade, so a game can change its mind every frame.

*Done when* the poses of a layer, an additive layer and a blend at given
weights match numbers, and changing clips every frame never jumps.

### Phase 4: many characters, and the measure

- **The lab's crowd scene**: 1, 10, 30 and 100 skinned characters,
  each on its own clip and time.
- **Measured** on the Mac, the iPhone and the Note10+: the animation's
  CPU time a frame, the GPU's, fps, memory, at each quality level. If
  the budget isn't met, the fix comes in this phase (fewer joints
  updated for characters far away, sampling on a worker thread).

*Done when* the budget (§2) holds, or is accepted short with the cause
found, as stage 8 did for the Note10+.

### Phase 5: games, the guide, and the report

- **The coins player** walks, runs, jumps and picks coins up with its
  own clips.
- **Sheepdog's dog and sheep**, if CC0 animated ones are found
  (decision 4); otherwise Sheepdog stays as it is.
- **The guide**: a new section, "Animation", and §19 updated; the lab's
  scenes; `docs/stage-9-report.md`.

## 4. Decisions

Each has a recommendation. Accepting the plan accepts them, unless you
say otherwise.

1. **Animation runs in Rust; the game says what to play from Dart.**
   *Recommended*: sampling every joint of every character is the hot
   path, and Fynkel 1 measured it cheap there. Dart sees joints only
   when it asks.
2. **No state-machine file or blend-tree asset: the game's code decides
   which clip plays.** `play`, `crossFade`, layers and a 1D blend are the
   tools. *Recommended*: Fynkel 1's Animator Controller file needed an
   editor, codegen and a second ADR in a day; in Dart, the state machine
   is an `if` or a `switch`, readable and hot-reloaded. A small helper
   can come later if games keep writing the same one.
3. **Clips play in place; root motion isn't in this stage.** The game
   moves the character, and the walk's pace is matched to it.
   *Recommended*: most games do it this way, and it keeps physics in
   charge of where the character is. Mixamo has an "In Place" option
   for every clip.
4. **The characters**: a CC0 skinned character with idle, walk, run,
   jump and wave, committed with its credits, from Quaternius or KayKit
   (both CC0, with glTF files). I'll find and check one and show you
   before committing it; if you have one you like, say so. The Mixamo
   hero stays on this Mac.
5. **Not in this stage**: morph targets (faces, blend shapes), IK (feet
   on slopes, looking at things beyond a head turn), retargeting a clip
   from one skeleton to another (Unity's Humanoid), and clips made in
   code. *Recommended*: each is real work, and none is needed for a
   character to walk, run and jump. They're listed in the guide's §19 as
   not there yet.

## 5. Checklist

- [x] Phase 1: clips, and models whose parts move (PR pending)
- [ ] Phase 2: skinned models
- [ ] Phase 3: blending
- [ ] Phase 4: many characters, and the measure
- [ ] Phase 5: games, the guide, and the report
