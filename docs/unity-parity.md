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
| GameObject → 3D Object / Light / Camera | a visible cube, sphere, plane…, a light, a camera | ❌ "Create with fynkel.X" by raw type; no built-in meshes, so nothing visible can be made from the menu |
| Double-click a row | frames it in the Scene view | 🔧 `c5a7248` |
| Rename after Esc | F2 shows the real name | 🔧 `c5a7248` |
| Undo | brings the selection back | 🟡 selection only pruned |
| Delete | clears the selection | 🔧 `c5a7248` |
| Right-click empty space | Create menu | ❌ no menu |
| ⌥-click the arrow | expands everything below | ❌ |
| Light rows | light icon | 🔧 `c5a7248` |
| Delete ⌘⌫, rename F2/Enter, Esc cancels, reparent by drag (keeps world position), unparent to empty space, ⌘/⇧-click, arrows, search, active greyed | | ✅ |

### Inspector

| | Unity | Fynkel |
|---|---|---|
| Edit rotation with several objects selected | changes that axis on each | 🔧 `b5a9c11` (it overwrote the other axes with the first object's) |
| Drag a value (label scrub) | the Scene view follows live | 🔧 `b5a9c11` (Transform values; one undo on release) |
| A value that does not parse | stays and shows why | 🟡 silently snaps back |
| Component header | enable checkbox, Copy/Paste Values, Move Up/Down, drag to reorder | ❌ `enabled` is a field row; none of the rest |
| Click an asset reference | pings the asset in the Project | 🟡 opens the picker |
| Colour picker | live while picking, HDR intensity | 🟡 applies on OK only; no HDR |
| Rotation after reselect/undo | keeps the typed Euler | 🟡 recomputed from the quaternion, can flip |
| Typing, maths, Enter/Esc, drag-scrub with ⇧/⌥, one undo per edit, enums, sliders, tooltips, lock | | ✅ |

### Play

| | Unity | Fynkel |
|---|---|---|
| ⌘P | toggles Play/Stop | 🔧 `b5a9c11` (⌘. still stops) |
| ⌘⇧P / ⌘⌥P | Pause / Step | 🔧 `b5a9c11` (the Command Palette moved to ⌘K, asset search to ⌥⌘B) |
| Start paused | yes | ❌ Pause disabled until running |
| Changes during Play revert on Stop, tint | | ✅ |

### Project

| | Unity (macOS) | Fynkel |
|---|---|---|
| Enter | renames | 🟡 opens (F2 renames) |
| ⌘⌫ | deletes after a confirmation | 🟡 bare ⌫ deletes an unreferenced asset without asking; ⌘⌫ ignored |
| ⌘D | duplicates the asset | ❌ |
| Undo | ⌘Z, one history | 🟡 its own ⌥⌘Z history; empty-folder create/delete not undoable |
| Drag files in from Finder | imports | ❌ (changes on disk do import) |
| Model sub-assets | expand in the browser, draggable | ❌ only in the Inspector's asset page |
| Create → Volume Profile, Physics Material | | ❌ |
| Find References In Scene | | ❌ |

## Tier 2 — the look and the code

| | Unity | Fynkel |
|---|---|---|
| Material Inspector | edit colour, maps, emission; preview sphere; live in the Scene view | ❌ **read-only** |
| Drop a material on an object | assigns it | ❌ |
| Volume profile Inspector | Add Override, a checkbox per field | ❌ read-only |
| Model import settings | scale, normals, animation… | 🟡 external files + material remap only; unapplied changes dropped without asking |
| `transform.position` (world), `Translate`, `Rotate`, `LookAt`, `forward`, `eulerAngles` | | ❌ local setters only (`localPosition` semantics); read-only world transform |
| One script throwing | only that script stops | ❌ the rest of the frame's update pass is skipped |
| Coroutines / `Invoke` | | ❌ |
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
| Grid thumbnails, zoom slider | | ❌ list only |

## On purpose

- `Vec3.forward` is −Z (right-handed, as glTF), not Unity's +Z.
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
