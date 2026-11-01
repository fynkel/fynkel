# Stage 16 plan: a 2D engine

Status: proposed 2026-11-01, for the user's word on each decision (§7).
Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).
Stage 15 is done ([report](stage-15-report.md)).

Fynkel draws a 3D world, and since stage 15 it can put flat pictures and
text in it. A 2D game needs more than pictures: a character run from a
sprite sheet, a level laid out in tiles, bodies that fall, bounce and
stand on platforms in a plane, a camera that keeps pixel art crisp at
any screen size, and lights for a dark cave. Unity does all of this with
the same objects, components and scenes as its 3D, and so will Fynkel.
**Stage 16 makes Fynkel a 2D engine as well**: everything a 2D mobile
game made in Unity uses, made the Fynkel way, by code.

**The goal.** A platformer's hero, its level and its camera, each in a
line or two:

```dart
// A sheet cut by Aseprite's export: frames and their clips.
final hero = await world.assets.spriteSheet('hero.json');

player.addComponents([
  SpriteRenderer(hero['idle 0']),
  Animator.sprites(hero),            // animator.play('run'): the engine turns the frames
  Rigidbody2D(freezeRotation: true),
  CapsuleCollider2D(size: Vec2(0.8, 1.6)),
]);

// A level from Tiled, its walls one collider, its ledges one-way.
final level = await world.assets.tilemap('level 1.tmj');
level.layer('ground').addComponent(TilemapCollider2D());
level.layer('ledges').addComponents([TilemapCollider2D(), PlatformEffector2D()]);

// 320 × 180 pixels, scaled whole to the screen, never blurred.
world.camera.addComponent(PixelPerfectCamera(pixelsPerUnit: 16, referenceResolution: (320, 180)));

// Is there ground under the feet?
final grounded = Physics2D.raycast(player.position2, Vec2.down, 0.9, layers: ground) != null;
```

**The road after it**: speed (the iPhone measured by cable, then RN12,
RN11 and RN8, until the Sheepdog field is as fast as Unity's on each
device), path finding, then animation 2 (with 2D skinning, §5) and
cutting meshes. Shipping is far off.

## 1. What there is today

- **Sprites** (`SpriteRenderer`, §12c): a texture on a quad, its colour,
  flips, pivot, 9-slice, `pixelsPerMetre`, cut out or see-through,
  `facing`, `sortingLayer` and `order`, a thousand of one texture in one
  draw. A sprite is a whole texture: there's no part of one.
- **Cameras** (§15): `orthographic` with an `orthographicSize`, as
  Unity's. Nothing keeps a pixel of the art a whole number of the
  screen's, so pixel art shimmers as it moves.
- **Textures** are always filtered smoothly; there's no point filtering
  for pixel art, no wrap or clamp to choose.
- **Animation** (§13) plays a model's clips on its skeleton. A sprite's
  frames are the game's to change, one setter a frame for each sprite.
- **Physics** (`package:fynkel_physics`, §16) is 3D, on rapier3d:
  rigidbodies, five colliders, joints, a character controller, queries
  and callbacks. A 2D game can lock Z and rotations, but pays for three
  dimensions and gets none of 2D's own pieces (edges, polygons, one-way
  platforms).
- **Taps** (§11) find meshes by `world.raycast`; sprites and text
  aren't hit. Each cast tests every object's box (stage 15's missed bar).
- **Tilemaps, sprite sheets, 2D lights**: none.

## 2. Unity's way, and Fynkel's

| In Unity | In Fynkel after stage 16 |
|---|---|
| `Sprite` (`Sprite.Create(texture, rect, pivot, pixelsPerUnit, border)`) | `Sprite(texture, rect:, pivot:, pixelsPerUnit:, border:)`; a `Texture2D` is a sprite of itself, so stage 15's code stands. |
| Sprite Editor, Multiple mode: a sheet cut by grid or by hand; Sprite Atlas | `SpriteSheet.grid(texture, cell:)`, `world.assets.spriteSheet('x.json')` (Aseprite's and TexturePacker's export). |
| Texture import: Filter Mode Point, Wrap Mode, Compression None | `Texture2D.filter = TextureFilter.point`, `wrap`. |
| An Animation clip of sprite keyframes, played by the Animator | `Animator.sprites(sheet)`: the sheet's clips (Aseprite's tags, or `sheet.clip('run', [...], fps:)`), `play`, `crossFade` (a cut), `speed`, `onEnd`, as a model's. |
| `SortingGroup`; Transparency Sort Mode Custom Axis (Y for top-down) | `SortingGroup`; `world.sortAxis = Vec3(0, 1, 0)`. |
| Pixel Perfect Camera (assets PPU, reference resolution, upscale render texture, crop frame, snap) | `PixelPerfectCamera(pixelsPerUnit:, referenceResolution:, upscale:, crop:)`. |
| `Rigidbody2D` (Dynamic, Kinematic, Static; gravity scale, freeze rotation, interpolate) | `Rigidbody2D` with the same. |
| `BoxCollider2D`, `CircleCollider2D`, `CapsuleCollider2D`, `PolygonCollider2D`, `EdgeCollider2D`; `CompositeCollider2D` | The same five; a tilemap's collider merged into edges as a composite's is. |
| `PlatformEffector2D` (one-way) | `PlatformEffector2D`. |
| Hinge, Distance, Spring, Slider, Fixed, Wheel `Joint2D` | The same six. |
| `Physics2D.Raycast`, `OverlapCircle`, `OverlapBox`, `OverlapPoint`, `CircleCast`, `BoxCast` | `Physics2D.raycast`, `overlapCircle`, `overlapBox`, `overlapPoint`, `circleCast`, `boxCast`. |
| `OnCollisionEnter2D` / `Stay` / `Exit`, `OnTriggerEnter2D`… | `onCollisionEnter2D`… on a behaviour, as 3D's. |
| `Grid` (Rectangle, Isometric, Hexagon) + `Tilemap` + `TilemapRenderer`; `Tile`, `RuleTile` (2D Extras) | `Grid(layout:)`, `Tilemap`, `Tile`, `RuleTile`; drawn in chunks, a chunk one draw. |
| Tile Palette (painting in the editor) | Tiled's `.tmj` (a free editor most 2D games already use), or tiles set in code. |
| URP 2D: `Light2D` (Global, Point/Spot, Freeform), normal maps | `Light2D.global`, `.point`, `.spot`; a sprite's `normalMap`. |
| `Physics2DRaycaster` (taps on 2D colliders) | Taps hit sprites, tiles and text by their quads; `onClick` as for meshes. |

## 3. Design decisions

### 3.1 The plane and the units

- **2D lives on the XY plane**, Y up, the camera looking along −Z as
  every Fynkel camera does. A 2D object is a `GameObject` as any other:
  its `position.z` puts it nearer or farther (for parallax with a
  perspective camera), and it turns about Z (`rotation2`, in radians).
  `position2` reads and writes X and Y.
- **A unit is a metre**, and `pixelsPerUnit` (100, as Unity's, on a sprite
  or a sheet) says how many of a picture's pixels make one. 2D physics
  works in the same units, so a 16-pixel tile at 16 pixels a unit is a
  1 m box.
- **The same world**: 2D and 3D mix in one scene (a 3D coin in a 2D
  level, a sprite in a 3D room), drawn by the same renderer, in one frame.

### 3.2 Sprites from sheets, and their animation

- **`Sprite`** is a part of a texture: its `rect` in pixels, `pivot`,
  `pixelsPerUnit` and `border` (9-slice), as Unity's. `SpriteRenderer`
  takes a sprite; a `Texture2D` is one of itself, whole. Sprites of one
  texture are still one draw, whichever parts they show.
- **Sheets**: `SpriteSheet.grid(texture, cell:, padding:)` for a plain
  grid; `world.assets.spriteSheet('hero.json')` for Aseprite's export
  (and TexturePacker's, the same JSON): its frames by name, each frame's
  length, and its tags as clips. Trimmed frames keep their place.
- **Point filtering** (`TextureFilter.point`) for pixel art, set on the
  texture or by `world.assets` for a sheet; wrap or clamp.
- **Animation by the engine**: `Animator.sprites(sheet)` plays the
  sheet's clips with the Animator's own `play`, `crossFade` (a cut for
  frames), `speed`, `onEnd` and events at a frame (a footstep). Rust
  turns the frames, so 2,000 animated sprites cost Dart nothing a frame.
- **Sorting**: `SortingGroup` keeps a character's parts (body, sword,
  hat) together in the order, as one; `world.sortAxis` sorts what shares
  a layer and order by a direction (Y for a top-down game: lower is
  nearer).
- **Taps hit sprites**: `world.raycast` hits a sprite's quad (and a
  tile, a text's quads), cut by `alphaCutoff`, so `onClick` works as on
  a mesh.

### 3.3 A tree over the objects

`world.raycast` tests every object's box, and stage 15's 100 tags among
3,000 missed their bar by it (1.27 ms on the Note10+, bar 1 ms). With
sprites hit by casts, a 2D level of a few thousand pieces would pay the
same for every tap. A **bounding volume tree over the objects' world
boxes**, refitted as they move and rebuilt when it's worn, makes a cast
look at the few objects along its line. The same tree answers
`world.overlap(box)`, and culling may use it later (the speed stage).

### 3.4 The pixel-perfect camera

- **`PixelPerfectCamera`** on a camera, as Unity's: the art's
  `pixelsPerUnit` and a `referenceResolution` (320 × 180). It sets the
  `orthographicSize` so one art pixel is a whole number of screen
  pixels, the largest that fits.
- **`upscale: true`** draws the frame at the reference resolution and
  scales it up without blur: rotated sprites stay pixelated, as on a
  console. Off, the screen's pixels are drawn and positions are snapped
  to the art's pixel grid.
- **`crop`** puts bars at the sides or top when the screen's shape
  isn't the reference's, so every phone shows the same part of the
  level; off, a phone shows a little more.
- **Flutter over it** stays at the screen's pixels: a HUD is sharp over
  a 320 × 180 game.

### 3.5 2D physics

- **rapier2d** (decision 1), in a crate and a package of its own
  (`package:fynkel_physics_2d`, decision 2), as `fynkel_physics` is on
  rapier3d: the same shape of code, its buffers and its callbacks, so
  what's learnt there holds.
- **Unity's set**: `Rigidbody2D` (dynamic, kinematic, static; `mass`,
  `gravityScale`, `linearDamping`, `freezeRotation`, `interpolation`,
  `velocity`, `addForce`, `addTorque`, `movePosition`), the five
  colliders with `offset`, `isTrigger`, `PhysicsMaterial2D` (friction,
  bounciness), layers and the collision matrix shared with 3D's.
- **`PlatformEffector2D`**: one-way platforms, jumped through from
  below and stood on from above, by a contact hook in Rust.
- **Six joints**: hinge (with motor and limits), distance, spring,
  slider, fixed, wheel.
- **Queries**: `Physics2D.raycast`, `raycastAll`, `overlapPoint`,
  `overlapCircle`, `overlapBox`, `circleCast`, `boxCast`, with layers
  and `ContactFilter2D`'s triggers rule.
- **Callbacks**: `onCollisionEnter2D` / `Stay` / `Exit` with contacts and
  the impulse, `onTriggerEnter2D` / `Stay` / `Exit`, on a behaviour of the
  object or of its body.
- **Fixed step and interpolation** as 3D's: the same `fixedDelta`, the
  same pose between steps.

### 3.6 Tilemaps

- **`Grid(cellSize:, layout:)`** with `Tilemap` children, as Unity's:
  rectangle, isometric and hexagon layouts; `cellToWorld`,
  `worldToCell`.
- **`Tilemap`**: `setTile(cell, tile)`, `getTile`, `fill`, `clear`,
  a colour and flips per cell; a `Tile` is a sprite and its collider
  shape (none, its sprite's box, or a polygon). Changing tiles while the
  game runs (digging, building) costs only the chunk changed.
- **Drawing in chunks**: the engine keeps each 16 × 16 chunk of a layer
  as one mesh, one draw, and draws only the chunks on the screen. A
  layer is a sorting layer and an order, as a sprite's; isometric maps
  sort their tiles by the sort axis.
- **`RuleTile`** (decision 5): a tile that picks its sprite from its
  neighbours (a grass edge, a corner, a wall's top), and animated tiles
  (water, torches) by frames.
- **`TilemapCollider2D`**: the solid tiles' outlines merged into chains
  of edges, so a body slides along a floor of tiles without catching on
  their seams; changed with the tiles.
- **Tiled's maps** (decision 4): `world.assets.tilemap('level.tmj')`
  reads its tile layers, its tilesets (their pictures, sizes, collision
  shapes and animations), and its object layers as named points and
  shapes a game places its things at.

### 3.7 2D lights

- **`Light2D`** (decision 6): a global light (the scene's colour and
  brightness), point lights (radius, falloff, colour) and spot lights
  (an inner and outer angle), as URP's 2D renderer.
- Sprites and tiles opt in with `lit: true` (stage 15's flag), and a
  sprite or a sheet may have a `normalMap`, so a lamp lights a wall's
  bricks from the side.
- Lights are clustered as 3D lamps are (a cave of 64 torches costs what
  8 do near each pixel). No 2D shadows (§5).

## 4. Phases

Each phase is one PR or a few, with its lab page, its guide section,
its tests first, and plain words. A phase is done when it's merged with
CI green and seen on the Mac, the iPhone and the Note10+ (and the iPad,
the slowest).

### Phase 1: sprites the 2D way (L)

- The tree over objects for `world.raycast` and `world.overlap`
  (§3.3); stage 15's tags measured again.
- `Sprite`, `SpriteSheet.grid`, Aseprite's and TexturePacker's JSON,
  `TextureFilter`, wrap.
- `Animator.sprites`: clips turned in Rust, events at frames.
- `SortingGroup`, `world.sortAxis`; taps hit sprites and text.
- The lab: "2D: sprites", a character run from a sheet, a crowd of
  2,000 walking, a top-down yard sorted by Y.

*Done when* 2,000 animated sprites hold 60 at low on the Note10+ with
nothing spent in Dart a frame, and stage 15's 100 tags among 3,000 cost
under 1 ms there (its missed bar).

### Phase 2: the pixel-perfect camera (M)

- `PixelPerfectCamera` with `upscale`, `crop` and snapping; the frame
  drawn small and scaled without blur.
- Goldens: a sprite moved by a third of a pixel never shimmers; a
  rotated sprite pixelated under `upscale`; the bars under `crop`.
- The lab: "2D: pixels", with a slider for the screen's shape.

*Done when* the goldens hold, and the iPhone, the iPad and the Note10+
show the same 320 × 180 at whole scales.

### Phase 3: 2D physics (L)

- The crate on rapier2d and its package; `Rigidbody2D`, the five
  colliders, materials, layers; callbacks; interpolation.
- Queries; `PlatformEffector2D`; the six joints.
- The guide's §16 gets its 2D half; Unity's 2D physics is measured with
  the same pile (`tools/unity_field`'s way).
- The lab: "2D: physics", a pile, a bridge of hinges, a car on wheel
  joints, one-way ledges.

*Done when* 500 bodies in a pile take under 2 ms a step on the
Note10+, as fast as Unity's 2D on each device or the gap said and
explained, and a body slides along 1,000 boxes laid flat without
catching.

### Phase 4: tilemaps (L)

- `Grid`, `Tilemap`, `Tile`, chunks drawn on the screen only;
  rectangle, isometric and hexagon.
- `RuleTile`, animated tiles; `TilemapCollider2D` merged into edges.
- Tiled's `.tmj`: layers, tilesets, collision shapes, objects.
- The lab: "2D: tiles", a 256 × 256 level scrolled, dug into and built
  on, and an isometric room.

*Done when* a 256 × 256 level of four layers scrolls at 60 at low on
the Note10+, a tile changed costs under 0.2 ms with its collider, and a
body never catches on a seam.

### Phase 5: 2D lights (M)

- `Light2D` global, point and spot; normal maps; clustered.
- Goldens: a lit wall from either side, a dark level with torches.

*Done when* a level with 32 torches holds 60 at low on the Note10+.

### Phase 6: a 2D game, the guide, the report (M)

- `examples/platformer`: a short pixel-art platformer on Kenney's CC0
  *Pixel Platformer* art (decision 7), its level made in Tiled: run,
  jump, one-way ledges, coins, an enemy, a lit cave, a pixel-perfect
  camera that follows. Written as a user would, from the guide only.
- The guide: a new §12d (2D), §16's 2D half, §21 and §22 updated,
  `check_guide` green.
- Measured on the four devices beside Unity's; the report.

## 5. Not in this stage

- **2D skinning and IK** (Unity's 2D Animation package and its PSD
  importer: a character cut into parts and bent by bones), with
  animation 2.
- **2D shadows** (`ShadowCaster2D`) and freeform lights.
- **Sprite Shape** (terrain along a spline, filled and edged); a
  sweep of a `Spline` (§12b) does some of it in 3D.
- **A tile painting editor**: Tiled is the editor.
- **Sprite masks**, the Tiled draw mode of `SpriteRenderer` (repeated,
  not stretched), and LDtk's maps.
- **Effectors** but the one-way platform (area, buoyancy, point,
  surface).

## 6. Measurements

On the Mac, the iPhone 16 Pro Max, the iPad (6th gen) and the Note10+
(profile builds, at low unless said):

- **Sprites**: 2,000 animated, 60 fps, nothing spent in Dart a frame.
- **Casts**: stage 15's 100 tags among 3,000 under 1 ms on the Note10+;
  a tap among 10,000 sprites under 0.1 ms.
- **Physics**: 500 bodies in a pile under 2 ms a step on the Note10+,
  and Unity's 2D with the same pile beside it on each device.
- **Tiles**: a 256 × 256 level of four layers, 60 fps scrolling; a tile
  changed with its collider under 0.2 ms.
- **Lights**: 32 torches, 60 fps.
- **The game**: 60 fps at low on each phone, 120 on the iPhone at
  medium.
- **Nothing lost**: every golden from before is the same, and the 3D
  games at each level are as fast as now.

## 7. Decisions to make

1. **rapier2d for 2D physics** (*recommended*: the 3D side's engine, in
   Rust, the same shape of code and the same build), rather than Box2D
   v3 (Unity's own and very fast, but C: a second toolchain in the build
   hooks, and none of the 3D side's code reused).
2. **Its own package, `fynkel_physics_2d`** (*recommended*: a 2D game
   ships no rapier3d and a 3D game no rapier2d, as a game without
   physics ships none), rather than both in `fynkel_physics`.
3. **The Animator plays sprite clips** (*recommended*: Unity's way, one
   Animator for both, its frames turned in Rust), rather than a
   `SpriteAnimator` of its own in Dart (simpler, but each frame changed
   is a call, and 2,000 sprites would cost a frame's share).
4. **Tilemaps from Tiled's `.tmj`, and from code** (*recommended*: Tiled
   is free and most 2D games' editor already), rather than code only
   (every level typed as text) or LDtk first.
5. **`RuleTile` in this stage** (*recommended*: without it every edge
   of a level is placed by hand), rather than later.
6. **2D lights in this stage**, without shadows (*recommended*: a full
   2D engine has them, and the clustered lamps carry over), rather than
   left for later.
7. **The example on Kenney's CC0 Pixel Platformer** (*recommended*: free
   to commit and credit, as the garden's Kenney art is), rather than art made in
   code.
8. **The raycast tree in phase 1** (*recommended*: taps on sprites need
   it, and it meets stage 15's missed bar now), rather than in the
   speed stage.
9. **Stage numbers**: this is stage 16; speed becomes 17, path finding
   18, animation 2 and cutting meshes after.
