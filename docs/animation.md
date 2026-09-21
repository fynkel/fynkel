# Animation: the clip core, skinning and the `Animator`

The animation contract: blueprint §9 ("Animation"), §19.2d item 19
(M3-19, built 2026-09-21), §5.1 (where in the tick it runs). One clip
core, built once, with every front door on it: a rigged character's walk,
a door's swing and a lamp's bob are the same thing to the sampler, and
the follow-ups (the state machine as data, the timeline/sequencer
document, clip markers) sit on the same sampler with no second one.

| Piece | Where | What |
| --- | --- | --- |
| the clip core | `crates/fynkel_core/src/animation.rs` | `Clip` = channels keying one property of one **named** node; `Clip::sample(time)` → `Pose`; `Pose::blend`; `Skin` = named joints with inverse bind matrices |
| the systems | `crates/fynkel_core/src/world/animation.rs` | `run_animation` in the pre-tick slot: every `fynkel.Animator`'s clock, crossfade and pose write; every `fynkel.Skin`'s joint lookup; `skin_palette` for the extract |
| the components | `fynkel.Animator` (id 21), `fynkel.Skin` (22) | ordinary native components (`docs/codegen.md`): authored, validated, generated into every codec, reachable from Dart on the M3-07a path |
| the import | `crates/fynkel_import/src/gltf.rs` (importer version 4) | glTF skins → `skin/<s>`, animations → `anim/<a>`, `JOINTS_0`/`WEIGHTS_0` → a skinned `mesh/<n>`; the prefab gets the `fynkel.Skin` |
| the cache | `crates/fynkel_cooked/src/payload.rs` | `SKIN` and `ANIM` chunks; a skinned `MESH` carries a trailing `SkinWeight` section |
| the renderer | `crates/fynkel_render` (`docs/renderer.md`) | the skinned vertex path: a joint palette storage buffer and `vs_skinned` in five pipeline variants |
| the wire | `fynkel.cmd.Animate` (kind 9), `fynkel.ev.AnimationEnded` (9), `fynkel.rec.AnimatorState`, `FYNKEL_QUERY_ANIMATOR` (11) | play/crossfade/stop/pause/resume/speed/time; the one-shot's end; the state read-back |
| Dart | `packages/fynkel/lib/src/animation.dart`, `world.dart` | `GameObject.animation` → `AnimationControl`; `Behaviour.animation`, `onAnimationEnded`; `AnimatorState`; `LoadReport.animation` |

Tests: `crates/fynkel_core/tests/animation.rs` (the sampler, blending,
the animator's clock, the skin's palette), `crates/fynkel_import/tests/gltf.rs`
(the rig fixture), `crates/fynkel_render/tests/project.rs`
`skinned_meshes_follow_their_joint_palette`, `crates/fynkel_native/tests/abi.rs`,
`packages/fynkel/test/animation_test.dart`, the reference game's
`test/game_test.dart` (the character posed from the controller). The
fixture is `crates/fynkel_import/fixtures/rig.glb` (written by `rig.py`):
a skinned column on three joints under a "wave" clip, a static Prop under
a one-shot "bob"; the document fixture project's `Rig` scene
(`c0000000-…-000034`) plays both.

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
| `fynkel.Animator` | 21 | `clip` (an `anim/<a>` asset ref; nil plays nothing), `speed` (finite; negative plays backwards), `looping`, `playing` | plays `clip` over the named nodes under this entity; `speed` non-finite is `INVALID_PAYLOAD` (`FYN-H0008` at `fynkel validate`) |
| `fynkel.Skin` | 22 | `skin` (a `skin/<s>` asset ref), `skeleton` (the name of the joint root entity; empty: the tree's root) | next to the `fynkel.MeshRenderer` of a skinned mesh: the joints are found by name under `skeleton`, itself found by name from the entity's tree root |

Both are ordinary native components: authored in a scene or prefab,
validated, listed in `fynkel.project.json` → `components`, `get<Animator>()`
/ `add(const Skin(…))` / `set` / `remove<Animator>()` from Dart. The
runtime state each needs lives in companion components that are never
authored or on the wire: `AnimatorRun { time, targets, fade, fresh, seek,
reported }` and `SkinRun { root, joints, resolved, reported }`, attached
on the first tick the component is seen and detached with it.

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
where the pose put it before Rapier sees it. Every animator, in entity
order:

1. **The clip.** A nil `clip` poses nothing. A clip the world does not
   hold reports `CLIP_UNAVAILABLE` (16) once per clip per animator and
   the animator waits (`duration` reads 0); the report repeats for the
   next unknown clip.
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
   prefab never has).
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
   entry that changes nothing writes nothing.

Then every `fynkel.Skin`: the skeleton root is found by name from the
mesh entity's tree root (`skeleton` empty: the tree root itself), the
skin's joints by name under it, cached while every one still lives under
its name. A skin the world does not hold reports `SKIN_UNAVAILABLE` (17)
once and the mesh draws in its bind pose; so does a joint that did not
resolve (its palette entry is the identity).

`World::systemTimes()` reports the pass as `SystemKind.animation`
(entities = animators + skins); `fynkel test --headless` prints an
`animation:` line — animators + skins, clips and skins registered,
skinned meshes with their joint count, one-shots ended, diagnostics.

**Root motion, the V1 policy (item 19 (d)).** Clips extract no root
motion. A `CharacterController` (or the game) moves the entity; the
animator only poses nodes *under* it, so clips are authored **in place**
— the reference game's walk and run cycle the legs while the controller
carries the player, and a jump's hips dip and rise while the controller's
own jump lifts the entity. A clip that does key the animator entity
itself would fight whatever else moves it; nothing stops it, nothing
compensates for it.

## Commands, the query and the event

`fynkel.cmd.Animate` (command kind 9, simulation lane, 48 bytes:
`{ entity, op, seconds, speed, looping, clip }`) carries one `AnimateOp`:

| `op` | Effect | Rejection |
| --- | --- | --- |
| `play` | switch to `clip` from its start (its end for `speed < 0`), `looping`/`speed` as given, `playing` | |
| `crossfade` | like `play`, the current clip blended out over `seconds` | `seconds < 0` or non-finite → `INVALID_PAYLOAD` |
| `stop` | rewind to 0 and clear `playing`; the entities keep their last pose | |
| `pause` / `resume` | clear / set `playing`, the time kept | |
| `setSpeed` | `speed` | non-finite → `INVALID_PAYLOAD` |
| `setTime` | seek to `seconds` (clamped to the clip) on the next tick, without advancing | negative or non-finite → `INVALID_PAYLOAD` |

An entity without an `Animator` is `MISSING_COMPONENT` (component 21).
The native layer (`fynkel_native`) registers from the cache, before the
batch is applied, any clip a `play`/`crossfade` names that the loaded
scene never did, and the clip or skin of an `Animator`/`Skin` a behaviour
`set`s — so a behaviour plays `anim/3` of a character whose scene only
authored `anim/0` without listing every clip somewhere; a clip the cache
cannot read stays unregistered and the animator reports
`CLIP_UNAVAILABLE` at the tick.

`FYNKEL_QUERY_ANIMATOR` (11) with an entity answers one
`fynkel.rec.AnimatorState { entity, time, duration, speed, fade_remaining,
playing, looping, clip }` (56 bytes) as the last step left it, or no
record for an entity without an animator.

`fynkel.ev.AnimationEnded { entity, clip }` (event kind 9, general lane)
is raised the tick a one-shot passes its end (`playing` is cleared the
same tick); a looping clip never raises it, and neither does `stop`.

## Import and the cache

The glTF importer (`fynkel.gltf` **version 4**; `docs/documents.md`)
keeps what a rigged source carries:

- a primitive with `JOINTS_0`/`WEIGHTS_0` becomes a **skinned mesh**: one
  `SkinWeight` per vertex — four `u16` joint indices into the skin's
  joint list and four `f32` weights, normalized (a vertex bound to
  nothing follows joint 0 with weight 1, so the shader can trust the
  sum). The cooked `MESH` payload stays the 72-byte-vertex layout and
  appends the 24-byte weights *after* the indices, so a static mesh's
  bytes are unchanged and a loader tells the two apart by length
  (`MeshPayload::is_skinned`); a joint index past the skin is `A0104`.
- every glTF skin becomes `skin/<s>` (`SkinData`, cooked as a `SKIN`
  chunk): the joints in the skin's order, each by node name with its
  parent when the parent is a joint of the same skin, its inverse bind
  matrix and its rest pose (the node's TRS), and the skeleton root's
  name (the skin's `skeleton` node, else empty). Two joints of one skin
  sharing a name is an import error (`A0104`; targets resolve by name),
  more than `max_joints` = 256 is `A0101`.
- every glTF animation becomes `anim/<a>` (`AnimationData`, cooked as an
  `ANIM` chunk): its translation/rotation/scale channels targeting nodes
  by name, step/linear/cubic-spline as authored (a cubic spline keeps its
  in-tangent, value, out-tangent triples), and the duration (the last
  key time over all channels). Morph-weight channels are skipped with a
  warning, like morph targets; a channel whose node name is not unique
  in the file is warned about (`A0105`).
- the prefab puts a **`fynkel.Skin`** next to the `fynkel.MeshRenderer`
  of a node that has a skin (`skin/<s>`, `skeleton` = the root's name);
  a skin on a node without a skinned mesh, or a skinned mesh on a node
  without a skin, is a warning and the mesh draws in its bind pose.

Node names are the contract, as above: the prefab's entities are named
after their nodes (an unnamed node gets `node<i>`), so an instantiated
prefab resolves its own clips.

The intermediate is `FYNI` version 3 (`skins` and `animations` next to
`meshes`), the cache version 4; the cooked container is unchanged
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

```dart
class Player extends Behaviour {
  late AssetRef idle, walk, jump;                      // AssetRef(character, sub: 'anim/<n>')

  @override
  void start() {
    final character = get<Animator>()!.clip.asset;     // the asset the scene's animator names
    idle = AssetRef(character, sub: 'anim/0');
    walk = AssetRef(character, sub: 'anim/1');
    jump = AssetRef(character, sub: 'anim/3');
  }

  @override
  void fixedUpdate(FixedUpdateContext c) {
    final me = characterState();
    if (Input.pressed('jump') && me!.grounded) {
      jump(jumpSpeed);
      animation.play(jump, looping: false);            // a one-shot
    } else if (me!.grounded) {
      final next = Input.action2D('move').length > 0.05 ? walk : idle;
      if (animation.state?.clip != next) animation.crossfade(next, 0.15);
    }
  }

  @override
  void onAnimationEnded(AnimationEnded e) {            // the one-shot passed its end
    if (e.clip == jump && characterState()!.grounded) animation.crossfade(idle, 0.1);
  }
}
```

`GameObject.animation` is an `AnimationControl`: `play(clip, {looping,
speed})`, `crossfade(clip, seconds, {looping, speed})`, `stop()`,
`pause()`, `resume()`, `speed =`, `time =` — each one `fynkel.cmd.Animate`
on the simulation lane through the object's `CommandBatch` (a pending
object is driven through its temp id, so a spawn, its `Animator` and a
`play` land in one tick) — and `state` (`AnimatorState { clip, time,
duration, speed, fadeRemaining, playing, looping, isFading, progress }`,
one `FYNKEL_QUERY_ANIMATOR`; `null` without an animator or before the
object is spawned), `isPlaying`. `Behaviour.animation` forwards to its
object. A rejection (`missingComponent`, `invalidPayload`, component
`Animator.componentId`) is a diagnostic at the tick like any other
command's, never an exception; `DiagnosticCode.clipUnavailable` (16) and
`skinUnavailable` (17) are the once-only reports above.

`onAnimationEnded(AnimationEnded { entity, clip })` runs on the
behaviours of the animator's object right after each tick's step, with
the contact and zone callbacks and before `afterPhysics`; the event is
also in `drainEvents()` for a host that reads the world raw.
`LoadReport.animation` (`AnimationAssets { clips, skins }`) says what a
load registered. The joints are objects: `findChild('hips')`,
`localTransform`, `worldTransform`, `get<Transform>()` and a transform
write (which the next tick's pose overrides for the properties the clip
keys — write the ones it does not, or a joint it does not key).

## Cost

Per animator per tick: one target-validity pass over the cached targets
(a `Name` read each), one sample per channel (a binary search over the
key times, then the interpolation), one local-transform write per target
that changed; a crossfade doubles the sampling and adds one blend. Per
skin per tick: one validity pass over the joints. Per skinned mesh per
frame: one palette (`joints` matrix products) in the extract and one
storage-buffer write for all of them together. No per-tick allocation
once targets are resolved (the pose and tick scratch vectors live on the
world). The reference game's player — 11 joints, 10 channels in the run
clip, one skin — costs ≈ 20 µs of `animation` per tick on an M-series
Mac (`fynkel test --headless`'s `systems:` line).

## Not in V1

- The state machine as data (states = clips or blends, transitions on
  named parameters, Dart sets parameters only) and the timeline/sequencer
  document (tracks over named objects: transforms, component fields,
  events) — the next items on this core (blueprint §9).
- Clip markers/events, layers and masks per animator (the core's `Pose`
  blends per channel; nothing exposes a mask yet), additive blending,
  IK, retargeting, root-motion extraction, morph targets, a node-graph
  editor.
- Compressed or quantized clips; a clip is cooked as `f32` keys.
- A skinned mesh collider following the pose (a `Collider` on a joint
  entity is the way).
- Instancing a source's `prefab` sub-asset from a scene or
  `World.instantiate` — blueprint item **19a** (after audio, before the
  reference game's second half). Until then the hierarchy is authored
  by hand (`playground.py`) or the `prefab` body is copied out of the
  intermediate (`.fynkel/cache/import/<id>.fyni`: `FYNI` magic, `u32`
  version, `u32` header length, JSON header with `prefab`) into a
  `.prefab.json` with a fresh id.

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
`f32`; fine for a handful. The one step done by hand was the prefab
document above (item 19a).
