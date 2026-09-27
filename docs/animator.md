# The Animator Controller

An **Animator Controller** decides which animation a character plays,
and when it changes to another one. It works like Unity's: a graph of
**states** (each plays a clip or a blend space), **transitions** between
them (arrows), and **parameters** (values your code sets, such as
`speed` or `jump`). Your code only sets parameters. The controller picks
the animation and blends from one to the next.

A controller is one file, `<name>.controller.json`. An object's
`Animator` names it in its **Controller** field. An Animator without a
controller plays its **Clip** as before.

The decisions behind this page are in
[ADR-0032](decisions/ADR-0032-animator-controller.md). The engine's rules
and the file format are in [animation.md](animation.md) § Animator
Controllers and [documents.md](documents.md) § Animator Controllers.

## Make one

- **From a model:** right-click a model in the Project and choose
  **Create Animator Controller**. You get a state for each of its clips;
  the first one is the default.
- **Empty:** Create ▸ Animator Controller in the Project.

Then drop the controller on an object in the Hierarchy. This gives the
object an Animator that uses it. You can also drag it onto the
Animator's **Controller** field in the Inspector.

## The Animator window

Select the object (or the controller) and open the **Animator** tab, next
to the Project and the Console.

- **The graph.** A box is a state. The orange box is the layer's default
  state: the one the layer starts in. **Entry** points at it. **Any
  State** stands for every state: its transitions can fire from anywhere.
- **Look around:** drag the empty graph, scroll or pinch to zoom, and
  press **F** to fit everything in view.
- **Right-click the empty graph** for Create State ▸ Empty, From Clip…,
  From Blend Space… or From New Blend Tree…. You can also drop a clip or
  a blend space from the Project onto the graph. A state that plays a
  blend tree shows a small tree icon.
- **Right-click a state** for Make Transition (then click the target
  state), Set as Layer Default State, Rename and Delete.
- **Right-click the empty graph ▸ Create Sub-State Machine** makes a
  box that holds a small graph of its own (see below). Double-click it
  to open it; the path bar above the graph ("Base Layer ▸ Airborne")
  takes you back.
- **Click** a state or an arrow to edit it in the Inspector. **⌫** deletes
  it. **⌘Z** undoes (while the window has the keyboard).
- **Parameters** are on the left: **+** adds a Float, Int, Bool or
  Trigger. A new name renames it everywhere it is used.
- **Layers** are the tabs along the top: **+** adds one (see below).

Every change is saved at once, like a material.

## States

In the Inspector, a state has:

- **Motion**: the clip or blend space it plays, or a blend tree (see
  below). Empty holds the pose. **Make Blend Tree** turns the motion into
  a tree that holds it.
- **Speed**, and **Multiplier**: a float parameter the speed is
  multiplied by.
- **Loop**: whether the motion repeats. A jump does not.
- **Root Motion**: whether the clip moves the character. Empty uses the
  Animator's setting.

## Blend trees

A **blend tree** mixes several clips by the values of parameters. For
example, a character that strafes: idle in the middle, walk and run in
each direction around it, picked by `moveX` and `moveY`. It works like
Unity's blend trees, but you edit it right in the Inspector. The
decisions are in [ADR-0033](decisions/ADR-0033-animation-v2.md).

Make one with **Create State ▸ From New Blend Tree…** (pick its first
clip), or **Make Blend Tree** on a state. A tree always holds at least
one motion.

In the Inspector, the **Blend Tree** section has:

- **Type**: how the children are mixed.
  - **1D**: one parameter; each child has a **threshold** on a line.
  - **2D Simple Directional**: two parameters; one child per direction
    (and maybe one in the middle).
  - **2D Freeform Directional**: two parameters; several children per
    direction, such as a walk and a run forward. Best for movement.
  - **2D Freeform Cartesian**: two parameters that are not a direction.
  - **Direct**: each child has its own parameter, which is its weight.
- **Parameter** (or X and Y): the float parameters it reads.
- **The map.** Each child is a diamond. The circle around it shows its
  weight now. The red dot is the parameters' values: click or drag it
  to try other values. In Play it moves with the game, and dragging it
  sets the running Animator's parameters. Drag a diamond to move a
  child.
- **The children.** Each row has the motion (a clip, a blend space, or
  a tree: click it to open it), the threshold or position, the speed,
  its weight now, and buttons to move or remove it. **+** adds a motion
  or a new tree inside this one. The path at the top takes you back out
  of a tree inside a tree.
- **Compute Positions** places the children from how far their clips
  move the character each second. For 1D: Speed, Velocity X, Y or Z,
  or Angular Speed. For 2D: Velocity XZ (right, forward) or Speed and
  Angular Speed.
- **The preview** plays the tree on the clips' model, at the red dot.

All the clips in a tree play in step: a walk and a run lift the same
foot at the same moment.

Limits: 32 children in a tree, trees 4 deep, 64 clips in all.

## Transitions

A transition fires when **all** its conditions hold. In the Inspector:

- **Conditions**: a parameter and a test. A float or int is compared
  (greater, less; an int also equals, not equals). A bool is tested
  against true or false. A trigger only needs to be set; the transition
  that uses it clears it.
- **Has Exit Time** and **Exit Time**: wait until this much of the state
  has played (0.9 is 90 %, 2 is two full loops). A transition with an
  exit time and no conditions fires when that time comes.
- **Duration**: the blend into the next state, in seconds.
- **Offset**: where the next state's motion starts (0 to 1).
- **To Self** (Any State only): may it restart the state that is
  already playing?

A state with several transitions checks them in order; the arrows in
the Inspector change the order. Only one transition fires per tick.

- **Interruption**: what may cut the transition's blend short. By
  default nothing can: the blend always ends first. **The next state's
  transitions** lets the state it goes to leave again at once (a new
  jump during a landing). **Any State's and the next state's** also
  lets Any State in. An interrupted blend fades on from the pose of that
  moment, so nothing pops. The Inspector says it in words: "Can be
  interrupted by: the next state's transitions". In the file:
  `"interruptibleBy": "next"` (or `"any"`, or `"none"`).

## Sub-state machines

A big graph gets hard to read. A **sub-state machine** is one box that
holds states and transitions of its own: an `Airborne` box with Jump,
Fall and Land inside, instead of three more boxes in the base graph.

- **Into it:** a transition to the box enters its **default state**
  (the orange one inside; the Inspector's **Enters** field for the box).
- **Out of it:** a transition *from the box* is checked from every
  state inside it, like an Any State that only covers the box. There is
  no Exit node to learn.
- **Inside it:** transitions join its own states. Any State belongs to
  the layer, so it is not shown inside.
- **Order:** Any State's transitions are checked first, then the
  transitions from the boxes a state is in (the outer one first), then
  the state's own.
- **From code**, a state inside a box is named by its path:
  `animation.play('Airborne/Fall')`. `play('Airborne')` enters the box's
  default state. `stateOf()!.name` gives the path.

```json
{ "name": "Airborne", "defaultState": "Jump",
  "states": [
    { "name": "Jump", "motion": { "asset": "…", "sub": "anim/jump" }, "loop": false },
    { "name": "Fall", "motion": { "asset": "…", "sub": "anim/jump" }, "speed": 0, "loop": false },
    { "name": "Land", "motion": { "asset": "…", "sub": "anim/idle" }, "loop": false }
  ],
  "transitions": [
    { "from": "Jump", "to": "Fall", "exitTime": 1, "offset": 0.99 },
    { "from": "Fall", "to": "Land", "interruptibleBy": "next",
      "conditions": [{ "parameter": "grounded", "is": true }] },
    { "from": "Land", "to": "Jump", "conditions": [{ "parameter": "jump" }] }
  ] }
```

A state's name may not hold `/`, boxes nest at most 8 deep, and a box
always keeps one state.

## From code

```dart
@override
void fixedUpdate(FixedUpdateContext context) {
  animation.setFloat('speed', planarSpeed);   // also setInteger, setBool
  animation.setBool('grounded', grounded);
  if (jumped) animation.setTrigger('jump');   // resetTrigger clears it
}
```

- `animation.stateOf()!.name` is the state playing now (`next` is the
  one it is blending into); `normalizedTime` is how far it has played.
- `animation.play('Jump')` and `animation.crossfade('Jump', 0.1)` jump
  to a state by name, without conditions. A state in a sub-state
  machine is named by its path (`'Airborne/Fall'`); the machine's own
  name enters its default state.
- `onAnimatorStateEntered` and `onAnimatorStateExited` tell a behaviour
  when states change.
- A wrong parameter name or type throws, and the message lists the
  parameters the controller has.

### Typed, from codegen

`fynkel codegen` writes `lib/generated/animators.g.dart`, and the
editor rewrites it whenever a controller is saved. Each controller gets
a type named after its file, so a typo is a compile error instead of a
throw:

```dart
final anim = PlayerAnimator(animation);   // player.controller.json
anim.speed = planarSpeed;                 // setFloat('speed', …)
anim.grounded = me.grounded;              // setBool
anim.jump();                              // setTrigger('jump')
if (anim.state == PlayerState.jump) { … } // the base layer's state
anim.playOneShot(Assets.models.character.clips.wave, slot: PlayerSlot.upperBody);
```

It also has `nextState`, `play(PlayerState.x)`, `crossfade(…)` and
`resetJump()`. A state inside a sub-state machine gets its path in
camel case: `Airborne/Fall` is `PlayerState.airborneFall`. The string calls stay for code that does not know its
controller. `fynkel codegen --check` fails when the file is stale.

## Layers

A second layer plays over the first — for example, an aim pose on the
arms while the legs keep walking. Each layer has its own graph. Its
settings:

- **Weight**: how much of it shows (0 to 1).
- **Blending**: `override` replaces the pose below. `additive` adds
  the motion's change from its first frame.
- **Root**: a node such as `spine`. The layer then moves only that node
  and the nodes under it. Empty moves the whole character.
- **Mask**: Unity's Avatar Mask, per node. The button opens the
  followed object's nodes as a tree with a tick on each. Untick a node
  and the layer leaves it, and the nodes under it, to the layers below.
  Right-click a node to tick or untick only that node. The list under
  the tree shows the entries the mask keeps; **Clear** makes it pose the
  whole character again. Follow an object with the controller (select
  it) to see its nodes.

How a mask decides: the nearest entry wins. That is the node's own
entry, else the entry of the nearest node above it (unless that entry
is for its node only). With no entry that applies, a node is posed
unless some entry includes a node — so "only the arms" is one ticked
entry per arm, and "all but the head" is one unticked entry.

A layer may have **no states**. It then shows nothing until a one-shot
plays in its slot (below).

## Slots and one-shots

Every layer has a **slot** with the layer's name. The window shows it
in a row above the graph. A **one-shot** played into a slot covers that
layer's states: it blends in, plays once and blends out. The states
keep running under it, so the layer is in the right state when the
one-shot ends. There is no need for an arrow from Any State for every
attack or emote.

```dart
animation.playOneShot(attack, slot: 'UpperBody', blendIn: 0.1, blendOut: 0.2);
animation.stopOneShot(slot: 'UpperBody');           // blends it out
if (animation.isPlayingOneShot(slot: 'UpperBody')) { … }
```

- `slot` is a layer's name; empty is the base layer.
- The motion is a clip or a blend space.
- The clip's markers arrive as usual. `onAnimationEnded` fires when it
  reaches its end (not after `stopOneShot`).
- A second one-shot in the same slot blends from the first.
- The usual way to play an attack or a wave over walking legs: add a
  layer named `UpperBody`, set its Root to `spine`, give it no states,
  and play the one-shot there.
- `play(clip)` and `crossfade(clip)` still refuse an animator with a
  controller. A one-shot is how to play a clip over one.

During Play the slot row shows the one-shot playing, how much of it
shows and how far it has played.

### Clips made in code

A clip can be made in Dart and played the same way:

```dart
final wave = world.createClip((ClipBuilder()
      ..rotation('upperarm_r', {0: rest, 0.25: raised, 0.75: raised, 1: rest})
      ..marker(0.25, 'hello'))
    .build());
animation.playOneShot(wave, slot: 'UpperBody');
```

`ClipBuilder` keys a node's `position`, `rotation` or `scale` as
`{seconds: value}` (linear, or `ClipInterpolation.step`), by the node's
name under the animator. The clip is as long as its last key unless
`ClipBuilder(length:)` says otherwise. `world.setClip` changes it and
`world.destroyClip` removes it. It lasts across scene loads. An
`Animator`'s `clip` and `animation.play` take it too.

## IK

The **IK** tab, beside Parameters, lists the controller's IK goals.
They bend the pose after all the layers, in order. **+** adds one of
three kinds, with the node names a humanoid usually has (the window
picks the nearest name the followed object has, like
`mixamorig:RightHand`). Each node field can be typed or picked from the
object's nodes.

- **Two Bone**: a hand or a foot (the *tip*) reaches a target. The
  *middle* (elbow, knee) and the *root* (upper arm, thigh) are the
  tip's parent and grandparent unless named. The *pole* is an object
  the elbow or knee points to.
- **Look At**: the *head* turns to a target, within a *limit* in
  degrees, eased. The *spine* nodes take a share (*body*, 0 to 1) of
  the turn.
- **Foot Placement**: each foot (*left*, *right*) stands on what is
  under it. A ray goes down from each foot, up to *reach* metres above
  and below the character. The *hips* go down as far as the lower foot
  needs, the legs bend and each foot tilts to the ground. It needs the
  physics module.

Every goal has a **weight**: a number, or a float parameter so code
can fade it (for example 0 in the air). A goal's **target** is an
object's name; code can set one instead:

```dart
animation.setIKTarget('look', coin.position);          // world space
animation.setIKTarget('hand', handle, weight: 0.5);    // scales the goal's weight
animation.clearIKTarget('look');                       // back to its object, or nothing
```

A goal without a target does not bend anything (foot placement finds
its own ground). A wrong goal name is reported with the goals the
controller has. A node the character lacks, or foot placement without
physics, is reported once per goal (`FYN-R0015`) and the goal is
skipped.

## In Play

Press Play with the object selected and the Animator window shows the
game running. The playing state has a progress bar, and the arrow being
taken fills as the blend goes. The parameters show their live values:
change one (or click a trigger's button) to try a transition. These
changes are not saved.

## Examples

- **The reference game's Player**
  (`examples/reference_game/assets/models/player.controller.json`):
  Locomotion plays a blend space of idle, walk and run along `speed`.
  Aim (hold the right mouse button or LT) plays a 2D Freeform
  Directional blend tree over `moveX` and `moveY`: idle in the middle,
  walks at 4 and runs at 7 in each direction, so the Player strafes
  while facing the camera.
  The `jump` trigger goes to Jump; `grounded` being true goes back.
  `game.Player` only sets the parameters and the look target
  (`lib/game/player.dart`). The `UpperBody` layer (Root `spine`, no
  states) is the wave's slot: the wave key plays the wave clip there.
  Its `feet` goal stands each foot on the Playground's steps (its
  weight is the `feet` parameter: 1 standing, fading to 0 by 2 m/s, 0
  in the air — pinned feet suit standing, not a stride), and its `look` goal
  turns the head to the nearest coin (`setIKTarget`).

- **The showcase's Runner**
  (`examples/showcase/assets/models/runner.controller.json`): Run for
  four loops, then Jump once, then Run again. It uses exit times only,
  so it needs no code at all. Every 3 s `game.Waver`
  (`lib/game/waver.dart`) plays a wave made in code in its `UpperBody`
  slot.
