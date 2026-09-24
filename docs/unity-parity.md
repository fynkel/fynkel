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
| Component header | enable checkbox, Copy/Paste Values, Move Up/Down, drag to reorder | 🔧 checkbox + Copy Component / Paste Component Values (`13d2876`); Move Up / Move Down (`moveComponent` `8b8e747`, `5d36af3`); ❌ drag to reorder |
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
| Create → Volume Profile, Physics Material | | 🔧 Volume Profile (`682f953`, `adb5146`); Physics Material ❌ — no such asset: friction and restitution are the Collider's own fields, and an asset would need an ADR (Tier 3) |
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
| `Input.GetKey` | raw keys | ❌ declared actions only; an undeclared one reads false silently |
| `Debug.Log` | Console, click to the line | 🟡 info only, no source link; `print` not captured |
| Inspector fields | public fields; GameObject/component/colour/enum references | 🟡 `@Expose` only; no object references, enums or colours |
| A failed hot reload | blocks Play, errors in the Console | 🟡 a toast; old code keeps running |
| Drag a script onto an object | adds it | ❌ |

## Tier 3 — the Scene view and prefabs, beyond the basics

| | Unity | Fynkel |
|---|---|---|
| Pivot / Center (Z) | | ❌ always the lead object's origin |
| Click a prefab, click again | root, then the child under the pointer | 🟡 root, then the object behind |
| ⇧F / double-F | follow the selection | 🟡 ⇧F frames all |
| Selection outline | silhouette | 🟡 bounding box |
| Rect tool (T), free-rotate trackball | | ❌ |
| Prefab mode | in context, breadcrumb, auto-save | 🟡 a document tab |
| Prefab variants, Unpack Completely, a prefab from an object holding instances | | ❌ |
| Overrides dropdown, blue margin bar | | 🟡 bold labels + a list at the bottom |
| Multi-scene Hierarchy | | ❌ one document per Hierarchy |
| Physics Material asset | shared friction/bounciness, combine modes | ❌ per-Collider fields; needs an ADR |
| Grid thumbnails, zoom slider | | ❌ list only |

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
