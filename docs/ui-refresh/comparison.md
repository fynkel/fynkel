# Editor UI refresh — Fynkel next to Unity (2026-09-26)

The user's screenshots of the editor (`examples/code_only`) and of Unity
6.3 LTS, panel by panel, with what makes each Fynkel panel feel crowded
and what Unity does instead. The goal the user set: **simple and elegant,
like Unity**; the first complaint: **the Inspector feels crowded**.

Images are in `images/` as `<panel>-fynkel.png` / `<panel>-unity.png`
(macOS Retina, 2× pixels). Unity's screenshots are a third party's UI,
kept only as design reference for this repository.

## What is wrong everywhere (the root causes)

1. **One flat, near-black tone.** Window `#17181A`, panel `#1E1F22`,
   header `#212328`, field `#131416` (`theme/tokens.dart`): four greys
   within a few steps of black, so panels, headers, fields and component
   bands melt into one mass and the eye cannot find the groups. Unity uses
   a mid grey (panel ≈ `#383838`, header ≈ `#282828`, field ≈ `#2A2A2A`,
   component band ≈ `#3E3E3E`): fewer, *further apart* tones, so structure
   shows without borders.
2. **Big type, tall rows, loose tracking.** Fynkel's rows are ~20 px
   with 13 px text that reads wide; Unity's are ~18–20 px with 12 px
   tight text. Same information, Fynkel needs about a third more height.
3. **Borders around every field.** Each Inspector value is its own
   outlined box; Unity's fields are flat darker fills with no outline.
4. **Too much text that is not the user's data.** GUIDs under names,
   units in every field, two-sentence empty states in every panel, a
   status pill in the Scene view, "Camera · ⌘P plays here" on the Game
   view, entity counts in footers.
5. **The accent is everywhere.** Blue fills on the Console's counts and
   Collapse, the Project view toggle, the Hierarchy badges, Open
   Document…, extension sliders. Unity keeps blue for selection and the
   pressed state of a toggle.
6. **Toolbars of many same-weight icons** (Project: 7, Console: 4 + 4
   boxes, Hierarchy: 4) with no grouping; Unity uses fewer, text where it
   helps (Clear, Collapse, Error Pause), and grouped segments.

## Window (`window-*.png`)

| Fynkel | Unity |
| --- | --- |
| Empty panels fill with a big icon + heading + paragraph (Hierarchy, Scene, Game, Inspector) and a wide blue button whose label wraps ("Open / Document…") | An empty panel is simply empty |
| Tab headers wide, lots of padding, all tabs look alike | Compact tabs; the active one is the panel's colour, the rest the darker strip |
| Extension panels (Level Tools) get stock Material sliders — large blue thumbs | Everything drawn with the editor's own controls |
| Status bar: counts of documents/assets/references | Status bar: the last console line |

## Inspector (`inspector-*.png`) — first to fix

| Fynkel | Unity |
| --- | --- |
| Name row, then a GUID line (`bb714ce6-…`) with a copy button | Name, Static; the id is not shown |
| Layer and Tags on two rows | Tag and Layer on one row |
| Components are separated by a thin line; header text bold on the same background | Each component header is a lighter full-width band: foldout ▸, icon, enable checkbox, name, then help / presets / ⋮ |
| Inside a component, ALL-CAPS section labels (LENS, CLIPPING, OUTPUT, EXPOSURE, OUTPUT again) each adding a row plus space | Sub-groups are foldouts (General, Emission, Shadows) — or nothing |
| Each field outlined, ~24 px tall, unit text inside (`54.431 deg`, `10 m`) | Flat fill, ~20 px, just the number |
| Labels truncate ("Orthographic heig…") | Label column wide enough, resizable |
| Vector fields: `X 0` boxes with coloured letters inside the box | `X` outside, number box beside it |
| No Add Component button at the end | Add Component centred at the bottom |

## Project (`project-*.png`)

| Fynkel | Unity |
| --- | --- |
| Right side is a table: NAME / KIND / CONTENTS / SIZE columns | Icons (or a plain list) with names only; a breadcrumb header ("Assets") |
| Toolbar: big blue grid/list segmented control, search, 7 icons (filter, up, lock, +, undo, redo) | `+` on the left, search, 3–4 filter icons |
| Lowercase folder names in a tree with large rows | Tight tree with Favorites on top |
| Footer "0 of 25 assets in assets" and a size slider | Only the size slider |

## Hierarchy (`hierarchy-*.png`)

| Fynkel | Unity |
| --- | --- |
| Prefab instances in bold with a blue badge chip "⬡ 1" and a › on the right | Prefab instances are blue text with a blue cube; the › appears on hover |
| Search, filter, lock, +, ⋮ above | `+▾` and search |
| Footer "27 entities · 24 shown · 1 selected" (wraps when narrow) | No footer |
| Scene row an ordinary row | The scene is a header bar with its own ⋮ |

## Game view (`game-*.png`)

| Fynkel | Unity |
| --- | --- |
| No toolbar; a pill "Camera · ⌘P plays here" over the picture | A toolbar: display, aspect/resolution, Scale, Play Focused, mute, Stats, Gizmos |

## Scene view (`scene-*.png`)

Closest to Unity already — floating grouped toolbars, the axis gizmo, Persp.

| Fynkel | Unity |
| --- | --- |
| Two pills at the bottom always on: a notice ("Cook: 1 asset(s) failed — …") and a status line (`persp 60° pivot (…) d 10.00 1 selected move · world`) | Nothing over the picture but the tools and overlays the user opened |
| Tool buttons in one horizontal pill | Vertical tool palette on the left, view options in the top strip |

## Console (`console-*.png`)

| Fynkel | Unity |
| --- | --- |
| Sub-tabs Project / Game | One list (the Editor ▾ menu picks the source) |
| Counts are big filled blue boxes; Collapse a filled blue button; 4 icons at the end | Flat text buttons Clear ▾, Collapse, Error Pause, Editor ▾; small icon+count toggles at the right |
| Footer "0 errors, 0 warnings · 19 documents, 6 sources, 65 references · validated 00:44:44" | No footer |
| A monospace category column ("extension") | Icon, then the message (two lines, the time first) |

## Proposed order

1. **Tokens** (every panel at once): Unity-like mid-grey tones further
   apart, 12 px text with normal tracking, 20 px rows, fields as flat
   fills with no outline, the accent only for selection/focus/pressed.
2. **Inspector**: component header bands, GUID hidden (in ⋮ → Copy ID),
   Tag + Layer on one row, section caps → foldouts or spacing, units out
   of the boxes, Add Component at the bottom.
3. **Hierarchy, Project, Console**: fewer toolbar buttons, no footers,
   prefab instances as blue text, Project as icons/names with a
   breadcrumb, Console buttons flat.
4. **Empty states and overlays**: quiet or empty panels, no always-on
   pills in Scene/Game; a thin Game toolbar.

Each step with before/after screenshots for the user to judge.

## Done (2026-09-26)

Before/after pictures of each step are in `steps/` (from
`FYNKEL_SHOTS=<dir> flutter test test/look/shots_test.dart`).

1. Tokens — mid grey, flat fields, tighter text: 127510b.
2. Inspector — id in the icon's tooltip, Tags + Layer on one row,
   [Header] captions, units as tooltips (° kept), centred Add
   Component: dc8d021.
3. Hierarchy (prefab blue, › on hover, + first, footer only when
   filtering), Project (+ first, grey toggle, path footer), Console
   (flat toggles, no footer): 7afa365.
4. Empty panels one quiet line; the Scene pill only for a passing state;
   the Game caption on hover; grey sliders: 1a320dc.
5. The title bar's controls centred on the traffic lights; a Game view
   toolbar (camera, aspect ratio, scale, Stats): 435e6e5.
6. Asset pages as Unity's (`images/asset-*`): `name (Kind)` with Open
   and a ⋮ menu, no facts box or button row, fold-out section bands, a
   prefab's root, the file's facts and named uses folded under Info;
   texture thumbnails never a blank gap. Then a script page
   (`images/script-unity.png`): whatever the Project selects shows in
   the Inspector.

7. Project: the folder path over the list (`assets › prefabs`, each
   part goes back), the toolbar down to +, search, filter, lock (one
   list, sorting, undo and redo are in the menus and the column titles),
   a footer of only the slider, names without the document suffix.
8. The status bar: the Console's newest line, the counts at the right.
9. The Console: one list at a time with Project ▾ / Game ▾ (Unity's
   Editor ▾) instead of sub-tabs, Collapse and the counts as flat
   toggles, copy/open/validate in a ⋮ menu; the Game's footer only after
   a run.
10. The colour picker as Unity's (`images/picker-unity.png`): hue ring
    around the square, new | old, RGB 0–255 / RGB 0–1 / HSV sliders with
    their numbers, hexadecimal, the Defaults palette, the user's
    Swatches (kept in the preferences). No eyedropper yet.

Left: the Project list keeps its Kind / Contents / Size columns (Unity
shows names only).
