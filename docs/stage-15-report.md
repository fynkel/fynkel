# Stage 15 report: UI in the world

[The stage 15 plan](stage-15.md). Phases 1 and 2 are merged (#188,
#189); phases 3 and 4 are #190.

## The result

A game can put its UI in the world three ways now, each for what it's
good at:
- **Flutter widgets over the view, at an object** (`WorldWidget`, §17):
  live, tappable, the game's own look.
  - They hide behind what's in front of them (`hideBehind`).
  - They keep to the screen's edge (`keepOnScreen`), with an arrow toward
    a point that's off it.
  - They move with the engine's picture, not a frame ahead of it.
- **Flat text in the scene** (`TextRenderer`, §12c), as TextMeshPro's:
  - crisp close up, and readable a few pixels high;
  - outlines, softness and a shadow;
  - facing the camera, standing upright, or flat on the ground;
  - over everything when asked.
- **Pictures in the scene** (`SpriteRenderer`, §12c), as Unity's Sprite
  Renderer:
  - 9-slice;
  - drawn by sorting layer and order;
  - cut out by alpha;
  - a thousand of one texture in one draw;
  - a Flutter widget drawn once into a texture
    (`widget.toTexture`).

| Phase | What | PRs |
|---|---|---|
| 1 | `WorldWidget`'s `hideBehind` (eight looks a frame in turn) and `keepOnScreen` with `WorldWidget.of`; each host says how many frames behind Flutter's its picture is, measured from films; the lab's "UI in the world"; the game's speech bubbles moved onto it | #187 (plan), #188 |
| 2 | The SDF atlas from a font's outlines (Rust); `TextRenderer` with `Facing`, outline, softness, shadow, `onTop`, `lit`; goldens; the lab's "Text in the world" | #189 |
| 3 | `SpriteRenderer`, `Insets` (9-slice), `sortingLayer` and `order` for everything see-through; `widget.toTexture`; TextRenderer's setters made closure-free; the lab's "Pictures in the world" | #190 |
| 4 | The guide (§12c new; §12b, §17, §21 and §22); measured on the three devices; a crash on Android found and fixed (below); this report | #190 |

`Texture2D.fromWidget`, as the plan named it, is
`widget.toTexture(size:, pixelRatio:)`: Dart has no static extension
members, so a package can't add a constructor to another package's class.

## Measured

At low, a minute each after 20 s settling, from the lab with
`LAB_RATES` (examples/lab, `LAB_QUALITY`, `LAB_TAGS`). The world's
update is the whole page's frame on the game's thread.

| | Note10+ | iPhone 16 Pro Max | iPad (6th gen, A10) |
|---|---|---|---|
| 1,000 sprites of one texture, moving | **59.9 fps** (lowest second 57.9); update 0.89 ms | **60.0** (60.0); 0.35 ms | **60.0** (57.0); 1.20 ms |
| 200 numbers changed every frame | **59.9** (58.9); update **0.73 ms** | **60.0**; 0.32 ms | **59.0** (57.0); 1.78 ms |
| 100 `hideBehind` tags among 3,000 objects | 59.9 fps; **+1.27 ms** on the UI thread a frame | not measured | not measured |
| How many frames the engine's picture is behind Flutter's | 1 | 0 (its own layer) | — |

- **Phase 1** (100 tags under 1 ms on the Note10+): **missed**.
  - The tags cost 1.27 ms on the UI thread (7.10 ms a frame against
    5.83 with the same 3,000 cubes and no tags; with tags that don't
    look, 5.76).
  - So all of it is the eight looks a frame: each cast still tests
    every object's box. That's 0.3 ms on the Mac, and about 4½ times
    that on the phone.
  - A tree over the objects' boxes (a broadphase) would fix it, and
    every game's raycasts with it.
  - The rest of phase 1's bar held: a tag never shows through a pillar
    beyond its fade (tested). Filmed with the camera spun at 180°/s,
    the cross stays on its cube: on the Note10+ the median gap is 0 px,
    and the iPhone shows none. On the Mac it went from 21 to 11 px,
    the half frame left being its texture's own timing.
- **Phase 2** (200 numbers under 1 ms on the Note10+, with no garbage):
  **met**.
  - 0.73 ms is the whole page's update, numbers and all.
  - The allocation tests count nothing made a frame (0.07 ms on a Mac).
  - Text a few pixels high reads, by the golden `text_signs`.
- **Phase 3** (1,000 sprites of one texture in one draw, 60 at low on
  the Note10+): **met**.
  - One draw, by the render test.
  - 59.9 fps on the phone.
- **The render thread's "busy"** on iOS includes waiting for the next
  drawable (16.7 ms at 60 fps on the iPad), so it isn't the frame's work
  there. On the Note10+ it was 1.9 ms a frame with the sprites, 1.3 with
  the numbers, and 1.2 on a quiet page.

## Found on the way

- **`toTexture` crashed the app on Android when called as it opened.**
  - Impeller's GL backend has no context before the window's surface
    is made, so drawing the widget then killed Flutter's raster thread
    (SIGSEGV).
  - It now waits for the first frame on Android. Tests never draw a
    frame, so they don't wait.
  - Found by the first phone run of the sprites page.
- **The lab's tests start Flutter's binding**, as the app has one, since
  a page draws widgets into textures.
- **Every lab page prints its rates** under `LAB_RATES`, with the render
  thread's time a frame, not only the pages that asked for it.

## Left open

- **A broadphase for raycasts**, for phase 1's bar: a tree over the
  objects' world boxes, refitted as they move. It belongs with the speed
  stage, as it speeds up every cast a game makes.
- **The tags on the iPhone and the iPad** weren't measured.
- **The game's bubbles** on `WorldWidget` are in scamonthephone's branch
  m2.5-new-call (faab407), not pushed; that's the game's to merge.
- **A sprite benchmark** in CI: sprites have correctness tests and the
  render test's one draw, but no bar of their own in `fynkel_bench`.
- **Not in this stage**, as the plan said:
  - a world-space Flutter canvas (live widgets laid out in the scene and
    tapped through it);
  - right-to-left text and emoji (a font shaper);
  - rich text.

## Next

As agreed: a full 2D engine as a stage of its own; its place in the
order is the user's to set. Then speed (the iPhone measured by cable;
RN12, RN11 and RN8; a broadphase for raycasts), path finding, then
animation 2 and cutting meshes.
