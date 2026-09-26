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
the Inspector change the order. Only one transition fires per tick, and none
while a blend is running.

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
  to a state by name, without conditions.
- `onAnimatorStateEntered` and `onAnimatorStateExited` tell a behaviour
  when states change.
- A wrong parameter name or type throws, and the message lists the
  parameters the controller has.

## Layers

A second layer plays over the first — for example, a wave on the arms
while the legs keep walking. Each layer has its own graph. Its settings:

- **Weight**: how much of it shows (0 to 1).
- **Blending**: `override` replaces the pose below. `additive` adds
  the motion's change from its first frame.
- **Root**: a node such as `spine`. The layer then moves only that node
  and the nodes under it. Empty moves the whole character.

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
  `game.Player` only sets `speed`, `grounded` and `jump`
  (`lib/game/player.dart`).
- **The showcase's Runner**
  (`examples/showcase/assets/models/runner.controller.json`): Run for
  four loops, then Jump once, then Run again. It uses exit times only,
  so it needs no code at all.
