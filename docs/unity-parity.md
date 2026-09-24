# Unity parity

What a Unity user does in their first hours, what Fynkel does today, and
the order the gaps are closed in. Started 2026-09-24 after the gate
session showed features that pass their tests but do not behave the way
a Unity user expects (the prefab drag, the Hierarchy reorder, the
volume's weight). Unity is the reference (see `docs/editor.md`): where
Fynkel differs on purpose, the row says why.

Every row that is fixed gets a test that drives the **whole editor over
a real project** the way a person does — clicks, shortcut keys, typing,
drags — and checks what they would look at (`apps/editor/test/parity/`,
`harness.dart`). A look setting's test checks that turning it on changes
the picture by a visible amount, not that the number arrived.

Status: ✅ works as in Unity · 🟡 works but differs · ❌ missing or broken
· 🔧 fixed in this pass (with the commit).

## Tier 1 — what gets in the way every day

### Hierarchy

| | Unity | Fynkel |
|---|---|---|
| Reorder scene roots by dragging | drag between any two rows | 🔧 `14108b9` `c5a7248` (several rows keep their order; the order is saved) |
| ⌘D Duplicate | copy right below, named `Cube (1)`, selected | 🔧 `14108b9` `c5a7248` |
| ⌘C / ⌘X / ⌘V, Paste as Child ⌘⇧V | copy objects, also across scenes | 🔧 `pasteObjects` (`14108b9`), `c5a7248` |
| Create Empty | ⌘⇧N at the root; ⌥⇧N as a child | 🔧 `c5a7248` (named `GameObject`; New Project has no key now) |
| GameObject → 3D Object / Light / Camera | a visible cube, sphere, plane…, a light, a camera | 🔧 ADR-0023, `033df72` `installPrimitives`, `89889b6` (the menu bar and the Hierarchy's menus cascade; the first shape adds `Fynkel/Primitives.glb`) |
| A child made from + | its name can be typed at once | 🔧 `89889b6` (the new row took no focus) |
| Double-click a row | frames it in the Scene view | 🔧 `c5a7248` |
| Rename after Esc | F2 shows the real name | 🔧 `c5a7248` |
| Undo | brings the selection back | 🔧 `eaf9a80` (redo too) |
| Delete | clears the selection | 🔧 `c5a7248` |
| Right-click empty space | Create menu | 🔧 `eaf9a80` (Paste, Create Empty, Expand/Collapse All; rows gain Cut/Copy/Paste) |
| ⌥-click the arrow | expands everything below | 🔧 `eaf9a80` |
| Light rows | light icon | 🔧 `c5a7248` |
| Delete ⌘⌫, rename F2/Enter, Esc cancels, reparent by drag (keeps world position), unparent to empty space, ⌘/⇧-click, arrows, search, active greyed | | ✅ |

### Inspector

| | Unity | Fynkel |
|---|---|---|
| Edit rotation with several objects selected | changes that axis on each | 🔧 `b5a9c11` (it overwrote the other axes with the first object's) |
| Drag a value (label scrub) | the Scene view follows live | 🔧 `b5a9c11` (Transform values; one undo on release) |
| A value that does not parse | goes back to the old value | ✅ (Unity does the same) |
| Component header | enable checkbox, Copy/Paste Values, Move Up/Down, drag to reorder | 🔧 checkbox + Copy Component / Paste Component Values (`13d2876`); Move Up / Move Down (`moveComponent` `8b8e747`, `5d36af3`); drag a header onto another to reorder (`7d531c3`) |
| Click an asset reference | pings the asset in the Project | 🔧 `2455f5d` (⊙ opens the picker) |
| Colour picker | live while picking, HDR intensity | 🔧 live in the Scene view, one undo on OK, Cancel restores (`ff6611e`, `7edb455`); HDR intensity goes with the Material Inspector (no component colour is HDR; emission is) |
| Rotation after reselect/undo | keeps the typed Euler | 🔧 `10e1f8b` (editor-side hints) |
| Typing, maths, Enter/Esc, drag-scrub with ⇧/⌥, one undo per edit, enums, sliders, tooltips, lock | | ✅ |

### Play

| | Unity | Fynkel |
|---|---|---|
| ⌘P | toggles Play/Stop | 🔧 `b5a9c11` (⌘. still stops) |
| ⌘⇧P / ⌘⌥P | Pause / Step | 🔧 `b5a9c11` (the Command Palette moved to ⌘K, asset search to ⌥⌘B) |
| Start paused | yes | 🔧 `8d9e277` |
| Changes during Play revert on Stop, tint | | ✅ |

### Project

| | Unity (macOS) | Fynkel |
|---|---|---|
| Enter | renames | 🔧 `16e16ab` (⌘↓ opens) |
| ⌘⌫ | deletes after a confirmation | 🔧 `16e16ab` |
| ⌘D | duplicates the asset | 🔧 `duplicateAsset` (`52bde19`), `16e16ab` |
| Undo | ⌘Z undoes the scene; asset operations are not undoable | ✅ ⌘Z undoes the focused document, as in Unity; asset operations also have their own ⌥⌘Z history (more than Unity), except that creating or deleting an empty folder can't be undone |
| Drag files in from Finder | imports | 🔧 `14811dd`, `395220e` (into the shown folder, one ⌥⌘Z step) |
| Model sub-assets | expand in the browser, draggable | 🔧 `554ae0d` (▸, → / ←, a part's page on click) |
| Create → Volume Profile, Physics Material | | 🔧 Volume Profile (`682f953`, `adb5146`); Physics Material (`eee5782`, ADR-0025; `089773d` editor: the + menu, the folder menus and Assets ▸ New Physics Material) |
| Find References In Scene | | 🔧 `fc22546` (the Hierarchy searches `ref:<id>:<name>`) |

## Tier 2 — the look and the code

| | Unity | Fynkel |
|---|---|---|
| Material Inspector | edit colour, maps, emission (HDR colour + intensity); preview sphere; live in the Scene view | 🔧 `dcd42ba`, `27f122f` (saved at once, live while picking or dragging, ⌘Z in the Inspector); no preview sphere yet; a custom-shader material stays read-only |
| Drop a material on an object | assigns it | 🔧 `6203b47`, `b6ee085` (Hierarchy row or Scene view; drawn while hovering; an override inside a prefab instance) |
| Volume profile Inspector | Add Override, a checkbox per field | 🔧 `b1b7402`, `d2fa8ea` (Remove, All / None; saved at once, live in the Scene view, ⌘Z in the Inspector) |
| Model import settings | scale, normals, animation… | 🔧 `41190a1`, `c3e7d3e` (Scale Factor, Normals Import / Calculate + Smoothing Angle, Import Animation; leaving with unapplied settings asks Apply / Revert); 🟡 no Rig or Animation tab beyond that (clips are split by the rig document), no mesh compression / read-write / lightmap UVs |
| `transform.position` (world), `Translate`, `Rotate`, `LookAt`, `forward`, `eulerAngles` | | 🔧 `2e0183f` (`position` / `rotation` are world, `localPosition` / `localRotation` / `localScale`, `translate` / `rotate` / `rotateAround` / `lookAt` with `Space.self` / `Space.world`, `forward` / `right` / `up`, `eulerAngles` in degrees, `transformPoint` and friends); 🟡 Euler order is the Inspector's X-then-Y-then-Z, not Unity's Z-X-Y |
| One script throwing | only that script stops | 🔧 `faf648d` (every callback on its own; the Console gets the message and the stack; called again next frame) |
| Coroutines / `Invoke` | | 🔧 `b92178e` (`startCoroutine` over a `sync*` generator; `WaitForSeconds`, `WaitUntil`, `WaitWhile`, `WaitForFixedUpdate`, nested routines; `invoke`, `invokeRepeating`, `cancelInvoke`; Unity's lifetimes) |
| `Input.GetKey` | raw keys | 🔧 `6b25b52` (ADR-0024: `getKey` / `getKeyDown` / `getKeyUp`, `getMouseButton…`, `anyKey`, with the actions' tick rules; an undeclared action warns once in the Console) |
| `Debug.Log` | Console, click to the line | 🔧 `9bdf3d6` (`Debug.log` / `logWarning` / `logError` / `logException` with the caller's stack: the row shows the line, ↩ opens it; `print` in game code is a Console row) |
| Inspector fields | public fields; GameObject/component/colour/enum references | 🔧 `46c091a`, `de1de2c`, `962d30a` (ADR-0018 amended: `GameObject?` and script-typed fields — `None (Type)`, drop a Hierarchy row, ⊙, ×, `Missing` — resolved at load whatever the attach order, a prefab instance's to its own copies; Dart enums as a dropdown; colours were already `@Expose(kind: 'color')`); 🟡 fields still need `@Expose` (no implicit public fields), no asset-typed fields beyond `AssetRef`, no `List` of references |
| A failed hot reload | blocks Play, errors in the Console | 🔧 `68f7d45` (each `file:line:column: Error:` is a Console row, ↩ opens the line in the IDE; Play and Build And Run refused until a reload goes through; the notice links the Console); 🟡 the old code keeps running in the game view meanwhile, as Unity keeps the old assemblies |
| Drag a script onto an object | adds it | 🔧 `888092b` (a behaviour's file in Scripts drags it — of several, the one named after the file; onto a Hierarchy row, the Inspector for the selection, or the object under the pointer in the Scene view; schema defaults, an override on a prefab instance, one undo step, refused where it is already); 🟡 an entity still holds one of each script (FYN-D0204), Unity allows several; a file without a behaviour does not drag rather than saying why |

## Tier 3 — the Scene view and prefabs, beyond the basics

| | Unity | Fynkel |
|---|---|---|
| Pivot / Center (Z) | | 🔧 `51a245a` (the gizmo at the active object's origin, each turning and scaling about its own, or at the selection's bounds centre, children included, the group about it; a scale multiplies each object's own, no shear) |
| Click a prefab, click again | root, then the child under the pointer | 🔧 `7a66084` (root, then the child under the pointer, then what is behind; inside a selected instance a click goes straight to the child) |
| ⇧F / double-F | follow the selection | 🔧 `5c57adb` (Lock View to Selected: the view follows the selection dragged or in Play; orbit and zoom keep the lock, pan, fly, framing or another selection let go; Frame All stays in the View menu) |
| Selection outline | silhouette | 🔧 `6344488`…`4b493b7`, `70ad2db` (the renderer rings what the selection draws — a mask of the outlined instances, then a composite before the overlay lines, ABI 8 — in Unity's orange, what is under it in its grey-blue: children, an instance's parts; an object without a mesh keeps its cross); rung through what hides it, as Unity's (its issue tracker counts an occluded outline as a regression) |
| Rect tool (T), free-rotate trackball | | 🔧 `510d218` (the rectangle in the plane of the two axes most across the view, children included: corners about the opposite one, ⌥ about the centre, ⇧ in proportion; edges one way; inside moves; just outside a corner turns; a click inside selects what is under it. Inside the rotate rings a drag turns freely, the rings first) |
| Prefab mode | in context, breadcrumb, auto-save | 🔧 `20edc8e` (an instance's › or Open opens its prefab in context; the Scene view's bar: ‹, the breadcrumbs, Context Normal / Gray / Hidden, Auto Save on by default or a Save button; the Hierarchy's ‹ header; from the Project, in isolation; leaving with unsaved changes asks); `a95f43b`: the scene drawn around it through the viewport's extra worlds — `965d4b7`…`1a023c4`, ABI 9 — placed so the prefab's root sits where the instance was, the instance hidden, greyed in Gray) |
| Prefab variants, Unpack Completely, a prefab from an object holding instances | | 🔧 `9146759`…`3fa1469` (ADR-0022 amendment, accepted), `9241460` editor |
| Overrides dropdown, blue margin bar | | 🔧 `7d51553` (the Prefab bar — Open, Select, Overrides ▾ — lists the overrides by object, a row comparing the prefab's value with the instance's, Revert / Apply, Revert All / Apply All; the blue bar and Apply / Revert on a right-click; an added component's '+' and Apply / Revert Added Component; Unpack stays in the Hierarchy menu) |
| Multi-scene Hierarchy | | 🔧 `f632f96`, `ca34f28` (every open scene a section under its header — the active one bold, Set Active / Save / Unload / Load / Remove / Select Scene Asset; Open Scene Additive or a scene dropped on the Hierarchy adds one, a double-click opens one alone after asking; a click on another scene's row or object in the Scene view activates it; rows dragged onto another scene move there; one Scene view camera); 🟡 the selection is one scene's at a time (Unity's may span scenes); `a95f43b`: the other loaded scenes drawn in the Scene view, lit and shadowed with the active one) |
| Physics Material asset | shared friction/bounciness, combine modes | 🔧 `eee5782`, `797c4b8` (ADR-0025, accepted: `.physicsmaterial.json` with `friction`, `bounciness`, `friction_combine` / `bounce_combine` in Unity's priority, the Collider's `material`, hot reload of live colliders); `089773d` editor (its Inspector page, the Collider's Material picker, the collider's own Friction and Bounciness hidden while one is set); 🟡 one friction coefficient, as Rapier has no static friction |
| Grid thumbnails, zoom slider | | 🔧 `8da4595` (the slider at the status bar's right, remembered; tiles act like rows; models, prefabs, materials and a model's meshes and materials drawn by `fynkel thumbnail` in the background, again when their file changes) |

## On purpose

- `Vec3.forward` is −Z (right-handed, as glTF), not Unity's +Z.
- Euler angles are degrees (as in Unity) but every other angle is
  radians (`Quat.axisAngle`), per ADR-0013's amendment.
- A post effect needs a volume (as Unity's URP/HDRP): no volume, no
  ambient occlusion or bloom (2026-09-24).

## Log

- 2026-09-24: list made from four audits of the editor and SDK plus the
  first parity tests.
- 2026-09-24: Hierarchy basics (`14108b9`, `c5a7248`) —
  `test/parity/hierarchy_parity_test.dart`, 15 tests over the fixture
  project.
- 2026-09-24: Inspector rotation for several objects, live label drags,
  Unity's Play keys (`b5a9c11`) — `test/parity/inspector_parity_test.dart`.
- 2026-09-24: Start paused (`8d9e277`); the Project browser's keys and
  Duplicate (`52bde19`, `16e16ab`, `test/parity/project_parity_test.dart`);
  the Hierarchy's undo selection, ⌥-click and empty-space menu
  (`eaf9a80`); the Euler hint (`10e1f8b`), Copy/Paste Component Values
  and the header checkbox (`13d2876`), the asset ping (`2455f5d`).
- 2026-09-24: ADR-0023 built-in primitives and GameObject presets
  (`ad6a6ca`, `033df72`, `89889b6`) —
  `test/parity/gameobject_parity_test.dart` renders the saved scene with
  `fynkel render` and checks that the cube is drawn.
- 2026-09-24: Move Up / Move Down (`8b8e747`, `5d36af3`) — the parity
  test reads the cards' order on screen and in the saved file.
- 2026-09-24: the colour picker is live (`ff6611e` preview `setComponents`,
  `6b8ffe7`, `7edb455`) — `inspector_parity_test.dart` checks the world's
  light colour while picking, Cancel, and one undo on OK.
- 2026-09-24: New Volume Profile (`682f953`, `adb5146`) —
  `project_parity_test.dart` makes one from + and renames it.
- 2026-09-24: Find References In Scene (`fc22546`) — the parity test
  finds a model's users (one of them inside a prefab instance) and a
  prefab's instance.
- 2026-09-24: a model's parts in the Project (`554ae0d`) — the parity test
  opens robot.glb, drags a mesh onto the Robot's Mesh field and saves.
- 2026-09-24: files dragged in from Finder (`14811dd` `importFiles`,
  `395220e`) — the parity test sends a drop through the channel and
  finds the file, its sidecar and one undo step. Undo reclassified: Unity
  doesn't undo asset operations either, so ⌘Z on the focused document
  matches.
- 2026-09-24: the material Inspector (`dcd42ba` preview `setMaterial`,
  `27f122f`) — the parity test types Metallic, drags the Roughness
  slider (drawn during the drag, written on release), switches to Cutout,
  clears the Base Map, undoes all four with ⌘Z and redoes one; a second
  test picks a Base Color (live, Cancel writes nothing, OK once). Every
  Inspector slider is live now, a component's included.
- 2026-09-24: a material dropped on an object (`6203b47` the pick's
  `object`, `b6ee085`) — the parity tests drop one on the Robot's row, on
  Crate A's (an override) and on the Sun's (refused), and drag one over
  the Scene view: the Robot draws with it while hovering, the drop is one
  undo step.
- 2026-09-24: the Volume Profile Inspector (`b1b7402` profiles edit
  through document fields and preview `setVolume`, `d2fa8ea`) —
  `volume_parity_test.dart` unticks Bloom's Threshold (inherited, left out
  of the file), types an Intensity (saved, blended), removes Ambient
  Occlusion and adds it back empty, ticks Radius, undoes all five with ⌘Z
  and redoes one; a second test puts the profile on a global volume and
  checks with `fynkel render` that the bloom set in the Inspector makes a
  tenth of the picture brighter.
- 2026-09-24: model import settings (`41190a1` the importer's
  `scaleFactor`, `normals` / `smoothingAngle`, `importAnimation`;
  `c3e7d3e`) — `import_settings_parity_test.dart` types a Scale Factor,
  switches Normals to Calculate, applies, and checks with `fynkel render`
  that the robot is drawn bigger; a second test unticks Import Animation,
  selects another asset, and answers the Unapplied import settings
  question with Revert and then Apply.
- 2026-09-24: the world transform (`2e0183f`) — `transform.position`
  and `rotation` are world values now, with the local ones beside them
  and Unity's `translate` / `rotate` / `rotateAround` / `lookAt`, axes and
  Euler degrees (ADR-0013 amended: Euler angles are degrees). Scripts
  run in the SDK, so the parity test is `packages/fynkel/test/transform_test.dart`
  over the real native library: a child of a turned, scaled parent reads
  and writes its world position, `lookAt` and `translate` move it the
  way a Unity script would, and the world's own snapshot agrees.
- 2026-09-24: one script throwing (`faf648d`) — every behaviour
  callback runs on its own and reports to `World.onBehaviourError` (the
  Console's Game tab with the stack under the editor).
  `packages/fynkel/test/script_errors_test.dart`: a throwing `update`
  leaves the behaviours before and after it running every frame, a
  throwing `onCreate` / `start` is reported once, and the channel record
  carries the message and the stack.
- 2026-09-24: coroutines and Invoke (`b92178e`) —
  `packages/fynkel/test/coroutine_test.dart` checks where a coroutine runs
  in the frame (after `update`, before `lateUpdate`), each wait's frame,
  `invokeRepeating`'s calls, and that a disabled behaviour keeps its
  routines while an inactive object or a destroyed behaviour stops them.
- 2026-09-24: raw keys (`6b25b52`, ADR-0024 — actions stay the binding
  source; raw reads sit beside them) — `packages/fynkel/test/raw_input_test.dart`
  presses F, which no action binds, and a mouse button over the fixture
  project: down on the first of three catch-up ticks and in `update`,
  held after, a release in a frame too short to tick reaching the next
  tick, a focus reset; a misspelt action warns once in the Console.
- 2026-09-24: Debug.Log (`9bdf3d6`) — `debug_log_parity_test.dart`
  sends the SDK's own `Debug.logWarning`, `logError` and a `print` over
  the channel into the Game tab: each at its level, the warning located at
  its calling line, ↩ opening that line; `packages/fynkel/test/debug_log_test.dart`
  covers every level from a behaviour.
- 2026-09-24: object fields and enums (`46c091a` crates, `de1de2c` SDK +
  generator, `962d30a` editor; ADR-0018 amended) —
  `object_field_parity_test.dart` drops Hierarchy rows on a Turret's
  `GameObject` and `Spinner` fields (the crate without a Spinner is
  refused), saves `{entity}` / `{entity, component}`, undoes one, picks
  through ⊙ (only the Robot has a Spinner), clears with ×, and shows
  `Missing` after the target is deleted;
  `packages/fynkel/test/object_fields_test.dart` loads such a scene over
  the real library (the script attached after the one referencing it
  still resolves; a gone target is None with `FYN-B0003`) and
  instantiates a prefab twice (each reaches its own child); the prefab
  expansion test checks the derived component id.
- 2026-09-24: a failed hot reload.
  `apps/editor/test/parity/compile_error_parity_test.dart` writes a
  `reloadFailed` status with two compiler errors: ⌘P is refused with
  Unity's message, the Console has both rows at their file:line:column, ↩
  runs the IDE command with the absolute path and position, and a
  `reloaded` status clears them. `code_reload_test.dart` covers the
  parser.
- 2026-09-24: drag a script onto an object.
  `apps/editor/test/parity/script_drop_parity_test.dart` gives the fixture
  copy three scripts (one behaviour, none, two): the payload is the
  behaviour, the file-named one, or nothing; the Spinner dropped on the
  Sun's row gets its defaults with the selection kept, the Robot (which
  has one) is left alone, Crate A gets an `addComponent` override and ⌘Z
  takes it back; dropped on the Inspector it goes onto the selected
  Crate B, and on the Scene view onto Crate A under the pointer.
- 2026-09-25: Tier 3, the Scene view. Pivot / Center (`51a245a`,
  `test/parity/pivot_parity_test.dart`: Z moves the gizmo between the
  active object's origin and the selection's centre; two crates turned
  and scaled each about their own origin and then as a group about the
  centre; the uniform cube doubles over 100 points). Prefab picking
  (`7a66084`, `prefab_click_parity_test.dart`: root, child, what is
  behind; straight to the child inside a selected instance). Lock View
  (`5c57adb`, `lock_view_parity_test.dart`: ⇧F and F twice lock, a drag
  and Play move the view with the object, orbit keeps the lock, pan lets
  go). The Rect tool and the trackball (`510d218`,
  `rect_tool_parity_test.dart`: a corner ×1.5 with the opposite one
  still, ⌥ on an edge, a move inside, a quarter turn from a corner zone,
  a click inside selecting the crate under it; a 90-point drag inside the
  rotate rings turning the front a quarter to the right).
- 2026-09-25: the Prefab bar and Overrides ▾ (`7d51553`,
  `overrides_parity_test.dart`: Crate A's two overrides listed, the
  prefab's `true` beside the instance's `false`, Revert as one undo step,
  Apply to Prefab from a right-click on the field saved into the prefab,
  an added Point Light reverted and then applied, Apply All emptying the
  list).
- 2026-09-25: the Project grid (`8da4595`,
  `project_grid_parity_test.dart`: the slider turns the list into tiles,
  bigger further right, remembered, the list again at its left end;
  ← → ↑ ↓ across and down, ⇧ extending, Enter renaming under the
  picture, a double-click opening the prefab, a folder tile going in, a
  model's ▸ bringing its parts; the thumbnails asked for in one batch,
  not again when shown again, a refusal not asked again, a changed file
  drawn again as a new image, a texture never asked for).
- 2026-09-25: Prefab mode (`20edc8e`, `prefab_mode_parity_test.dart`:
  › on Crate A opens the Crate in context — Main › Crate, the Hierarchy's
  ‹ header —; with Auto Save a rename and its undo are on disk at once;
  without it Save saves, a crumb or ‹ asks, Cancel stays, Discard leaves
  the file alone; Context Hidden remembered; from the Project, isolation
  and no Context menu).
- 2026-09-25: several scenes (`f632f96`, `ca34f28`,
  `multi_scene_parity_test.dart`: Open Scene Additive keeps Main active,
  Results under its own header; a click on the Trophy's row, or on the
  Trophy in the Scene view, makes Results active with the Trophy
  selected and Main's selection gone, the camera unmoved; Crate A dragged
  onto Results' header moves there as an instance, the Floor onto it goes
  under it, Main's undo brings the Floor back; Unload leaves the header,
  the last loaded scene cannot unload; a double-click on Results asks
  about Main's change, Cancel keeps both, Discard leaves Results alone).
- 2026-09-25: the selection outline (`6344488`…`4b493b7` renderer and
  ABI, `70ad2db` editor; `selection_outline_parity_test.dart`: the Robot
  ringed and not boxed, a Cube made under it blue until it is selected,
  an instance's own mesh and its parts apart, the Camera's cross kept).
- 2026-09-25: an outline shows through what hides the object, as Unity's
  does (its issue tracker files the opposite as a regression); the row's
  🟡 was wrong and is gone.
- 2026-09-25: drag to reorder components (`7d531c3`,
  `component_reorder_parity_test.dart`: Crate B's Collider dragged onto
  the RigidBody shows the line above it and takes its place, dragged
  back lands after it, nothing goes above the Transform or moves it, one
  ⌘Z each; a prefab instance's inherited cards do not drag).
- 2026-09-25: prefab variants and nested prefabs (`9146759`, `e5e44c8`,
  `3fa1469` document and API, `9241460` editor;
  `prefab_variant_parity_test.dart`: Crate A dragged to the Project asks,
  Cancel leaves it; Prefab Variant writes `Crate A.prefab.json` whose root
  is a Crate instance holding Crate A's two overrides, the instance left
  with none and its bar reading *Prefab Variant*; a new override offers
  *Apply to Prefab Variant 'Crate A'* and *Apply to Prefab 'Crate'*, the
  latter writing the Crate and leaving the variant no override of it;
  Apply All ▾ into the variant; Original Prefab unpacks the Crate into
  the new prefab one level; Create ▸ Prefab Variant in the Project; an
  empty holding Crate A makes a prefab with Crate A nested, overrides
  kept; Unpack leaves Crate A an instance, Unpack Completely does not).
- 2026-09-25: Physics Material, engine side (`eee5782` crates,
  `797c4b8` SDK; ADR-0025, accepted). `fynkel_physics` tests: a ball
  bounces on bounciness 1, how far a box slides depends on friction, and
  the higher combine rule wins on both sides (minimum on ice slides on,
  maximum stops it). The cook test and `fynkel_assets/tests/physics_material.rs`
  cook a bouncy material into the fixture project: the crate rebounds
  over a metre, and a recook re-registers the new values on the live
  collider.
- 2026-09-25: Physics Material in the editor (`089773d`,
  `physics_material_parity_test.dart`: + ▸ New Physics Material writes
  `New Physics Material.physicsmaterial.json`; Bounciness typed and
  Friction Combine set to Maximum are saved at once, ⌘Z reads *Undo
  Physics Material Change* and undoes each, the scene untouched; the
  Floor's Collider shows Material None with Friction and Bounciness, ⊙
  offers the one physics material only, picked it hides both, ⌘Z brings
  them back).
