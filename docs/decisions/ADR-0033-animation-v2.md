# ADR-0033: Animation v2 — blend trees, one-shot slots, typed parameters, sub-state machines, masks and IK

**Status:** Accepted — proposed by the assistant 2026-09-26 after the user's question ("should we have gone with a more complex system like Unreal's? … follow how Unity works but make it much better, not copy its limits") and "yes please let's go"; confirmed by the user 2026-09-26 ("ok cool let's go")  
**Date:** 2026-09-26  
**Related:** ADR-0032 (the Animator Controller; its § 9 lists what this ADR takes up), ADR-0026 (code-first; `ClipBuilder` still waits), ADR-0030 (Inspector previews), ADR-0016 (codegen), ADR-0029 (the optional animation and physics modules), `docs/animation.md`, `docs/animator.md`, `docs/roadmap.md` Phase 5

## Context

ADR-0032 gave Fynkel Unity's Animator Controller: parameters, layers,
states that play a clip or a 1D blend space, transitions on conditions,
the Animator window, the live view in Play. The reference game's Player
and the showcase's Runner run on it. The user checked it in the editor
and in the game on 2026-09-26: it works.

The user then asked the right question: Unity's animation system is
widely thought to be limited — should we have copied Unreal's instead?

**What Unreal has that Unity lacks.** Unreal's power is not its state
machine (about the same as Unity's). It is the **pose graph** (the
Animation Blueprint's AnimGraph) the state machine sits in: poses flow
through nodes — blend, blend per bone, IK, a **slot** where a
**Montage** (a one-shot from code) plays over whatever the machine
does — plus sync groups, notifies, curves, Control Rig and Motion
Matching. Unreal's cost: every character needs a graph and a blueprint,
it is hard to learn, and it is a common complaint too.

**What people complain about in Unity**, and where Fynkel stands after
ADR-0032:

| Pain | Unity | Fynkel today |
| --- | --- | --- |
| Every one-shot (attack, hit, emote) needs a state and arrows from Any State: "transition spaghetti" | yes | **same** — under a controller, `animation.play(clip)` is refused |
| Blend trees buried in a nested graph; 2D types hard to set up | partly | **worse** — 1D only, a separate `.blend.json`, no editor for it |
| Parameters and states are strings; a typo fails silently | yes | better (a wrong name throws, listing the names) but still strings |
| Interruptions: five cryptic options | yes | none at all (ADR-0032 § 3) |
| Big graphs unreadable | sub-state machines, whose Exit rules confuse | no sub-state machines |
| Per-bone masks | Avatar Mask asset | one `root` node per layer |
| IK | Humanoid-only `OnAnimatorIK`, or the Animation Rigging package | none |
| Retargeting | Humanoid avatars | bone names must match |

**What we have underneath** is the right base for more: one clip
sampler (`fynkel_animation`), pose blend and add-delta, a pure state
machine core (`fynkel_animation::controller`) with no world in it,
layers with a mask by root node, markers, root motion, and a controller
that already evaluates, per tick, *layer → state machine → pose → blend
over the layers below*. That is a pose graph of a fixed shape.

## Decision

**Unity's workflow, with Unity's known pains fixed — taking from Unreal
only where it fixes one.** A person still works in Unity's terms: a
controller, the Animator window, states, transitions, parameters,
layers, the Inspector. We do **not** add a free-form node graph (the
AnimGraph) in this ADR. Instead, each layer is evaluated as a fixed
chain — Unreal's useful nodes, placed where they are always needed:

```text
per layer:  state machine (states play clips, blend spaces or blend trees)
            → slot (one-shots from code over the machine)
            → mask (the bones this layer poses)
            → blend over the layers below (override / additive, weight)
after all layers:  IK (two-bone, look-at, foot placement)
            → root motion, skinning
```

If a real game later needs a node the chain does not have, a free graph
can be opened then (§ 7) — the runtime already works this way inside.

### 1. Blend trees inside states (step 1)

A state's `motion` can be a **blend tree**, written in the controller:

```json
{
  "name": "Strafe",
  "motion": {
    "blendTree": {
      "type": "freeformDirectional",
      "parameters": ["moveX", "moveY"],
      "children": [
        { "motion": { "asset": "<hero>", "sub": "anim/idle" },       "position": [0, 0] },
        { "motion": { "asset": "<hero>", "sub": "anim/walk" },       "position": [0, 2] },
        { "motion": { "asset": "<hero>", "sub": "anim/walk_back" },  "position": [0, -2] },
        { "motion": { "asset": "<hero>", "sub": "anim/walk_left" },  "position": [-2, 0] },
        { "motion": { "asset": "<hero>", "sub": "anim/walk_right" }, "position": [2, 0] }
      ]
    }
  }
}
```

- **Types**, Unity's five:
  - `1d` — children at a `threshold` along one parameter (what
    `.blend.json` does today);
  - `simpleDirectional` — 2D, one child per direction, plus one at the
    centre (strafing at one speed);
  - `freeformDirectional` — 2D, several children per direction
    (walk and run in every direction: the usual choice);
  - `freeformCartesian` — 2D, where the two values are not a direction
    (turn rate × speed);
  - `direct` — each child's weight is its own float parameter (faces,
    breathing).
- **The weights** follow Unity's published methods, so a tree made for
  Unity behaves the same: linear between neighbours in 1D, barycentric
  in `simpleDirectional`, gradient-band interpolation (the polar kind
  for `freeformDirectional`, the plain kind for `freeformCartesian`) in
  the freeform types. Children with a weight below 0.001 are not
  sampled.
- **One clock, like Unity.** All children share a normalized time.
  The tree's length is the weighted average of theirs. So feet stay in
  step between walk and run, as the 1D space does now.
- **A child's motion** is a clip, a `.blend.json` or another tree
  (nesting up to 4 deep). Each child also has `timeScale` (default 1).
- **Parameters** are the controller's float parameters. The check at
  cook time: an unknown parameter or a non-float one, a 2D tree without
  two parameters, two children at the same position, no children
  (`FYN-D1104`…).
- **`.blend.json` stays.** It works as a state's motion and as a child.
  The editor can turn one into a tree in a state ("Copy into State").
  Nothing breaks.
- **Limits**, for the cost: at most 32 children per tree and 64 clips
  sampled per animator per tick. More is a cook error, not a slow
  frame.

**The Inspector** edits a tree the way the user's screenshot of Unity
shows:
- the type and the parameters at the top;
- for 2D, a **map**: a diamond per child, the weights shown as colour
  around each one, and a red dot for the current values. Drag a diamond
  to move its child; drag the red dot to try values — in Play it
  follows the game;
- the list of children: the motion, the position (or threshold), the
  time scale, a drag handle, + and −;
- **Compute Positions**: set each child's position from its clip's root
  travel (speed, or velocity X/Z; Unity's button of the same name);
- the **preview** (ADR-0030): the model posed by the blend at the red
  dot, playing.

In the Animator window a state playing a tree shows a small tree icon.
A double-click on it selects it in the Inspector.

### 2. One-shots over the machine: slots (step 2)

Unreal's Montage slots, made simple. This removes most Any-State arrows.

- **Every layer has a slot**, named after the layer. A **one-shot**
  played into a slot covers that layer's state machine: it blends in,
  plays once, blends out. The machine keeps running underneath, so it
  is in the right state when the one-shot ends.
- **A layer can have no states** (only a slot, and a mask). It shows
  nothing until a one-shot plays in it. That is the "upper body" layer:
  root `spine`, no states; an attack played there moves the arms while
  the legs keep walking.
- **Dart:**

  ```dart
  animation.playOneShot(attackClip, slot: 'UpperBody', blendIn: 0.1, blendOut: 0.2);
  animation.stopOneShot(slot: 'UpperBody', blendOut: 0.2);
  final busy = animation.isPlayingOneShot(slot: 'UpperBody');
  ```

  `slot` defaults to the base layer. A one-shot takes a clip, a blend
  space or a tree's asset. Its markers arrive as usual, and
  `onAnimationEnded` fires at its end. Playing another one-shot in the
  same slot blends from the first.
- **Code clips** (ADR-0026's `ClipBuilder`, waiting for the Animator)
  are played this way. That closes the last item of the code-first
  pass.
- The old rule stays: `play(clip)` and `crossfade(clip)` still refuse
  an animator with a controller. A one-shot is the way to play a clip
  over a controller.

### 3. Typed parameters and states from codegen (step 3)

`fynkel codegen` (and the editor, when a controller changes — like
`assets.g.dart`) writes `lib/generated/animators.g.dart`. For each
controller there is a class named after the file:

```dart
final anim = PlayerAnimator(animation);   // player.controller.json
anim.speed = planarSpeed;                 // setFloat('speed', …)
anim.grounded = me.grounded;              // setBool
anim.jump();                              // setTrigger('jump')
if (anim.state == PlayerState.jump) { … } // the base layer's state, an enum
anim.playOneShot(Assets.models.hero.anims.wave, slot: PlayerSlot.upperBody);
```

A renamed parameter or state is a compile error in the game, not a
throw at run time. `codegen --check` fails when the file is stale. The
string API stays for code that does not know its controller.

### 4. Sub-state machines and interruptions (step 4)

- **A sub-state machine** is a state with its own `states`,
  `transitions` and `defaultState` instead of a `motion`. It is drawn
  as one box. Double-click opens it; a path bar ("Base Layer ▸
  Airborne") goes back.
  - A transition **into** it enters its default state.
  - A transition **from** it is checked from every state inside it —
    a scoped Any State. This replaces Unity's Exit node and its
    confusing rules.
  - Names inside are paths for code: `play('Airborne/Fall')`.
    `stateOf().name` gives the path.
- **Interruptions**, one clear field per transition:
  `interruptibleBy`: `none` (the default, today's rule), `next` (the
  transitions of the state it goes to), `any` (Any State's and the
  next state's). An interruption blends from the pose of that moment,
  frozen, so nothing pops. The Inspector says it in words: "Can be
  interrupted by: the next state's transitions".

### 5. Masks per bone, and IK (step 5)

- **Masks.** A layer's `mask` names nodes to include or exclude, whole
  subtrees or single nodes:
  `"mask": [{ "node": "spine", "include": true }, { "node": "head", "include": false, "subtree": false }]`.
  `root` stays as the short form of a one-entry mask. The editor shows
  the character's node tree with checkboxes, like Unity's Avatar Mask
  — but inside the layer, not as a separate asset. (A shared mask
  asset can come later if several controllers need the same one.)
- **IK**, after all layers, native, per animator:
  - `twoBone` (an arm or a leg reaches a target: root, middle and tip
    nodes, a pole);
  - `lookAt` (the head, and the spine a little, turn to a point, with
    limits);
  - `footPlacement` (a ray down from each foot; the hips go down and
    the feet land on slopes and steps). It needs the physics module.
    Without it, it is skipped with one diagnostic.

  Each IK entry has a `weight`, which can be a float parameter.
  Targets come from Dart
  (`animation.setIKTarget('leftHand', position, weight: 1)`) or name
  an object. Written in the controller under `ik`, edited in the
  Animator window's IK tab.

### 6. The Animator window and the Inspector grow with each step

Blend trees in the Inspector (§ 1), a slot row per layer showing the
running one-shot in Play (§ 2), the path bar (§ 4), the mask tree and
the IK tab (§ 5). The live view shows all of it during Play, as it does
for states now.

### 7. Not in this ADR (and where it goes)

- **A free-form pose graph** (Unreal's AnimGraph). Only if a game
  needs a node the fixed chain lacks. It would be its own ADR, built on
  the same runtime.
- **Motion Matching** (Unreal 5's Pose Search): picking the next
  pose from a database of motion instead of states. It is the real step
  past both engines, and worth an ADR once the basics are solid and
  there is motion-capture content to test with.
- **Humanoid retargeting** (one clip on many skeletons). Needs a
  humanoid definition and content for several rigs. Its own ADR.
- **Montage sections**, **sync groups across layers**, **curves**,
  **mirror**, **state behaviours**, **a timeline/sequencer**, **morph
  targets**.

## Order

Each step is committed and
tested on its own; shared crates get their own commits. Each step
changes the wire (new `Animate` ops, new document fields), so the
fingerprint moves once per step.

1. **Blend trees.** The format and its checks, the weights in
   `fynkel_animation` (tested against Unity's methods), the runtime and
   cook, the Inspector map and list, Compute Positions, the preview.
   **Content:** strafe clips (walk and run forward, back, left, right)
   generated by `tools/character.py`. The reference game gets an **aim
   mode** — hold the right mouse button: the Player faces where the
   camera looks (`turnSpeed` 0, the behaviour turns it) and strafes on
   a `freeformDirectional` tree driven by `moveX`/`moveY`. The user's
   local Mixamo hero can take Mixamo's strafe clips (never committed).
2. **Slots.** Empty layers, `playOneShot`/`stopOneShot`, the events, the
   slot row in the window. The reference game's wave moves from a
   second Animator on the spine to a one-shot in an `UpperBody` layer.
   The showcase gets a code clip played as a one-shot.
3. **Typed parameters.** `animators.g.dart`, `codegen --check`, the
   editor writing it. The reference game's Player uses
   `PlayerAnimator`.
4. **Sub-state machines and interruptions.** The format, the runtime,
   the path bar. The reference game's jump becomes an `Airborne`
   machine (Jump → Fall → Land) with a landing that a new jump may
   interrupt.
5. **Masks and IK.** The mask field and tree, the three IK kinds, the
   IK tab. The reference game's Player gets foot placement on the
   Playground's steps and a look-at to the nearest coin.
6. **Docs:** `docs/animator.md` gains a section per step as it lands;
   `docs/animation.md` the engine's side. Then the user checks each
   step in the editor.

The user may reorder steps 2–5; step 1 comes first because ADR-0032
already named 2D blend trees as the next animation item.

## Consequences

- **Good.** The pains in the table are addressed where people meet
  them: one-shots need no arrows, 2D locomotion is set up on a map,
  typos are compile errors, big graphs fold into sub-machines,
  interruptions are one readable field, an upper-body attack and feet
  on steps need no add-on. The workflow stays Unity's, so a Unity user
  is at home.
- **Good.** No second runtime: blend trees, slots, masks and IK are
  stages of the controller evaluation that exists. The fixed chain
  keeps the per-tick cost predictable and inside the limits in § 1.
- **Bad.** The fixed chain is less flexible than Unreal's graph. A game
  that needs, say, two state machines blended by a curve cannot say so
  yet (§ 7).
- **Bad.** Five steps, each touching the format, the runtime, Dart and
  the editor. This is several days of work, and the content for tests
  (strafe clips) must be made first.
- **Risk.** Matching Unity's blend weights exactly. The gradient-band
  method is published (Johansen, 2009) and Unity documents which kind
  each type uses; step 1 tests the weights at known points against
  hand-worked values.
- **Risk.** Foot placement depends on physics raycasts inside the
  animation stage. It runs after physics in the tick, reads the
  physics world, and is skipped when the physics module is off.
