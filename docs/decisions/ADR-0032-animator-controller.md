# ADR-0032: The Animator Controller — states, transitions and parameters as one asset, run by the engine, set from Dart

**Status:** Accepted — proposed by the assistant 2026-09-26 after the user's "ok let's do that" (the recommendation: start Phase 5 with the Animator state machine); confirmed by the user 2026-09-26 ("ok let's go")  
**Date:** 2026-09-26  
**Related:** ADR-0008 (parameters are simulation-lane commands), ADR-0011 (native components for the many), ADR-0012 (wire records), ADR-0021 (Play in the editor), ADR-0026 (code-first; `ClipBuilder` waits for this), ADR-0029 (the optional animation module), ADR-0030 (Inspector previews, step 7), blueprint §9 Animation, `docs/animation.md` § Not in V1, `docs/roadmap.md` Phase 5

## Context

Phase 5 is "a real game", and the roadmap puts the **Animator state
machine** first. A game with a character needs one: idle, walk, run,
jump, land, attack, die, and the rules for moving between them.

**How Unity does it.**
- An **Animator Controller** is one asset (`.controller`). It holds:
  - **parameters**: Float, Int, Bool and Trigger, each with a default;
  - **layers**: a base layer, then more on top, each with a weight,
    Override or Additive, and an avatar mask;
  - in each layer a **state machine**: states, one default state (the
    orange one), and **Any State**.
- A **state** plays a **motion**: a clip or a **blend tree**. It has a
  speed, optionally multiplied by a parameter.
- A **transition** goes from one state (or Any State) to another. It
  has **conditions** on parameters (all must hold), an optional **exit
  time**, a **duration** (the crossfade) and an order. A Trigger is
  used up by the transition it fires.
- The **Animator** component names the controller. Code only sets
  parameters (`SetFloat`, `SetBool`, `SetTrigger`) and reads the state
  (`GetCurrentAnimatorStateInfo`); `Play`/`CrossFade` by state name
  jump straight to a state.
- The **Animator window** draws the graph: boxes for states, arrows for
  transitions, the parameter list at the side. The Inspector edits the
  selected state or transition. In Play the window shows the running
  state with a progress bar, and the parameters can be changed live.

Godot's `AnimationTree` with a `StateMachine` root and Unreal's
Animation Blueprint state machines follow the same idea: states that
play motions, transitions on conditions, parameters set by gameplay
code.

**What Fynkel has today** (mapped 2026-09-26, `docs/animation.md`):
- **Clips** (`anim/<name>` sub-assets from glTF and FBX, markers
  included), sampled natively in `fynkel_animation`.
- **The `fynkel.Animator` component** (id 21): `clip`, `speed`,
  `looping`, `playing`, `weight`, `additive`, `root_motion`. The clip
  can be a clip or a **1D blend space** (`.blend.json`: points along one
  float parameter, phase-synced).
- **Commands** (`fynkel.cmd.Animate`): Play, Crossfade (one fade at a
  time), Stop, Pause, Resume, SetSpeed, SetTime, SetWeight,
  SetParameter. Parameters are **floats only**.
- **Layers** as a *child entity's* Animator ("mask by placement"),
  additive or override by `weight`. **Markers** as events, **root
  motion** to the `CharacterController`.
- **Dart**: `GameObject.animation` (`play`, `crossfade`,
  `setParameter`, `state` …), `onAnimationEnded`, `onAnimationMarker`.
- **Nothing** decides *which* clip to play. The reference game's Player
  does it by hand in Dart: blend on `speed`, `play(jump)` on a jump,
  `crossfade(locomotion)` on landing (`player.dart` `pose()`).
- **The editor** has no animation UI: the Animation tab is a
  placeholder.

`docs/animation.md` already names the next step: "the state machine as
data (states = clips or blends, transitions on named parameters, Dart
sets parameters only)". This ADR makes it concrete.

## Decision

### 1. One asset: `<name>.controller.json`

A new document kind, **Animator Controller** (`DocumentKind::Controller`,
`schemas/controller.schema.json`), in the same place any asset can be.
Like Unity's, it holds everything about one character's animation
logic:

```json
{
  "$schema": "https://fynkel.dev/schemas/controller.schema.json",
  "formatVersion": 1,
  "id": "…",
  "name": "Player",
  "parameters": [
    { "name": "speed",    "type": "float",   "default": 0 },
    { "name": "grounded", "type": "bool",    "default": true },
    { "name": "jump",     "type": "trigger" }
  ],
  "layers": [
    {
      "name": "Base Layer",
      "defaultState": "Locomotion",
      "states": [
        { "name": "Locomotion", "motion": { "asset": "<player.blend.json>" } },
        { "name": "Jump", "motion": { "asset": "<hero>", "sub": "anim/jump" }, "loop": false }
      ],
      "transitions": [
        { "from": "Locomotion", "to": "Jump", "duration": 0.1,
          "conditions": [ { "parameter": "jump" } ] },
        { "from": "Jump", "to": "Locomotion", "exitTime": 0.3, "duration": 0.2,
          "conditions": [ { "parameter": "grounded", "is": true } ] }
      ]
    }
  ],
  "editor": { "Base Layer": { "Locomotion": [0, 0], "Jump": [260, 0] } }
}
```

- **Names, not ids, inside the file.** States and parameters are named
  (unique per layer and per controller). Transitions name them. This is
  what a person reads and what Dart uses (`play('Jump')`). A rename in
  the editor rewrites the references in the same transaction.
- **`editor`** holds only where each box sits in the graph. The runtime
  never reads it.
- **Cooked** as a document chunk that depends on the motions it names,
  like a blend space. Checked by the cook: unknown state or parameter,
  a condition that does not fit its parameter's type, a missing
  motion, two states with one name, no default state
  (`FYN-D1100`…).

### 2. States and motions

A state has:
- **`motion`**: a clip (`anim/<name>` of a model) or a **blend space**
  (the existing `.blend.json`, so the 1D locomotion blend works as a
  state unchanged). Empty is allowed: the state holds the pose
  (Unity's empty state).
- **`speed`** (default 1) and an optional **`speedParameter`**: a float
  parameter multiplied in (a walk that slows with a slope).
- **`loop`**: whether the motion loops. By default it loops.
- **`rootMotion`**: whether its root motion goes to the character
  (default: the Animator's `root_motion`).

**Any State** is written `"from": "any"`. Its transitions are checked
first, every tick, from whatever state is playing. They never fire into
the state already playing, unless the transition says
`"canTransitionToSelf": true`.

**Not in this ADR:** sub-state machines, and 2D blend trees. See § 9.

### 3. Transitions and conditions

A transition has:
- **`from`** (a state or `any`) and **`to`** (a state);
- **`conditions`**: all must hold. For a:
  - float: `{ "parameter": "speed", "greater": 0.1 }` or `"less"`;
  - int: `"equals"`, `"notEquals"`, `"greater"` or `"less"`;
  - bool: `{ "parameter": "grounded", "is": true }`;
  - trigger: `{ "parameter": "jump" }`, which holds while the trigger
    is set.
- **`exitTime`** (optional): the transition may only fire once the
  state has played that fraction of its motion (0.9 = 90 %). Without
  conditions it fires then (Unity's "Has Exit Time").
- **`duration`**: the crossfade, in seconds (default 0.25).
- **`offset`**: where the next state starts, as a fraction of its motion
  (default 0).

The rules, the same each tick:
1. Only one transition fires per layer per tick. Any State's come
   first, then the current state's, each in file order. The Animator
   window shows and changes that order.
2. A fired transition **uses up** every trigger in its conditions.
   A trigger nothing uses stays set until something does, or until
   code calls `resetTrigger` (Unity's behaviour).
3. **No interruptions** in V1. While a transition's crossfade runs, no
   other transition of that layer fires. When the crossfade ends, the
   next tick checks again. This fits the engine's one-fade crossfade.
   Unity's interruption sources come later if a game needs them.

### 4. Parameters: four types

`float`, `int`, `bool` and `trigger`, each with a default (a trigger
starts unset). The Animator's parameters live on the running animator,
start at the controller's defaults, and belong to that animator only.
Two characters with the same controller have their own values.

This replaces the float-only parameter of today. `SetParameter` becomes
typed; a blend space still reads a float parameter by name, and it can
be any of the controller's floats.

### 5. The Animator component

`fynkel.Animator` (id 21) gains one field, **`controller`** (an asset
reference, None by default), and goes to version 2:

> **As built (step 3):** `controller` is an `Option<AssetRef>`. An
> optional reference is never a required schema field, so, as with
> ADR-0025's `Collider.material`, the documents need no new version and
> no migration: a missing `controller` reads as None.

- **With a controller**, the Animator runs the controller. Its `clip`
  is not used. `speed`, `playing`, `weight` and `root_motion` still
  apply to the whole Animator (Unity's `animator.speed`,
  `applyRootMotion`).
- **Without one**, it works exactly as today. Nothing already made
  changes, and the reading of every existing scene is unchanged (a
  missing `controller` is None).

**Layers.** A controller's layers after the first are applied over the
one below, as the engine's child-entity layers are today:
- `"weight"` (0–1) and `"blending": "override" | "additive"`;
- `"root"` (optional): the name of the node the layer poses from, such
  as `spine`. It poses only that node and those under it, which is the
  engine's "mask by placement" moved into the asset. This is how an
  upper-body attack plays over the legs' run.

The child-entity layers of today keep working next to this.
Per-bone avatar masks come later (§ 9).

### 6. The runtime: native, in the tick

The state machine runs in Rust, in `fynkel_animation` (the pure logic:
conditions, transition choice, times) and in `fynkel_core`'s
`run_animation` (applying it). It runs in the same place in the tick as
today: after the tick's commands, before physics.
- Parameters set by code are **simulation-lane commands** (ADR-0008),
  so a parameter set in `fixedUpdate` is seen by the same tick.
- The state machine is **deterministic**: the same parameters on the
  same ticks give the same states and the same pose, as the rest of the
  simulation does. A headless test can check a whole jump.
- It costs little: a few comparisons per animator per tick. The pose
  work is today's.

**Events.** Two new events, reported like markers:
- **state entered**: the layer, the state, and the state left;
- **state exited**.

Clip markers keep working inside a state. During a crossfade, the
dominant motion's markers fire, as they do in a blend space.

### 7. Dart: Unity's `Animator` API

On `GameObject.animation` (and `Behaviour.animation`):

```dart
animation.setFloat('speed', v);           // Unity: SetFloat
animation.setInteger('weapon', 2);
animation.setBool('grounded', true);
animation.setTrigger('jump');             // SetTrigger
animation.resetTrigger('jump');
animation.getFloat('speed');              // … getBool, getInteger

animation.play('Jump');                   // Play(stateName): at once
animation.crossFade('Jump', 0.1);         // CrossFade(stateName, seconds)

final s = animation.stateOf(layer: 0);    // GetCurrentAnimatorStateInfo
s.name;                                   // 'Locomotion'
s.normalizedTime;                         // 1.5 = second loop, halfway
s.isInTransition;                         // IsInTransition
s.next;                                   // the state being faded to, or null
```

- A wrong parameter name or type throws an `ArgumentError`, with the
  names the controller has.

> **As built (step 4):** Dart has no overloads, so the existing
> `play` and `crossfade` (spelled as before, not `crossFade`) take a
> state name (`String`) or a clip (`AssetRef`). `stateOf` has a
> companion `layers` (every layer) and `parameters` (the values, for
> the editor's live view).
- **`onAnimatorStateEntered` / `onAnimatorStateExited`** on `Behaviour`
  receive the events of § 6 (Unity's `StateMachineBehaviour.OnStateEnter`,
  kept on the object's own scripts rather than on the state).
- Today's `play(clip)`, `crossfade(clip, …)` and `setParameter` stay,
  for Animators without a controller. On an Animator with one they
  throw, with a message pointing to the state-name versions: a
  controller owns what plays.
- **Code-first** (ADR-0026): code can *use* a controller (set it on
  the Animator of an object made in code), but not *make* one in this
  ADR. Unity cannot make one at runtime either. `ClipBuilder`
  (ADR-0026 §8) comes after this and is untouched by it.

### 8. The editor: the Animator window

The **Animation** bottom tab (today a placeholder) becomes the
**Animator** window. It shows the controller of the selected object, or
the controller selected in the Project.

- **The graph.**
  - A box per state, the default state in orange, **Entry** and **Any
    State** boxes, and arrows for transitions (one arrow per pair, with
    a count when there are several).
  - Drag boxes to arrange them. Right-click the empty graph for
    *Create State ▸ Empty / From Clip / From Blend Space*. Right-click a
    state for *Make Transition* (then click the target), *Set as
    Layer Default State*, *Rename* and *Delete*.
  - Drop a clip or a blend space from the Project on the graph to make
    a state that plays it.
  - A tab strip at the top for **layers**, with + to add one.
- **Parameters** list at the left: + (Float, Int, Bool, Trigger),
  rename, delete, the default value. A rename rewrites every condition
  and speed parameter that uses it.
- **The Inspector** shows the selected state (name, motion picker,
  speed, speed parameter, loop, root motion, the transitions out of it
  in their order) or the selected transition (conditions with + and −,
  exit time, duration, offset).
- **Every change is one undoable transaction on the controller,
  saved at once**, as a material's is (`AssetEdits`).
- **During Play** (ADR-0021): the running state is highlighted with a
  progress bar that fills, and the transition being taken is drawn
  active. The parameters show their live values and can be changed
  (Unity's way to test a transition). Changing the *controller* during
  Play is saved and reaches the running game from the next Play (a
  live graph change comes later if it is missed).
- **Project**: *Create ▸ Animator Controller*. On an imported model,
  *Create Animator Controller* makes one with a state per clip, the
  first as default. Dropping a controller on an object in the Hierarchy
  or the Inspector sets its Animator's `controller`, adding the
  Animator if needed.

### 9. Not in this ADR (and where it goes)

- **2D blend trees** (Simple Directional, Freeform), for strafing: the
  next animation item after this one, as a second kind of blend space
  a state plays.
- **Sub-state machines**, **interruption sources**, **transition
  offsets by parameter**, **mirror**, **cycle offset**.
- **Avatar masks** (a list of bones per layer) and humanoid
  retargeting.
- **Behaviours on states** (Unity's `StateMachineBehaviour` as a
  script attached to a state). § 7's callbacks cover the need for now.
- **A timeline/sequencer**, **IK**, **morph targets**.
- **Making or changing a controller from code.**
- **A live graph change during Play** (§ 8).

## Order

Each step is committed and tested on its own; shared crates get their
own commits.

1. **The document**: `DocumentKind::Controller`, the schema, reading,
   validation (`FYN-D1100`…), the cook (a chunk that depends on its
   motions), `fynkel asset init` and `fynkel validate` for it.
2. **The logic** (`fynkel_animation`): parameters, conditions, the
   choice of transition, times. Pure, and unit-tested against Unity's
   rules (§ 3).
3. **The runtime** (`fynkel_core`): `Animator.controller` (version 2),
   typed parameters on the wire, `Play`/`CrossFade` by state, the
   state-entered and state-exited events, the Animator query with the
   state. Headless tests: a whole jump, deterministic.
4. **Dart** (`package:fynkel`): § 7's API, the callbacks and tests over
   the native library.
5. **The editor, the graph**: the Animator window, the parameters, the
   Inspector for states and transitions, Create ▸ Animator Controller,
   and the drop onto an object.
6. **The editor in Play**: the live state, the progress, the live
   parameters.
7. **Acceptance**: the reference game's Player runs on a controller
   (Locomotion ↔ Jump, back on `grounded`), and its hand-written jump and land code in
   `player.dart` goes. The showcase's runner gets one too. Docs:
   `docs/animation.md` § The Animator Controller and a short guide page
   (`docs/animator.md`). Then the user checks it in the editor.

## Consequences

- **Good.** Animation logic becomes data, which a person arranges in a
  window rather than writing in `update()`. That is Unity's way, and
  the way the user expects. Games get Idle/Run/Jump/Attack without code
  beyond `setFloat`/`setTrigger`. The engine keeps the work native and
  deterministic, so thousands of animators stay cheap. Nothing that
  already exists changes its behaviour.
- **Cost.** A new document kind, a component version, wire and
  fingerprint changes (a new ABI fingerprint, as every wire change
  brings). The editor window is the largest piece.
- **Risk.** Unity's transition rules have many corners (interruptions,
  exit time and conditions together, triggers left set). V1 takes the
  common, predictable subset of § 3 and says so; the rest waits for a
  game to need it.
