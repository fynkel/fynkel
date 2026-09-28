# Animation: the clip core, skinning, the `Animator` — blend spaces, layers, markers, root motion

The animation contract: blueprint §9 ("Animation"), §19.2d item 19
(M3-19, built 2026-09-21) and item 19c (M3-19c, 2026-09-21: **blend
spaces**, **layers**, **markers** and **root motion** — what a character
needs beyond a crossfade), §5.1 (where in the tick it runs). One clip
core, built once, with every front door on it: a rigged character's
walk, a door's swing and a lamp's bob are the same thing to the sampler;
a blend space is two samples and one blend of it, a layer is a second
animator writing after the first, and the follow-ups (the state machine
as data, the timeline/sequencer document) sit on the same sampler with
no second one. The four features in one paragraph: an `Animator` plays a
**clip** or a **blend space** (`<name>.blend.json`: clips along a named
parameter, mixed phase-synced by `setParameter`); an `Animator` on a
descendant entity is a **layer** over its ancestors' pose (`weight`,
`additive`); a clip carries **markers** (a rig document's `markers`, a
glTF animation's `extras`) raised as `AnimationMarker` events when the
clock crosses them; **root motion** is opt-in per animator
(`root_motion`), the root's travel taken out of the pose and handed to
the character controller or the transform.

| Piece | Where | What |
| --- | --- | --- |
| the clip core | `crates/fynkel_animation` (its own crate since ADR-0029; `fynkel_core::animation` re-exports it) | `Clip` = channels keying one property of one **named** node, with its `markers`; `Clip::sample(time)` → `Pose`; `Pose::blend`, `Pose::add_delta`; `BlendSpace` = points `(at, clip)` along a `parameter` (`segment(value)` → the two neighbours and the fraction); `Skin` = named joints with inverse bind matrices |
| the systems | `crates/fynkel_core/src/world/animation.rs` | `run_animation` in the pre-tick slot: every `fynkel.Animator`'s clock, clip or blend space, crossfade, markers, root motion and pose write (ancestors first, so a child animator is a layer); every `fynkel.Skin`'s joint lookup; `skin_palette` for the extract |
| the components | `fynkel.Animator` (id 21), `fynkel.Skin` (22) | ordinary native components (`docs/codegen.md`): authored, validated, generated into every codec, reachable from Dart on the M3-07a path |
| the blend document | `<name>.blend.json` — `schemas/blend.schema.json`, `crates/fynkel_document/src/blend.rs`, `DocumentKind::Blend` (`docs/documents.md` § Blend spaces) | `{ parameter, points: [{ at, clip }], sync: "phase" }`; validated (`FYN-D0600`–`D0602`), cooked as a JSON document chunk depending on its clips' carriers, registered by the asset store with its clips (`Assets::blend`, `register_clip`) |
| the import | `crates/fynkel_import/src/gltf.rs` (importer version 7), `fbx.rs`, the rig document (`docs/documents.md`) | glTF skins → `skin/<name>`, animations → `anim/<name>` (with their `extras.markers`), `JOINTS_0`/`WEIGHTS_0` → a skinned `mesh/<n>`; the prefab gets the `fynkel.Skin`; a rig's `markers` join its clips at cook |
| the cache | `crates/fynkel_cooked/src/payload.rs` | `SKIN` and `ANIM` chunks (an `ANIM` carries its markers after the channels); a skinned `MESH` carries a trailing `SkinWeight` section |
| the renderer | `crates/fynkel_render` (`docs/renderer.md`) | the skinned vertex path: a joint palette storage buffer and `vs_skinned` in five pipeline variants |
| the wire | `fynkel.cmd.Animate` (kind 9), `fynkel.ev.AnimationEnded` (9), `fynkel.ev.AnimationMarker` (10), `fynkel.rec.AnimatorState`, `FYNKEL_QUERY_ANIMATOR` (11) | play/crossfade/stop/pause/resume/speed/time/weight/parameter; the one-shot's end; a marker crossed; the state read-back |
| Dart | `packages/fynkel/lib/src/animation.dart`, `world.dart` | `GameObject.animation` → `AnimationControl`; `Behaviour.animation`, `onAnimationEnded`, `onAnimationMarker`; `AnimatorState`; `LoadReport.animation` |

Tests: `crates/fynkel_core/tests/animation.rs` (the sampler, blending,
the animator's clock, the skin's palette, layers, markers, root motion
with and without a controller, a blend space phase-synced by its
parameter and waiting for its clips), `crates/fynkel_import/tests/gltf.rs`
(the rig fixture), `crates/fynkel_render/tests/project.rs`
`skinned_meshes_follow_their_joint_palette`, `crates/fynkel_native/tests/abi.rs`,
`crates/fynkel_document/tests/project.rs` and
`crates/fynkel_import/tests/cook.rs` (the blend document validated and
cooked), `crates/fynkel_assets/tests/store.rs` (registered with its
clips), `apps/cli/tests/check.rs` (the Khronos Fox on a blend space —
survey → walk → run by `speed`: every bone at 1.5 is the midpoint of the
walk and the run at one phase, the phase kept across a change),
`packages/fynkel/test/animation_test.dart` (the fixture's `column.blend.json`
from Dart), the reference game's `test/game_test.dart` (the character
posed through its Animator Controller — the blend space, the jump — the
wave layer, the footsteps). The fixture is `crates/fynkel_import/fixtures/rig.glb`
(written by `rig.py`): a skinned column on three joints under a "wave"
clip, a static Prop under a one-shot "bob"; the document fixture
project's `Rig` scene (`c0000000-…-000034`) plays both, its
`rigs/column.blend.json` mixes them by `energy`.

**An optional module** (ADR-0029). A project can turn animation off
(`modules.animation: false` in `fynkel.project.json`, Project ▸ Modules… in the
editor). Then the system is skipped, its commands are rejected with
`MODULE_OFF` (`FYN-R0014`), `Animator` and `Skin` in a scene or prefab are
`FYN-P0018`, and from Dart `GameObject.animation` throw a `StateError` naming the module
(`world.modules.has(Module.animation)` answers first). With `modules.strip: true`
a runtime built from source also leaves out the sampler; the types stay, so the
contract is the same in every build.

## The clip core

A **clip** is a set of channels. Each channel keys one property —
translation, rotation or scale — of one node **named** under the entity
that plays the clip, with step, linear or cubic-spline interpolation
(glTF's three), and the clip has a duration. Sampling a clip at a time
gives a **pose**: per target entity, the properties the clip sets and
nothing else (a channel-less property keeps whatever the entity has).
Poses **blend** — a lerp for translation and scale, a slerp for rotation,
per entry — which is all a crossfade needs, and all a state machine or a
timeline will need.

Targets resolve by **name**, not entity id, because a prefab instance
gets fresh ids: what the importer wrote into a clip stays valid for
every instance. The search is a depth-first walk of the animator's
subtree, first match wins, so a name must be unique under its animator
(the importer warns on duplicates across a file); the resolved targets
are cached on the animator and re-resolved only when the clip changes or
a cached entity dies or is renamed.

A **skin** is what a rigged mesh adds: an ordered joint list — each a
node name, its parent within the skin, its inverse bind matrix and its
rest pose — and the name of the skeleton root. The joints themselves are
ordinary entities with ordinary transforms: the same clip core drives
them, a behaviour reads or overrides one by name (`findChild('head')`),
a collider or a light can hang off one, and the renderer reads their
interpolated world matrices like any other entity's. At most
`Skin::MAX_JOINTS` = 256 joints per skin (the renderer's palette).

## Components

| Component | id | Fields | Rule |
| --- | --- | --- | --- |
| `fynkel.Animator` | 21 | `clip` (an `anim/<name>` asset ref, or a blend document's id with no sub — § Blend spaces; nil plays nothing), `speed` (finite; negative plays backwards), `looping`, `playing`, `weight` (0…1, default 1), `additive` (default false), `root_motion` (default false) | plays `clip` over the named nodes under this entity; a second animator on a descendant is a layer over its ancestors' pose (§ Layers); `speed` non-finite or `weight` outside 0…1 is `INVALID_PAYLOAD` (`FYN-H0008` at `fynkel validate`) |
| `fynkel.Skin` | 22 | `skin` (a `skin/<name>` asset ref), `skeleton` (the joint root object, a reference — Unity's `rootBone`, ADR-0035; None: the tree's root) | next to the `fynkel.MeshRenderer` of a skinned mesh: the joints are found by name under `skeleton` |

Both are ordinary native components: authored in a scene or prefab,
validated, listed in `fynkel.project.json` → `components`, `get<Animator>()`
/ `add(const Skin(…))` / `set` / `remove<Animator>()` from Dart. The
runtime state each needs lives in companion components that are never
authored or on the wire: `AnimatorRun { time, targets, fade, fresh, seek,
reported, reference, parameters, root, root_delta, root_yaw }` and
`SkinRun { root, joints, resolved, reported }`, attached on the first
tick the component is seen and detached with it. Scenes and prefabs
carrying an `Animator` are **formatVersion 3** (`scene/2-3/animator-layers`
fills the three new fields in; `docs/documents.md` § Migrations).

The importer writes the `fynkel.Skin` into the prefab for a node with a
skinned mesh, so instantiating an imported character's `prefab` gives a
rigged, skinned hierarchy; a scene may author the same tree by hand (the
reference game's `tools/playground.py` does, from the same table that
built the model) so the joints have stable ids a behaviour or the editor
can address.

## The tick (§5.1)

```text
World::step
  drive_characters                 the characters' action driver (M3-09)
  apply C[n]                       the tick's commands — Animate among them
  run_animation                    every Animator: clock, crossfade, pose → local transforms
                                   every Skin: joint entities resolved under its skeleton
  run_rotators, run_oscillators…   the catalogue's pre-tick movers
  physics                          propagate, sync, Rapier step, write-back
  propagate; camera rigs; followers; publish
```

Animators run **first** among the pre-tick systems: a mover or a body on
a posed joint follows the pose this tick, and a collider on a joint is
where the pose put it before Rapier sees it. Every animator, **ancestors
first** (sorted by depth in the tree, then entity — so an animator on a
child entity writes after the one above it and is a layer over it,
§ Layers):

1. **The clip.** A nil `clip` poses nothing. A clip the world does not
   hold reports `CLIP_UNAVAILABLE` (16) once per clip per animator and
   the animator waits (`duration` reads 0); the report repeats for the
   next unknown clip. A `clip` naming a blend space the world holds plays
   the space (§ Blend spaces: the two points around the parameter are
   "the clip" below, their lengths blended into the duration; a point's
   clip the world lacks is the `CLIP_UNAVAILABLE`, naming that clip).
2. **The clock.** A *fresh* animator — a new component, a `Play`, a
   `Crossfade`, a `SetTime` — samples its start without advancing: time
   0, the end for a negative speed, or the seek. So the first tick after
   a scene load shows the clip's first frame, and a 2 s one-shot loaded
   with the scene ends when tick 122 passes 2 s, not tick 120. Every
   later tick with `playing` advances `time += speed · dt`: looping wraps
   (`rem_euclid`), a one-shot clamps to `[0, duration]` and, the tick it
   reaches an end it was not at, raises `ANIMATION_ENDED { entity, clip }`
   on the general lane and clears `playing`. The pose holds. (A fresh
   non-looping clip of zero length ends at once.)
3. **The pose.** `clip.sample(time)` over the cached targets; a channel
   whose target did not resolve is skipped silently (the name is the
   clip's contract; a missing node is an authoring gap the importer's
   prefab never has). A clip written onto the component (`set` from
   Dart, a scene override) restarts the clock like a `Play`.
3a. **The markers.** Every marker the clock crossed between the previous
   time and this one is raised as `ANIMATION_MARKER { entity, clip,
   name }` — once per crossing, each lap of a loop again, backwards too
   (§ Markers). A fresh tick (a load, `Play`, `SetTime`) crosses nothing.
3b. **Root motion.** With `root_motion`, the root's planar travel and
   yaw are taken out of the pose and handed to the entity (§ Root motion).
4. **The crossfade.** `Crossfade(clip, seconds)` keeps the outgoing clip
   playing on its own clock (its speed, its looping) for `seconds` and
   blends its pose out linearly: weight `1 − remaining / total` on the
   new clip. `seconds` 0, or a crossfade into the clip already playing,
   is a plain `Play` (a restart, no fade); a crossfade during a crossfade
   drops the older fade and fades from the clip playing now (no stack of
   fades); a paused animator's outgoing pose holds still while it fades.
   `fade_remaining` reads back the seconds left.
5. **The write.** Each pose entry is applied to the target's local
   transform (only the properties the clip keys), through `set_local`, so
   a child of a posed joint is consistent at the tick's propagate; an
   entry that changes nothing writes nothing. At `weight < 1` the entry
   is blended toward from the target's current value (lerp / slerp /
   lerp); an `additive` animator adds the clip's *change from its first
   frame*, scaled by `weight`, onto the current value instead.

Then every `fynkel.Skin`: the skeleton root is the object `skeleton`
references (ADR-0035; resolved once when the scene loads or the prefab is
instantiated, so each copy uses its own; None: the mesh entity's tree
root; destroyed: the bind pose), the skin's joints by name under it,
cached while every one still lives under its name. A skin the world does not hold reports `SKIN_UNAVAILABLE` (17)
once and the mesh draws in its bind pose; so does a joint that did not
resolve (its palette entry is the identity).

`World::systemTimes()` reports the pass as `SystemKind.animation`
(entities = animators + skins); `fynkel test --headless` prints an
`animation:` line — animators + skins, clips and skins registered,
skinned meshes with their joint count, one-shots ended, diagnostics.

**Root motion, the default policy (item 19 (d)).** Clips extract no root
motion unless the animator opts in. A `CharacterController` (or the
game) moves the entity; the animator only poses nodes *under* it, so
clips are authored **in place** — the reference game's walk and run cycle
the legs while the controller carries the player, and a jump's hips dip
and rise while the controller's own jump lifts the entity. A clip that
does key the animator entity itself would fight whatever else moves it;
nothing stops it, nothing compensates for it. `root_motion: true` is the
opt-in for clips that travel (§ Root motion).

## Blend spaces (M3-19c)

A **blend space** is a document, `<name>.blend.json` (`DocumentKind::Blend`,
`schemas/blend.schema.json`; `docs/documents.md` § Blend spaces):

```json
{
  "$schema": "https://fynkel.dev/schemas/blend.schema.json",
  "formatVersion": 1,
  "id": "…", "name": "Player locomotion",
  "parameter": "speed",
  "points": [
    { "at": 0, "clip": { "asset": "<character>", "sub": "anim/idle" } },
    { "at": 4, "clip": { "asset": "<character>", "sub": "anim/walk" } },
    { "at": 7, "clip": { "asset": "<character>", "sub": "anim/run" } }
  ],
  "sync": "phase"
}
```

One named **parameter**; 1 to 16 **points**, each a clip (an `anim/<name>`
of a rig document or a model source) at a strictly increasing `at`
(`FYN-D0600` otherwise, `D0601` for a clip that is not one, `D0602` for
a malformed entry); `sync` is `phase` (the only mode: 1D and phase-sync
are V1, a 2D space is M5). The cook writes it as a document chunk that
depends on its clips' carriers, so a rig or model change re-cooks it;
the asset store registers a space together with every point's clip
(`Assets::register_clip` on a blend id), from a scene's animator, a
behaviour's `play`, or an `Animator` `set` from Dart.

An **animator plays a space like a clip**: `clip: { asset: <blend id> }`
(no sub), `play(AssetRef(blendId))`, `crossfade(...)` into or out of it.
Every tick the world reads the animator's parameter of that name
(`setParameter('speed', v)` — unset, the first point's `at`), clamps it
to the points, finds the segment `i…j` it falls in and the fraction `t`,
and:

- **phase sync** — the blended lap is `D = lerp(dᵢ, dⱼ, t)`; the
  animator's clock runs over `D` (looping, one-shot, `speed`, `setTime`
  as ever) and each point's clip is sampled at `phase · dₖ`, `phase =
  time / D`, so the feet of a walk and a run agree at every mix. When
  `D` changes (the parameter moved) the clock keeps its *phase*: `time
  ·= D / D_previous`. `AnimatorState.duration` reads `D`, `time` the
  blended clock, `parameter` the value the space reads.
- **the pose** — clip *i* sampled, clip *j* sampled, `Pose::blend(t)`
  (a property only one side keys is taken as is — the Column's Prop
  follows `bob` at every `energy`); at a point exactly (`t = 0`) one
  sample.
- **markers** come from the **dominant** clip (`t < 0.5` → *i*, else
  *j*) on its own scaled clock, so a walk's footsteps fire at the
  walk's times while the walk leads and the run's when the run does —
  never twice for one footfall.
- **root motion** (opted in) is the lerp of the two clips' root deltas.
- a **crossfade** out of a space fades its dominant clip at the scaled
  time; the additive **reference** of a layer playing a space is the
  first point's clip at 0.

The reference game's Player sets `speed` from the controller's planar
velocity every tick and never switches locomotion clips
(`examples/reference_game/lib/game/player.dart`; since ADR-0032 the
space is its Animator Controller's Locomotion state); the Khronos Fox's
survey → walk → run is the acceptance test (`apps/cli/tests/check.rs`).

## Layers (M3-19c)

There is no `layers[]` on the component. A **layer is a second
`fynkel.Animator` on a descendant entity** — the animators of a tree run
ancestors first, so the child's pose is written over what the parent
left, and its `weight`/`additive` say how:

| | `additive: false` | `additive: true` |
| --- | --- | --- |
| `weight` 1 | the child's clip replaces the parent's pose on the nodes it keys (the classic upper-body override: put the animator on the `spine` and its clip only keys the arms) | the child's clip's change from its first frame is added onto the parent's pose (a wave layered on a walk keeps the walk's spine sway) |
| `weight` w | lerp/slerp from the parent's value toward the child's, by w | the change scaled by w |

The child's clip keys nodes *under the child*, like any animator — a
layer on the `spine` cannot pose the hips. The additive reference is
the clip sampled at t = 0 over the same targets (cached with the
targets); a clip whose first frame *is* the base pose adds nothing at
rest. `weight` is authored or set from Dart (`animation.weight = 0.5`,
`SetWeight`), read back as `AnimatorState.weight`. This deviates from the
blueprint's `Animator.layers[]` on purpose: the layer is an entity, so
it has a transform, a name, a stable id, its own `state`, its own
markers and events, and the editor's tree shows it — nothing new on the
wire. Two animators on the *same* entity are not a layer (an entity
holds one component of a type).

## Markers (M3-19c)

A clip carries named times. Authored in the rig document —

```json
"markers": { "walk": [ { "at": 0.12, "name": "footstep" }, { "at": 0.61, "name": "footstep" } ] }
```

— or in a glTF animation's `extras`: `{ "markers": [ { "at": 0.12,
"name": "footstep" } ] }` (a Blender custom property, or a line in the
exported `.gltf`); the importer (`fynkel.gltf` version 5) keeps them on
the `anim/<name>` and the cook merges the rig's into the retargeted clip
(sorted; one past the clip's end is dropped with `FYN-D0502`; a
malformed entry is `A0104`). `fynkel rig inspect` counts them per clip.
The `ANIM` payload stores them after the channels (`marker_count`, then
`f32 at` + name each; an older payload without the section decodes to
none).

At the tick, every marker whose time lies in the interval the clock
advanced over is raised as `fynkel.ev.AnimationMarker { entity, clip,
name }` (event kind 10, general lane) — once per crossing, again on
every lap of a loop (the interval is unwrapped, so a tick that wraps
sees the end of one lap and the start of the next), backwards at a
negative speed, never on a fresh tick and never for the outgoing side of
a crossfade. A marker exactly at 0 fires when a loop wraps past it, not
on the tick that starts the clip. From Dart: `Behaviour.onAnimationMarker(
AnimationMarker { entity, clip, name })` on the animator's object, right
after the step with the other tick callbacks; the headless host counts
them (`animation.markers`).

## Root motion (M3-19c)

`root_motion: true` on the animator opts a travelling clip in. The
**root** is the target of the clip's shallowest translation channel
under the animator (a Mixamo `mixamorig:Hips`, the Fox's `_rootJoint`);
its motion is measured in its parent's space, assumed Y-up: the planar
`x`/`z` of the root's translation relative to the clip's first frame,
and the **yaw** as the twist about Y of `R(t) · R(0)⁻¹` when the clip
also rotates the root. Per tick the delta since the previous time
(laps of a loop add the whole clip's travel) is converted through the
local matrices from the animator's entity down to the root's parent —
so a 0.01-scaled character walks in metres — and:

- with a `CharacterController` on the same entity, handed to it as a
  velocity (`Physics::add_root_motion`, summed into the tick's `Move`),
  so the controller's collision, slopes and steps apply; the yaw is
  *reported*, not applied — the controller faces its motion
  (`docs/physics.md`);
- without one, the entity's local transform is moved by the rotated
  delta and turned by the yaw.

The pose itself keeps the root at its first-frame `x`/`z` (its `y`
stays: a jump still lifts the hips) and its rotation with the yaw
removed, so the clip plays in place under the moving entity. Forward is
−z, the controller's convention. `AnimatorState.root_delta` /
`root_yaw` read back what the last tick took out; both are zero without
the opt-in or for a clip that keys no root translation. Not in this
version: root motion from a layer (only the animator that owns the root
extracts), or from the outgoing clip of a crossfade.

## Commands, the query and the event

`fynkel.cmd.Animate` (command kind 9, simulation lane, 80 bytes:
`{ entity, op, seconds, speed, looping, clip, value, name, integer,
layer, point }`) carries one `AnimateOp`. The clip ops are for an animator
without a controller; the ones from `setFloat` on (ADR-0032, ADR-0033)
are for one with:

| `op` | Effect | Rejection |
| --- | --- | --- |
| `play` | switch to `clip` from its start (its end for `speed < 0`), `looping`/`speed` as given, `playing` | |
| `crossfade` | like `play`, the current clip blended out over `seconds` | `seconds < 0` or non-finite → `INVALID_PAYLOAD` |
| `stop` | rewind to 0 and clear `playing`; the entities keep their last pose | |
| `pause` / `resume` | clear / set `playing`, the time kept | |
| `setSpeed` | `speed` | non-finite → `INVALID_PAYLOAD` |
| `setTime` | seek to `seconds` (clamped to the clip) on the next tick, without advancing | negative or non-finite → `INVALID_PAYLOAD` |
| `setWeight` | the layer weight `value` (§ Layers) | outside 0…1 or non-finite → `INVALID_PAYLOAD` |
| `setParameter` | set the named parameter `name` to `value` on this animator (what a blend space reads every tick, clamped to its points, the phase kept; kept on the animator across clips) | empty `name` or non-finite `value` → `INVALID_PAYLOAD` |
| `setFloat` / `setInteger` / `setBool` | the controller's parameter `name` to `value` / `integer` / `value != 0` | no such name (the message lists the names), another type, or no controller → `INVALID_PAYLOAD` |
| `setTrigger` / `resetTrigger` | set / clear the trigger `name`; the transition it fires clears it | as above |
| `playState` | jump to the state `name` at once (Unity's `Play`), at `value` of its motion (0…1), in `layer` (`-1`: the first layer that has it) | no such state (the message lists the states), offset outside 0…1 → `INVALID_PAYLOAD` |
| `crossFadeState` | like `playState`, the current state blended out over `seconds` (Unity's `CrossFade`) | as above, and `seconds < 0` |
| `setIkTarget` | the IK goal `name`'s target is the world `point` from the next tick, `value` (0…1) scaling the goal's weight (§ Masks and IK) | no such goal (the message lists the goals), a non-finite `point`, `value` outside 0…1 → `INVALID_PAYLOAD` |
| `clearIkTarget` | drop the target code gave the goal `name` | no such goal |

A clip op (`play`, `crossfade`, `setTime`, `setParameter`) on an animator
that runs a controller is `INVALID_PAYLOAD`: the controller owns what
plays. `stop`, `pause`, `resume`, `setSpeed` and `setWeight` work on
both.

An entity without an `Animator` is `MISSING_COMPONENT` (component 21).
The native layer (`fynkel_native`) registers from the cache, before the
batch is applied, any clip or blend space a `play`/`crossfade` names that
the loaded scene never did (a space with every point's clip), and the
clip or skin of an `Animator`/`Skin` a behaviour `set`s — so a behaviour
plays `anim/jump` of a character whose scene only authored `anim/idle` without
listing every clip somewhere; a clip the cache cannot read stays
unregistered and the animator reports `CLIP_UNAVAILABLE` at the tick.

`FYNKEL_QUERY_ANIMATOR` (11) with an entity answers one
`fynkel.rec.AnimatorState { entity, time, duration, speed, fade_remaining,
playing, looping, clip, weight, parameter, root_delta, root_yaw }`
(80 bytes) as the last step left it, or no record for an entity without
an animator; `parameter` is the value the playing blend space reads
(else the last value `setParameter` set), `duration` a space's blended
lap.

`FYNKEL_QUERY_ANIMATOR_CONTROLLER` (14) answers one
`fynkel.rec.AnimatorLayerState { entity, layer, state, normalized_time,
length, in_transition, next, next_normalized_time, transition_progress }`
per layer of the controller the animator runs, and
`FYNKEL_QUERY_ANIMATOR_PARAMETERS` (15) one `fynkel.rec.AnimatorParameter
{ entity, name, kind, value, integer, flag }` per parameter; no records
without a running controller (§ Animator Controllers).

`fynkel.ev.AnimationEnded { entity, clip }` (event kind 9, general lane)
is raised the tick a one-shot passes its end (`playing` is cleared the
same tick); a looping clip never raises it, and neither does `stop`.
`fynkel.ev.AnimationMarker { entity, clip, name }` (kind 10) is raised
once per marker crossed (§ Markers).

## Animator Controllers (ADR-0032)

How to make and edit one — the Animator window, states, transitions,
parameters and the code — is the guide [animator.md](animator.md). This
section is the engine's side.

An `Animator` whose `controller` names an Animator Controller
(`<name>.controller.json`, [documents.md](documents.md) § Animator
Controllers) runs the controller's states instead of its `clip`. The
animator's `speed`, `playing`, `root_motion` and `weight` still apply
to the whole animator. Without a `controller` nothing changes.

**The machine** is `fynkel_animation::controller` (pure, no world): a
`ControllerDef` (the document with names turned into indices) and one
`ControllerRun` per animator (the parameter values and, per layer, the
current state, its normalized time and the crossfade). Unity's rules:

- The first tick enters each layer's default state at time 0; the next
  tick advances it.
- A state's clock moves by `speed × speed parameter × dt` over its
  motion's length (a blend space's blended length, so its phase is
  kept; no motion counts as one second). Normalized time is not wrapped:
  1.5 is halfway through the second loop.
- At most one transition per layer per tick: Any State's first (never
  into the current state unless `canTransitionToSelf`), then the current
  state's, in file order. Every condition must hold. A trigger it tests
  is cleared when it fires.
- An exit time below 1 on a looping state is reached once per loop; any
  other exit time holds once reached.
- No interruptions: while a crossfade runs, no transition is checked.
  The check starts again on the tick after it ends.
- `playState` and `crossFadeState` jump without conditions. A crossfade
  already running is cut short: its outgoing state exits.

**Events.** `fynkel.ev.AnimatorStateEntered { entity, layer, state, from }`
(event kind 12; `from` is empty on the first tick) when a state starts —
at the start of its crossfade — and `fynkel.ev.AnimatorStateExited {
entity, layer, state }` (kind 13) when the state stops playing: at the
end of its crossfade out, at once for a jump. On a jump the exit comes
first.

**The pose** (`fynkel_core::world::controller`). Each state's motion
resolves by name under the animator, or under the layer's `root` node
(the mask by placement). During a crossfade the outgoing state is
sampled at its clock and the incoming one blended over it by the
crossfade's progress. Layers after the first go over the layers below:
`override` at the layer's weight, or `additive` (the motion's change
from its first frame, at the weight). A node the layers below do not
pose starts from its rest transform (its transform when the controller
first resolved it), so an additive layer does not add to itself each
tick. Root motion comes from the base layer (the two states' deltas
weighted by the crossfade); markers from each layer's dominant state.

**Loading.** `Assets::register_controller` (and `register_animation`
for an `Animator` that names one) registers the controller's motions and
then the controller; `World::register_controller` checks it
(`ControllerDef::validate`). An animator whose controller is not
registered reports `CLIP_UNAVAILABLE` once and plays once it is.

**The query.** `World::animator_controller_state(entity)` gives the
layers and parameters as the last step left them, as Unity reports them:
during a crossfade, `state` is the state fading out and `next` the one
fading in.

Tests: `crates/fynkel_animation/src/controller.rs` (the rules),
`crates/fynkel_core/tests/controller.rs` (a whole jump, determinism, the
commands, layers, a missing controller),
`crates/fynkel_native/tests/abi.rs`
(`animator_controller_ops_events_and_queries`) and
`crates/fynkel_assets/tests/store.rs` (a cooked controller registers with
its motions).

**In the games** (step 7). The reference game's Player runs
`assets/models/player.controller.json` (written by
`tools/playground.py`): parameters `speed` (float), `grounded` (bool,
default true) and `jump` (trigger); Locomotion plays the blend space,
Jump the jump clip once. Locomotion → Jump on `jump` (0.1 s), Jump →
Locomotion when `grounded` is true, after 20 % of the jump (0.15 s).
`game.Player` sets the three parameters every tick and nothing else —
its own jump and land code is gone. It sends `grounded` as false on the
tick it jumps, so the landing test cannot pass before the character
leaves the ground. The showcase's Runner runs
`assets/models/runner.controller.json`: Run for four loops, then Jump
once, then Run, on exit times alone — a controller with no code. Both
games' tests follow the states (`stateOf()`), and `--hero` still works:
the generator writes the Player's controller with the rig's jump.

## Blend trees (ADR-0033)

A state's `motion` may be a blend tree
([documents.md](documents.md) § Animator Controllers has the format;
[animator.md](animator.md) § Blend trees the editor). At load, each
state's motion becomes a small tree of nodes; a blend space becomes a 1D
node on the float of its name. Every tick the weights are computed
without allocating:

- **1D**: linear between the two nearest thresholds, clamped at the
  ends.
- **Simple directional**: the two children around the input's
  direction, by barycentric weights (with the centre child when the
  input is inside them); otherwise as freeform cartesian.
- **Freeform cartesian**: the gradient band (Johansen): each child's
  weight is its smallest `1 - (ip·ij)/|ij|²` over the others.
- **Freeform directional**: the same in polar form, the angle weighed
  twice a relative change of speed.
- **Direct**: each child's parameter, negative as 0, normalized.

Weights under 0.001 are dropped and the rest renormalized. Every leaf
shares the state's normalized time; the tree's length is the leaves'
durations (over their `timeScale`) weighted. Markers come from the
leaf with the most weight; root motion is the weighted sum of the
leaves'. The editor's map uses the same math
(`apps/editor/lib/src/animator/blend_tree_weights.dart`), and both are
tested against the same hand-worked values.

`World::clip_root_travel` gives a clip's root travel per second (the
shallowest translation channel, the one root motion takes, and its
yaw): the editor's Compute Positions reads it through the preview
stage's `travel` call.

## Slots and one-shots (ADR-0033)

Every layer of a controller has a slot named after the layer
([animator.md](animator.md) § Slots and one-shots is the guide). The
`Animate` ops `PlayOneShot { motion, slot, blend_in, blend_out, speed }`
and `StopOneShot { slot, blend_out }` drive it; an empty slot is the
base layer. Each layer keeps the one-shot playing and, after a stop or
a replacement, the one blending out.

- A one-shot's weight is `in × out`: `in` rises over `blend_in` from
  its start, `out` falls over the last `blend_out` of its length (or
  over `blend_out` from a stop).
- **A layer with states**: the one-shot's pose is blended over the
  state machine's by that weight; a node the one-shot does not key
  keeps the machine's pose. The machine keeps ticking under it.
- **A layer without states**: the one-shot is the layer's pose, and the
  layer shows by its weight times the one-shot's.
- Markers come from the one-shot (its leading leaf for a blend space)
  while it is not stopped. `AnimationEnded` fires at its natural end
  only. A one-shot moves no root.
- Refused with a diagnostic that says why: an unknown slot (the message
  lists the layers), blends that are not finite and ≥ 0, a motion the
  world does not have, an animator without a controller.
- The layer state record carries `slot`, `one_shot`, `one_shot_time`
  (normalized) and `one_shot_weight`; Dart reads them as
  `AnimatorStateInfo.slot`/`oneShot`/`oneShotTime`/`oneShotWeight`.

## Sub-state machines and interruptions (ADR-0033)

The document keeps sub-state machines as a tree ([animator.md](animator.md)
§ Sub-state machines is the guide); `fynkel_assets::controller_def`
flattens them, so the runtime only ever sees a flat layer:

- The states that play become the layer's, named by path
  (`Airborne/Fall`), in file order.
- A transition into a machine goes to its default state, followed down
  through nested machines.
- A transition from a machine becomes one transition per state inside
  it. Scopes are emitted outer first, so for a state the machines it is
  in are checked before its own transitions (Any State still first).
- `LayerDef::machines` maps a machine's path to the state it enters:
  `find_state("Airborne")` (what `PlayState` and Dart's `play` use)
  gives that state.

**Interruptions.** `TransitionDef::interruptible_by` is `None` (the
default: nothing fires while its crossfade runs), `Next` (the incoming
state's transitions) or `Any` (Any State's too). The pure runtime
checks them during the fade; when one fires, the outgoing state exits
and the new `Crossfade` has `frozen: true` and bumps
`LayerRun::interruptions`. The world keeps each layer's machine pose
from the last tick (under its slot) and copies it when the count
changes; a frozen fade blends from that held pose instead of sampling
`from`, whose clock stops. Code's `play`/`crossFade` are not
interruptions: they cut a fade as before.

**Clips made in code.** `fynkel_world_set_clip(world, id, payload,
len)` registers an `ANIM` payload as the runtime clip `id`, on the
asset store (read before cooked clips, kept across clears and scene
loads) and the world at once; an invalid clip is refused and the old
one kept, an empty payload removes it. Dart's `ClipBuilder` writes the
payload and `world.createClip`/`setClip`/`destroyClip` call it.

## Masks and IK (ADR-0033)

[animator.md](animator.md) § Layers and § IK are the guide; this is how
the runtime does it.

**Masks.** `LayerDef::mask` is a list of `MaskEntry { node, include,
subtree }`; `controller::masked_in(mask, names)` decides for a node
given its name and its parents' up to the animator: the node's own
entry, else the nearest ancestor's entry with `subtree` (the last of
two for one node), else "posed unless some entry includes". The world
filters each layer's pose with it before the layer blends (the base
layer after its slot, a stateless layer's one-shot too), and caches the
answer per layer and node. A root and a mask together: the node must be
under the root and masked in.

**IK.** `ControllerDef::ik` holds up to 16 `IkDef { name, kind, weight,
target }`, `weight` a number or a float parameter (read each tick),
`target` an object's name (a node under the animator first, else the
first object so named). `world/ik.rs` runs after an animator's layers
have written the pose, goal by goal:

- `twoBone { tip, middle?, root?, pole? }`: the analytic two-bone
  solve — the root and the middle turn so the tip reaches the target
  (clamped to the chain's reach), bending towards the pole (else the
  way the chain already bends); the rotations are blended by the weight.
- `lookAt { head, spine, limit, body }`: the target's yaw and pitch
  from the character's forward (−Z, up +Y), each clamped to `limit`
  (radians at runtime; degrees in the document) and scaled by the
  weight, then eased (rate 10 per second, so the head follows rather
  than snaps); the spine nodes share `body` of the turn evenly, the
  head the rest.
- `footPlacement { hips, left, right, reach }`: a ray down at each
  foot, from `reach` above the animator's height to `reach` below
  (the character's own `CharacterController` body left out); the hips
  go down by the lower foot's drop (never up), each leg is a two-bone
  solve to its foot's ground (the foot's parent the knee, the knee's
  the thigh), and each foot turns to the ground's normal (at most
  0.6 rad). The drop and the lifts are eased and scaled by the weight;
  on flat ground it does nothing.

What IK writes is taken back on the next tick when no layer poses the
node again (a node's transform that still equals what IK wrote is
restored), so goals never pile up on nodes the clips do not key. A
node the character lacks, or foot placement with the physics module
off, is `IK_UNAVAILABLE` (22, `FYN-R0015`) once per goal, and the goal
is skipped. Code's target (`SetIkTarget`) wins over the document's
`target`; `ClearIkTarget` goes back to it.

**Checked on a rig we did not make.** `examples/reference_game/tools/hero_copy.py`
makes `examples/rg_hero`, a local copy whose Player wears the downloaded
Mixamo character (§ Four downloads, one character) with the goals on its
`mixamorig:` bones, and `test/hero_ik_test.dart` runs there (it skips
anywhere else): on flat ground, across two steps in four facings and
stopped on the ramp, each ankle stays at its flat-ground height (0.088 m)
above the ground under it, the hips go down one step when a foot is on
the lower, the knees keep bending forward, and the head turns to a coin
as far as the limit with the spine's share steady. Mixamo's tilted bone
axes, the ankle above the sole and the knees bent at rest need nothing
special: IK turns bones in world space.

## Import and the cache

The glTF importer (`fynkel.gltf` **version 5**; `docs/documents.md`)
keeps what a rigged source carries:

- a primitive with `JOINTS_0`/`WEIGHTS_0` becomes a **skinned mesh**: one
  `SkinWeight` per vertex — four `u16` joint indices into the skin's
  joint list and four `f32` weights, normalized (a vertex bound to
  nothing follows joint 0 with weight 1, so the shader can trust the
  sum). The cooked `MESH` payload stays the 72-byte-vertex layout and
  appends the 24-byte weights *after* the indices, so a static mesh's
  bytes are unchanged and a loader tells the two apart by length
  (`MeshPayload::is_skinned`); a joint index past the skin is `A0104`.
- every glTF skin becomes `skin/<name>` (`SkinData`, cooked as a `SKIN`
  chunk): the joints in the skin's order, each by node name with its
  parent when the parent is a joint of the same skin, its inverse bind
  matrix and its rest pose (the node's TRS), and the skeleton root's
  name (the skin's `skeleton` node, else empty). Two joints of one skin
  sharing a name is an import error (`A0104`; targets resolve by name),
  more than `max_joints` = 256 is `A0101`.
- every glTF animation becomes `anim/<name>` (`AnimationData`, cooked as an
  `ANIM` chunk): its translation/rotation/scale channels targeting nodes
  by name, step/linear/cubic-spline as authored (a cubic spline keeps its
  in-tangent, value, out-tangent triples), and the duration (the last
  key time over all channels). Morph-weight channels are skipped with a
  warning, like morph targets; a channel whose node name is not unique
  in the file is warned about (`A0105`). The animation's `extras.markers`
  become the clip's markers (§ Markers; a malformed entry is `A0104`).
- the prefab puts a **`fynkel.Skin`** next to the `fynkel.MeshRenderer`
  of a node that has a skin (`skin/<name>`, `skeleton` = a reference to the root node's entity);
  a skin on a node without a skinned mesh, or a skinned mesh on a node
  without a skin, is a warning and the mesh draws in its bind pose.

Node names are the contract, as above: the prefab's entities are named
after their nodes (an unnamed node gets `node<i>`), so an instantiated
prefab resolves its own clips.

Skins and animations entered the intermediate at `FYNI` version 4
(`skins` and `animations` next to `meshes`, the animations with their
markers) and the cache at version 5; both have moved on since (E1: `FYNI`
6, cache 6). The cooked container is unchanged
(`docs/documents.md` § Cache). The runtime store (`fynkel_assets`)
converts the payloads into the core's `Clip`/`Skin` on demand
(`Assets::clip`, `Assets::skin`) and `register_animation(scene, world)`
registers every clip an `Animator` and every skin a `Skin` of a loaded
scene or prefab names (the load report's `animation: { clips, skins }`);
`register_clip`/`register_skin` register one.

## The renderer

`docs/renderer.md` § Skinning has the detail. In short: a skinned
`GpuMesh` carries its weights in a second vertex buffer (slot 2:
`joints: vec4<u32>` at location 14, `weights: vec4<f32>` at 15); the
frame's extract computes, per skinned mesh with a resolved skin, the
**palette** `inverse(mesh_world) · joint_world · inverse_bind` per joint
from the joints' *interpolated* world matrices (so a posed character is
as smooth between ticks as any moving entity), appended to one
`Extract.joints` list; the renderer uploads it to a read-only storage
buffer of `mat4x4` at bind group 2 and each skinned instance carries its
first joint's index in `Instance.emissive.w`. `vs_skinned` computes
`pose = Σ wᵢ · joints[base + jᵢ]` and `model = instance_model · pose`,
then shares the static path's `vertex()`; the shadow pass has the same
entry, so a skinned mesh casts its posed shadow. Skinned instances still
batch by `(mesh, material, skinned)`. A skinned mesh without a `Skin`
component, or with one the world does not hold, draws as cooked — its
bind pose — through the static pipeline. `FrameStats.skinned` and
`.joints` count them; `fynkel render --steps <n>` ticks the world `n`
times before drawing so a pose can be captured (the showcase's
`renders/m3-19.png` is the runner at `--steps 10`).

## From Dart

The reference game's Player, cut down (`examples/reference_game/lib/game/player.dart`).
Its Animator runs an Animator Controller (§ Animator Controllers), so the
code only sets parameters:

```dart
class Player extends Behaviour {
  @Expose() AssetRef wave = AssetRef.none;           // played in the UpperBody slot

  @override
  void fixedUpdate(FixedUpdateContext c) {
    final me = characterState()!;
    move(...);                                       // the controller moves the entity
    final jumped = Input.pressed('jump') && me.grounded;
    if (jumped) jump(jumpSpeed);
    final v = me.velocity;
    animation
      ..setFloat('speed', math.sqrt(v.x * v.x + v.z * v.z))   // Locomotion's blend space: idle → walk → run
      ..setBool('grounded', me.grounded && !jumped);           // Jump → Locomotion on landing
    if (jumped) animation.setTrigger('jump');                  // Locomotion → Jump
    if (Input.pressed('wave')) {
      animation.playOneShot(wave, slot: 'UpperBody');          // the arm over the walk (ADR-0033 § 2)
    }
  }

  @override
  void onAnimationMarker(AnimationMarker e) {        // step_l / step_r from whichever clip leads the blend
    if (e.name.startsWith('step')) state.footsteps.value += 1;
  }
}
```

Without a controller an animator is driven clip by clip — before
ADR-0032 the Player did it by hand: `animation.setParameter('speed', v)`
for the blend space, `animation.play(jumpClip, looping: false)` on the
jump and `animation.crossfade(locomotion, 0.15)` on landing.

`GameObject.animation` is an `AnimationControl`: `play(clip, {looping,
speed})` (a clip or a blend space), `crossfade(clip, seconds, {looping,
speed})`, `stop()`, `pause()`, `resume()`, `speed =`, `time =`, `weight =`,
`setParameter(name, value)` — each one `fynkel.cmd.Animate`
on the simulation lane through the object's `CommandBatch` (a pending
object is driven through its temp id, so a spawn, its `Animator` and a
`play` land in one tick) — and `state` (`AnimatorState { clip, time,
duration, speed, fadeRemaining, playing, looping, weight, parameter,
rootDelta, rootYaw, isFading, progress }`,
one `FYNKEL_QUERY_ANIMATOR`; `null` without an animator or before the
object is spawned), `isPlaying`. `Behaviour.animation` forwards to its
object. A rejection (`missingComponent`, `invalidPayload`, component
`Animator.componentId`) is a diagnostic at the tick like any other
command's, never an exception; `DiagnosticCode.clipUnavailable` (16) and
`skinUnavailable` (17) are the once-only reports above.

`onAnimationEnded(AnimationEnded { entity, clip })` and
`onAnimationMarker(AnimationMarker { entity, clip, name })` run on the
behaviours of the animator's object right after each tick's step, with
the contact and zone callbacks and before `afterPhysics`; the events are
also in `drainEvents()` for a host that reads the world raw.
`LoadReport.animation` (`AnimationAssets { clips, skins }`) says what a
load registered.

**With an Animator Controller** (ADR-0032, Unity's `Animator` API):

```dart
animation.setFloat('speed', v);            // SetFloat; also setInteger, setBool
animation.setTrigger('jump');              // SetTrigger; resetTrigger
animation.getFloat('speed');               // GetFloat; getInteger, getBool
animation.play('Jump');                    // Play(stateName): at once
animation.crossfade('Locomotion', 0.1);    // CrossFade(stateName, seconds)
final s = animation.stateOf(layer: 0);     // GetCurrentAnimatorStateInfo
s?.name; s?.normalizedTime; s?.isInTransition; s?.next;
```

- The setters are commands for the next tick. The getters read what
  this tick set, else the last tick's value.
- A wrong parameter name or type throws an `ArgumentError` that lists
  the parameters. Before the controller's first tick the SDK does not
  know them yet, so the engine checks the command instead (an
  `invalidPayload` diagnostic).
- `play` and `crossfade` take a state name (`String`, with `layer` and
  `offset`) or a clip (`AssetRef`, with `looping` and `speed`). The clip
  calls (`play(clip)`, `crossfade(clip, …)`, `time =`, `setParameter`)
  throw a `StateError` on an animator with a controller; the parameter
  calls throw one on an animator without.
- `stateOf({layer})` is an `AnimatorStateInfo { layer, name,
  normalizedTime, length, next, nextNormalizedTime, transitionProgress,
  isInTransition }` (`FYNKEL_QUERY_ANIMATOR_CONTROLLER`); during a
  crossfade `name` is the state fading out and `next` the one fading
  in. `layers` gives them all, `parameters` the
  `AnimatorParameter { name, kind, value }` list
  (`FYNKEL_QUERY_ANIMATOR_PARAMETERS`).
- `Behaviour.onAnimatorStateEntered(AnimatorStateEntered { entity,
  layer, state, from })` and `onAnimatorStateExited(AnimatorStateExited {
  entity, layer, state })` run after the step, after
  `onAnimationEnded` and before `onAnimationMarker` (Unity's
  `StateMachineBehaviour.OnStateEnter`/`OnStateExit`, on the object's
  own scripts).
- Code can set a controller on an Animator
  (`set(animator.copyWith(controller: ref))`); the native layer
  registers it and its motions from the cache.

Tests: `packages/fynkel/test/controller_test.dart`. The joints are objects: `findChild('hips')`,
`localTransform`, `worldTransform`, `get<Transform>()` and a transform
write (which the next tick's pose overrides for the properties the clip
keys — write the ones it does not, or a joint it does not key).

## Cost

Per animator per tick: one target-validity pass over the cached targets
(a `Name` read each; a blend space's per point), one sample per channel
(a binary search over the key times, then the interpolation), one
local-transform write per target that changed; a blend space between
two points doubles the sampling and adds one blend, a crossfade the
same again, a layer at `weight < 1` or `additive` one more blend per
entry, markers a scan of the clip's (sorted) markers over the tick's
interval, root motion two samples of the root's channels and the matrix
chain. Per skin per tick: one validity pass over the joints. Per skinned
mesh per frame: one palette (`joints` matrix products) in the extract
and one storage-buffer write for all of them together. No per-tick
allocation once targets are resolved (the pose and tick scratch vectors
live on the world). The reference game's player — 11 joints, 10 channels
in the run clip, one skin — costs ≈ 20 µs of `animation` per tick on an
M-series Mac (`fynkel test --headless`'s `systems:` line). **The item
19c gate** is the stress scene's 200 animated rigs (`benchmarks/stress/generate.py`:
the box character 200 times — 200 skins over 2 200 joints, the animators
by turns on the walk, the run and the locomotion blend space, every
fourth with an additive wave layer on its `spine` — with the 5 000
movers, 500 bodies and 300 behaviours around them): `benchmarks/m3/capture.py`
checks the animation system under 1 ms a tick for all of them, the
markers counted, and the tick's p50 under 4 ms with everything else on
(`benchmarks/m3/results/`).

## Not in V1

- The state machine as data (states = clips or blends, transitions on
  named parameters, Dart sets parameters only) and the timeline/sequencer
  document (tracks over named objects: transforms, component fields,
  events) — the next items on this core (blueprint §9).
- ~~Clip markers/events, layers, additive blending, root-motion
  extraction, blend spaces~~ — done as item **19c** (§ Blend spaces,
  § Layers, § Markers, § Root motion); 2D blending as blend trees
  (ADR-0033 step 1, § Blend trees). Still out: 2D blend spaces and
  other sync modes than phase, root motion from a layer or a
  crossfade's outgoing clip, retargeting between skeletons, morph targets, a node-graph
  editor.
- Compressed or quantized clips; a clip is cooked as `f32` keys.
- A skinned mesh collider following the pose (a `Collider` on a joint
  entity is the way).
- ~~Instancing a source's `prefab` sub-asset from a scene or
  `World.instantiate`~~ — done as item **19a** (below, § Instancing an
  imported character).

## A downloaded character (Mixamo, 2026-09-21)

The path was tried end to end with the Mixamo X Bot and three clips
downloaded **with skin** and **In Place** (the engine extracts no root
motion: a clip that moves the hips forward walks the skin away from its
`CharacterController`). Mixamo ships FBX; the importer reads glTF, so
Blender does one conversion, headless (`Blender -b --python
convert.py`): import the first FBX with its mesh, the others for their
actions only (delete their armatures), put every action on the one
armature as a muted NLA track named after the clip, export one GLB with
`export_animation_mode="NLA_TRACKS"` (one glTF animation per track),
`export_all_influences=False` (four bones per vertex — `JOINTS_0`/
`WEIGHTS_0` is all the importer reads), `export_apply=True`, Y up. The
result imported with no diagnostics — two skinned meshes, one skin of
65 `mixamorig:` joints (the names are unique, as the importer
requires), walk 1.0 s / run 0.567 s / jump 1.933 s, a prefab of 68
entities whose root carries Blender's centimetre scale (0.01) and the
Z-up rotation — cooked, ran headless (74 entities, 2 skinned meshes over
130 joints, 0 diagnostics), rendered upright at human height next to
the starter's cubes, and played from Dart (`play(walk)`, a 0.2 s
crossfade to `run`, `play(jump, looping: false)` ending at frame 117).
Keys are baked per frame at 30 fps, so a clip is a few hundred KB of
`f32`; fine for a handful. The one step done by hand that day was the
prefab document; item 19a removed it the same day (next section).

## Instancing an imported character (M3-19a)

A scene names the source and nothing else: `prefab: { asset: <glb id>,
sub: "prefab" }` on an entity instances the glTF's own node hierarchy —
its skinned meshes, its joints, its `Skin`s, as `fynkel import` derived
them (documents.md § Prefab documents › Imported hierarchies) — and an
`Animator` on that entity finds the joints by name below it. `fynkel
validate` imports the file in memory to check the instance and its
overrides (a material swap on `node/<path>/renderer`, a turn on the root's
transform), the cook expands the scene against the cooked source's
`prefab` chunk, the runtime spawns the same hierarchy from
`World.instantiate(AssetRef(glb, sub: 'prefab'))` (hosts.md § Prefabs at
runtime). `fynkel import --prefab <file.glb>` writes the hierarchy as an
editable `.prefab.json` for the cases that want hand-edited ids. Four
downloads with the Blender recipe above become a playable character
with `examples/reference_game/tools/playground.py --hero
assets/models/hero.glb` (the Player's `Rig` child instances the file,
turned to face -Z; its clips are named idle, walk, run and
jump; opt-in, never committed — rerun without `--hero` before a commit).
The same path was proven on rigs nobody here made: the Khronos `Fox`
(24 joints, Survey/Walk/Run), `CesiumMan` and `RiggedFigure` (19 joints
each) under `crates/fynkel_import/fixtures/khronos/` import, validate,
cook, run headless and render posed from a scene that names only the
source (`apps/cli/tests/check.rs::khronos_rigs_instance_from_the_source_alone`,
`crates/fynkel_import/tests/gltf.rs::khronos_sample_rigs_import_clean`).
What was still by hand — the FBX → GLB conversion and the "which file is
idle" bookkeeping — is item 19b: the FBX importer (below) and the rig
document (its second part).

## FBX straight in (M3-19b, part 1)

`fynkel.fbx` (documents.md § Import › FBX) reads the Mixamo download as
it is: `fynkel asset init Walking.fbx` and the file imports with no
diagnostics — `ufbx` converts the centimetres and axes, so the prefab's
root is plain (no 0.01 scale, no Z-up turn); two skinned meshes welded
to 15 901 and 12 473 vertices, skins of 64/65 `mixamorig:` clusters with
`mixamorig:Hips` as the skeleton, one clip `anim/mixamo.com` (the stack's name)
of 0.967 s (52 moving nodes; the empty `Take 001` stack gives no clip),
the red default material. The four downloads (Idle, Walking, Fast Run,
Jumping) each validate, cook, run headless (74 entities, 2 skins over
129 joints, 0 diagnostics) and render posed and upright from a scene
whose `Hero` carries an `Animator` on `anim/mixamo.com` and instances
`{ asset: <fbx id>, sub: "prefab" }` on a `Rig` child turned to face
−Z — the same scene shape as the glTF path. The Blender recipe above is
no longer needed for import; what a game still wants is one name per
clip across files, which is the rig document (part 2, below).

The importer is the glTF importer's twin, not a general FBX tool: the
same intermediate through the same code (`crates/fynkel_import/src/
graph.rs`), checked against the glTF path on the same content
(`fixtures/rig.fbx` and `robot.fbx` are `rig.glb`/`robot.glb` exported
from Blender by `fixtures/blender_fbx.py` with `bake_space_transform`,
so the vertex data is Y-up like Mixamo's and Maya's). Joints keep their
cluster order, `inverse_bind` is the cluster's bind matrix, clips are
baked per frame at the file's rate and named after their stacks, and
anything outside mesh + skin + clips + embedded textures + hierarchy is
a named `FYN-A0105` skip or an `A010x` refusal.

## Four downloads, one character: the rig document (M3-19b, part 2)

Mixamo hands out one FBX per clip, each with the whole character in it.
The **rig document** (`<name>.rig.json`; documents.md § Rig documents)
says which file is the character and what each clip is called:

```sh
fynkel import Idle.fbx Walking.fbx "Fast Run.fbx" Jumping.fbx --rig hero
#   idle      Idle.fbx#anim/mixamo.com      1.97 s  156/156 channel(s)
#   walking   Walking.fbx#anim/mixamo.com   0.97 s  156/156 channel(s)
#   fast_run  Fast Run.fbx#anim/mixamo.com  0.53 s  156/156 channel(s)
#   jumping   Jumping.fbx#anim/mixamo.com   1.90 s  156/156 channel(s)
# hero.rig.json: created (4 clip(s), skeleton Idle.fbx)
```

The first file is the **skeleton** — its meshes, skins and joints are
the character —, every file's clips become entries named after the
files (edit the names in the document: `"walk"`, `"run"`, `"jump"`),
and the rig's sub-assets are `prefab` (the skeleton's hierarchy) and
`anim/<name>` per clip. A scene instances `{ asset: <rig>, sub:
"prefab" }` and its `Animator` plays `{ asset: <rig>, sub: "anim/walk"
}`; from Dart the clips are `AssetRef(rig, sub: 'anim/run')` — names,
not the index a re-export could shuffle. Clips are matched to the
skeleton **by bone name** at cook (and by `fynkel rig inspect|validate`
and `import --rig` before that): Mixamo names every bone `mixamorig:…`
identically across downloads of one character, so all 156 channels of
each file find their joint; a clip downloaded for another character
has bones the skeleton lacks and is refused with `FYN-A0106` naming the
first of them (the Khronos `CesiumMan` clip on the `RiggedFigure`
skeleton: 27 of 57 channels match, 10 bones missing) — extra bones in
the skeleton are fine, a clip file's own meshes are ignored. The cooked
rig is one object (the hierarchy and one `ANIM` per clip) whose runtime
closure is the rig and the skeleton file: the clip files stay on the
authoring side.

Acceptance on content nobody here made: the Khronos `Fox` through
`import --rig` (skeleton and Survey/Walk/Run from the one file, 21/21
channels each), `rig inspect|validate`, a scene on `<rig>#prefab` and
`anim/run` that cooks, runs headless (24 joints) and moves `b_Neck_04`
(`apps/cli/tests/check.rs::khronos_rigs_assemble_into_a_rig_document`);
the fixture project's Column rig (`crates/fynkel_import/tests/cook.rs`,
`crates/fynkel_assets/tests/store.rs`: cook, cache invalidation per
file, the runtime reading `prefab` and `anim/wave` off the rig); and the
user's four Mixamo files as `hero.rig.json` with the one command above
— `rig inspect` 65 joints / 67 entities, every clip 156/156 —, the
reference game on it (`tools/playground.py --hero
assets/models/hero/hero.rig.json`: the Player's `Rig` child instances
`<rig>#prefab`, `game.Player` is told `anim/idle` … by name), the
recorded route replayed (233 entities, 2 skins over 129 joints, 0
diagnostics), the Dart tests green and the Playground rendered with the
X Bot in its idle. The box character stays the committed default.

## The character, complete (M3-19c)

With the four features the reference game's Player stops choosing clips.
`tools/playground.py` writes `assets/models/player.blend.json` — the
character's idle at 0, walk at 4, run at 7 m/s along `speed` — and puts
the Player's `Animator` on it; `game.Player` sets `speed` every tick from
the controller's planar velocity (so a wall stops the stride with the
player), plays the jump once over the space and crossfades back on
landing (since ADR-0032 an Animator Controller does the jump and the
landing; § Animator Controllers), plays the **wave layer** — a second, additive `Animator` on the
`spine` joint (the character's `anim/wave`, keying the right arm alone,
paused on its first frame so it adds nothing at rest) — on the `wave`
action, and counts the walk's and the run's `step_l`/`step_r` markers
(`tools/character.py` writes them into the glTF's `extras`) into the
HUD's `steps`. `--hero <name>.rig.json` swaps the character: the space's
points are the rig's `anim/idle` … , its `markers` give the footsteps,
a `wave` clip on the rig (optional) gives the layer on the `Rig`.
Acceptance on content nobody here made: the Khronos Fox's survey →
walk → run by `speed` (`apps/cli/tests/check.rs`: validated, cooked, run
headless; at `speed` 1.5 every `b_*` joint's local transform is the
midpoint — lerp/slerp — of the walk's and the run's at the same phase,
and a change of parameter keeps the phase), and the user's Mixamo hero
in the Playground on the blend space with footstep markers from its rig
document (the Dart tests green with `--hero`; no wave clip in that
set, so no layer there). The 200-rig stress gate is under § Cost.
