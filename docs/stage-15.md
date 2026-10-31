# Stage 15 plan: UI in the world

Status: accepted 2026-10-31 ("ok let's do that"), with the
recommended answer to each decision (§7). Scope and rules:
[ADR-0001](decisions/0001-fynkel-2-code-first.md). Stage 14 is done
([report](stage-14-report.md)); its two phase 5 points left open there
came in #186.

A game's UI sits in two places: over the screen (a HUD, a menu, a shop),
which Flutter does, and in the world: a name tag over a head, a health
bar, a speech bubble, a number that pops out of a hit, a sign on a wall,
an arrow to where to go next. Fynkel has half of the second:
`WorldWidget` (§17) puts a Flutter widget at an object's place, but it's
always on top, even of the wall in front of it, and nothing can be drawn
*in* the scene but solid 3D text. **Stage 15 finishes UI in the world**:
widgets that know what's in front of them, and flat text and pictures
drawn by the engine, crisp at any distance, behind and in front of
things as the rest of the scene is.

**The goal.** Each of these takes a line or two:

```dart
// A name tag that hides behind walls, and an arrow at the screen's edge
// pointing to the exit while it's off the screen.
hero.addComponent(WorldWidget((_) => NameTag('Mo'), offset: Vec3(0, 2, 0), hideBehind: true));
exit.addComponent(WorldWidget((_) => const ExitArrow(), keepOnScreen: true));

// A sign on a wall, in the scene: crisp, lit or not, hidden by what's in front.
wall.add(GameObject(position: Vec3(0, 2, 0.01), components: [TextRenderer('EXIT', font: signs, size: 0.4, color: Color.red)]));

// A damage number that faces the camera: changed every frame for nothing.
hit.addComponent(TextRenderer('-12', font: numbers, size: 0.3, facing: Facing.camera, outline: 0.1));

// A picture in the world: a marker on the ground, a leaf, a cloud.
spot.addComponent(SpriteRenderer(arrow, size: Vec2(1, 1), facing: Facing.up));
```

**The road after it**: a full 2D engine next, the user's ask on
2026-10-31 (sprite sheets and their animation, tilemaps, 2D physics on
rapier2d, a pixel-perfect camera, sorting layers), so phase 3's sprites
are made the 2D way from the start; its place in the order is the
user's to set. Then speed (the iPhone measured by cable,
then RN12, RN11 and RN8, until the Sheepdog field is as fast as Unity's
on each device), path finding, then animation 2 and cutting
meshes. Shipping is far off.

## 1. What there is today

- **`WorldWidget`** (`package:fynkel_flutter`, §17): a Flutter widget at
  its object's position plus `offset`, its `alignment` point there.
  Built once and then only moved (100 health bars hold 120 fps on the
  Mac and the iPhone); `shrinkBeyond` makes it smaller with distance;
  nearer ones are painted over farther ones; a tap on it is its own.
  It's hidden while its object is off, behind the camera or off the
  view.
- **What it lacks**: it's drawn over everything, so a name tag shows
  through the wall in front of its owner. Off the view it's gone, so a
  "this way" arrow at the screen's edge is the game's to write. Nobody
  has checked that it stays on its object while the camera turns fast:
  Flutter and the engine present their frames separately (on iOS the
  engine's layer is under Flutter's), and a frame between them makes a
  widget swim around its object.
- **The game**: scamonthephone doesn't use it. It works out screen
  points itself each frame (`Office.heads`, `spots`, `ringAt`), puts
  them in `ValueNotifier`s of maps, and rebuilds whole `Stack`s of
  `Positioned` bubbles from them, with its own off-screen checks. That's
  what every game would write without this stage.
- **Text in the scene**: `Mesh.text` (§12b) makes solid 3D letters from
  a font's outlines, as Unity's `TextMesh` with depth. Two lines are
  4,000 triangles and 0.4 ms to make: fine for a score that changes a
  few times a second, too much for a number that changes every frame,
  and with no outline or shadow around the letters.
- **Pictures in the scene**: none. A quad (`Mesh.grid`) with a
  `Blending.transparent` material does it by hand, without facing the
  camera; particles' billboards face it but are particles.
- **Textures from code**: `Texture2D.rgba(width, height, pixels)`.

## 2. Unity's way, and Fynkel's

| In Unity | In Fynkel after stage 15 |
|---|---|
| A Screen Space Canvas whose elements follow a world point (by hand, `WorldToScreenPoint` each frame). | `WorldWidget`, as now: any Flutter widget, with its taps. |
| — (by hand: a raycast from the camera each frame) | `WorldWidget(hideBehind: true)`: hidden, fading, while something is in front of its point. |
| — (by hand, or an asset: off-screen indicators) | `WorldWidget(keepOnScreen: true)`: held at the screen's edge while its point is off it, with which way it is for an arrow to turn by. |
| TextMeshPro, 3D (`TextMeshPro` component): SDF text in the scene, with outline, softness, face and outline colours. | `TextRenderer(text, font:, size:, color:, outline:, outlineColor:, align:, facing:)`: SDF text in the scene. |
| Legacy `TextMesh`. | `Mesh.text`, as now, for solid letters with depth. |
| `SpriteRenderer` (`sprite`, `color`, `flipX/Y`, `drawMode: Sliced` for 9-slice, sorting order). | `SpriteRenderer(texture, size:, color:, flipX:, flipY:, slice:, order:, facing:)`. |
| A World Space Canvas: uGUI laid out on a plane in the scene, with input by raycasts. | Not in this stage (§5). |
| Billboarding, by a script (`transform.LookAt(camera)`). | `facing:` on both: `Facing.none` (as the object turns), `.camera` (faces it), `.upright` (turns about Y only, as a tree), `.up` (flat on the ground). |

## 3. Design decisions

### 3.1 World widgets that know what's in front of them

- **`hideBehind`** casts from the camera to the widget's point with
  `world.raycast` (meshes as drawn, its tree over each big mesh, §15),
  and hides the widget when something is hit before the point, less a
  small margin so its own object doesn't hide it. `hideLayers:
  LayerMask.of([walls])` casts only at those layers. It fades out and in over
  0.15 s, not blinking.
- **Not every frame for every widget**: each frame casts for a share of
  them, nearest and moved first, so 100 tags cost a fixed few casts (the
  budget is measured, §6). A widget's own object is never what hides it.
- **`keepOnScreen`**: off the view (or behind the camera), the widget is
  held inside the view's edge, `edgeMargin` in, on the line from the
  middle to its point; `WorldWidget.of(context)` gives its builder
  `offScreen` and `angle` to show an arrow instead of the tag.
- **On its object, exactly.** First measure it: a widget pinned to a
  cube while the camera turns at speed, filmed on the iPhone at 240 fps.
  If they part, the layer places widgets with the camera of the frame
  the engine is presenting, not of the frame just updated (the engine
  knows which one it is: `view.presented`).
- **The game**: its bubbles, speech line and phone ring moved onto
  `WorldWidget`, which proves it (with the user's word: it's their repo).

### 3.2 Flat text in the scene

- **Signed distance fields**, as TextMeshPro: each letter is drawn once
  into an atlas texture as its distance from the outline, from the same
  outlines `Mesh.text` reads (`crates/fynkel_mesh/src/outline.rs`). A
  letter is then one quad, crisp at any size and distance, and its
  outline, softness and shadow are one shader's numbers.
- **One atlas per font**, letters added the first time they're used, in
  Rust. The atlas grows by pages; a font's letters are drawn once for
  the life of the font.
- **`TextRenderer`** is a component: changing its `text` costs Dart's
  side nothing beyond the string, the engine lays it out into quads it
  keeps (no garbage, tested). Every `TextRenderer` of one font is one
  draw.
- **Drawn with the see-through things**, sorted with them, depth-tested
  so walls hide it; `onTop: true` draws it over everything (a hit
  number), as TextMeshPro's overlay shader.
- **Lit or not**: unlit by default (text reads the same in the dark),
  `lit: true` for a sign that's part of the room.
- **The same font asset** as `Mesh.text` (`world.assets.font`), the
  same layout: kerning, `align`, `lineSpacing`, `\n`, no
  right-to-left or emoji (§21).

### 3.3 Pictures in the scene

- **`SpriteRenderer`**: a texture on a quad of `size` metres, its
  `color` multiplied in, `flipX`/`flipY`, `pivot`, see-through or cut
  out (`alphaCutoff`). Sprites of one texture are one draw, instanced
  as particles are.
- **9-slice** (`slice: Insets(...)`), so a speech bubble or a panel
  stretches without stretching its corners.
- **`sortingLayer` and `order`**, as Unity's, from the start: a 2D game
  (the stage after) draws by them, not by depth.
- **A widget onto a texture** (decision 4): `await
  Texture2D.fromWidget(widget, size:)` draws a Flutter widget once into
  a texture, for a sign or a poster that looks like the game's UI. It
  doesn't take taps and isn't live; drawing it again is a game's choice
  (a score board once a second, not every frame).

## 4. Phases

Each phase is one PR or a few, with its lab page, its guide section,
its tests, and plain words. A phase is done when it's merged with CI
green and been seen on the Mac, the iPhone and the Note10+.

### Phase 1: world widgets, finished (M)

- Measure first: does a widget stay on its object while the camera
  turns at speed? Fix it if not (§3.1).
- `hideBehind` (and by layers), with its fade and its share of casts a
  frame.
- `keepOnScreen`, `edgeMargin`, `WorldWidget.of(context).offScreen` and
  `.angle`.
- The lab: "UI in the world", a courtyard of pillars with tags behind
  them, an arrow to a flag behind you, the camera spun by a slider.
- The game's bubbles moved onto it (with the user's word).

*Done when* 100 tags with `hideBehind` among 3,000 objects add under
1 ms on the Note10+, a tag never shows through a pillar for more than
the fade, and the film shows no gap between a tag and its cube.

### Phase 2: flat text in the scene (L)

- The SDF atlas from the outlines, in Rust; its shader with face colour,
  outline, softness, shadow.
- `TextRenderer` with `facing`, `onTop`, `lit`, `align`, `lineSpacing`;
  `font.measure` as now.
- Goldens: a sign up close and far off, an outline, hidden behind a
  wall, on top of it.
- The lab page gets signs and numbers popping from hits.

*Done when* 200 numbers changed every frame cost under 1 ms on the
Note10+ with no garbage, and text a few pixels high still reads.

### Phase 3: pictures in the scene (M)

- `SpriteRenderer`, `facing`, 9-slice, `sortingLayer` and `order`,
  instancing.
- `Texture2D.fromWidget` (decision 4).
- The lab page gets a speech bubble that's a sprite, markers on the
  ground, a poster drawn from a widget.

*Done when* 1,000 sprites of one texture are one draw and hold 60 at low
on the Note10+.

### Phase 4: the guide, Unity's table, the report (S)

- §17 and a new §12c, §21 and §22 updated, `check_guide` green.
- Measured on the three devices; the report.

## 5. Not in this stage

A world-space Flutter canvas: live widgets laid out on a plane in the
scene, turned with it and taking taps by raycasts (Unity's World Space
Canvas). Flutter draws on its own; getting its picture into the
engine's frame every frame needs a texture shared between the two on
each platform, and taps sent back through the plane. It's possible, and
costly; `Texture2D.fromWidget` and `WorldWidget` cover most of what a
mobile game wants until a game needs more. Also out: right-to-left
text and emoji (a font shaper), rich text (bold in the middle of a
line, colours by word), text along a curve, sprite sheets animated by
frames (a game can change `texture`), and 2D games.

## 6. Measurements

On the Mac, the iPhone 16 Pro Max and the Note10+ (profile builds):

- **A widget on its object**: filmed at 240 fps, no visible gap while the
  camera turns at 180°/s.
- **Hiding**: 100 tags with `hideBehind` among 3,000 objects under 1 ms
  a frame on the Note10+, casts included.
- **Text**: 200 numbers changed every frame under 1 ms on the Note10+,
  no garbage in Dart or Rust; a new font's first 100 letters into the
  atlas measured and said.
- **Sprites**: 1,000 of one texture in one draw, 60 at low on the
  Note10+.
- **Nothing lost**: every golden from before is the same, and the game
  at each level is as fast as now.

## 7. Decisions to make

1. **Hidden by the meshes as drawn** (`world.raycast`, *recommended*:
   works with no colliders, as what the player sees), rather than by
   physics colliders (cheaper for meshes changed every frame, but a
   wall with no collider wouldn't hide anything) or by reading the
   depth buffer back from the GPU (exact, but a frame late and a stall
   on phones).
2. **Signed distance fields for text** (*recommended*: TextMeshPro's
   way; one quad a letter, crisp at any distance, outlines for free),
   rather than flat letters from the outlines as triangles (no new
   shader, but thousands of triangles, jagged without anti-aliasing,
   and no outline).
3. **Unity's names**: `TextRenderer`, `SpriteRenderer`, `Facing`
   (*recommended*). `TextMeshPro` itself is a product's name, not a
   component's.
4. **`Texture2D.fromWidget` in this stage** (*recommended*: small, and
   it gives in-world signs the game's own look), rather than later.
5. **The game's bubbles moved onto `WorldWidget` in phase 1**
   (*recommended*: the real test of it), in scamonthephone, with the
   user's word for each change there.
