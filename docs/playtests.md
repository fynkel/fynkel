# Playtests: games written from the docs, and a targeted audit

Status: agreed with the user 2026-11-16. Comes before any publishing work.

**Why.** Only one real game (Ring Ring Deals) and only one person have used
Fynkel. Tests check what we thought to check; games find what we didn't:
confusing API, gaps in the guide, systems that break together. Stage 14
already audited the whole engine, so the audit here is narrow: the parts
games rarely reach.

## 1. Games written by agents from the docs

Each game is written by an agent that knows Fynkel only from the docs, as a
new user would.

**What the agent may read:** `README.md`, `docs/writing-a-game.md`, the
examples' own code, and the public libraries (`packages/*/lib/*.dart`). It
may read a public member's doc comment in `lib/src/` when the guide doesn't
answer, and logs each time it had to (that's a docs gap). It never reads
`crates/`, the rest of `lib/src/`, the stage reports or the decisions.

**What it may change:** only its own game, in `playtests/<game>/`. It never
edits the engine, its packages or the examples. When the engine is in the
way it logs it and works around it, and says how.

**It shows its game works** by `flutter test` (§19 of the guide) and by
pictures from `HeadlessView`, saved in `playtests/<game>/shots/`, which it
looks at itself. The look and feel on devices is the user's or a
`fynkel_measure` run's.

### The log

`playtests/<game>/LOG.md`, one entry per problem:

```
### <n>. <one line>
- Kind: bug | confusing | docs gap | missing
- Tried: what it was doing
- Guide said: the section, or "nothing"
- Happened: what it saw, with the error as printed
- Repro: the smallest code that shows it (a test where it can)
- Worked around: how, or "stuck"
```

### Sorting what comes back

Every entry is checked against the engine before anything changes; an agent
can be wrong.

- **Bug:** a failing test, then the fix, now.
- **Confusing / docs gap:** the guide or the API is fixed, or the reason not
  to is written down.
- **Missing:** a list only. A feature is made when two games need it.

### The games

| # | Game | What it pushes on |
|---|---|---|
| 1 | Third-person character | Animation, character controller, camera, a level from models |
| 2 | Mostly-Flutter app with a 3D viewer | Several views, navigation, pausing, lifecycle, memory |
| 3 | Physics toy | Joints, many rigidbodies, queries |
| 4 | Puzzle with levels | Loading and unloading, saving by hand, hot reload |
| 5 | Effects arcade | Particles, sound, shaders, bloom |
| 6 | Idle game left running | Memory over an hour, background and back |

1 and 2 first, to see how useful the logs are, then the rest.

**Stop rule:** when three games in a row find no new engine bug, this ends
and publishing work starts.

## 2. Targeted audit

Only where games rarely reach and bugs are worst:

1. **Threads and surfaces:** the render thread against Flutter's, a surface
   configured or resized while frames are drawn (the iOS freeze of stage 17
   phase 2 came from here, and a new layer's first configure is still open).
2. **The Dart–Rust boundary:** who owns each handle, use after free, a
   double free, a Rust panic crossing into Dart.
3. **The app's lifecycle:** background and back, a memory warning, a view
   removed while it draws, hot reload, a world disposed with work in flight.
4. **Stage 14's rows still open:** closed or carried, each with a reason.

Each finding: the place (`file:line`), what goes wrong and when, how sure,
and the failing test that would show it. Nothing is fixed until it's
checked.

## 3. What came back

Games 1 and 2 and the audit, 2026-11-16; fixed on the `playtest-fixes`
branch, each bug with a failing test first. Both games found engine bugs,
so the stop rule's count hasn't started.

| From | What | Settled |
|---|---|---|
| Game 1 | A controller moved up still said grounded: the guide's jump rose 8 cm | Fixed (Rust and Dart tests; §23) |
| Game 1 | A key held through a pause wasn't held again in a `World` test | Fixed: the world re-holds, not the view |
| Game 1 | §13's Farmer walked backwards, and played clips its character hasn't got; `model.clips` | Guide fixed; `check_guide.py` now checks them |
| Game 1 | A widget test couldn't wait for loaded models (game 2 too) | `tester.loaded()` |
| Game 1 | `HeadlessView.png()` was 2 MB a shot | Compressed |
| Game 1 | "unlit materials are drawn lit" on every load of the examples' character | The file no longer asks for unlit |
| Game 1 | A field named `animator` hid GameObject's | GameObject's shortcuts are `@nonVirtual` |
| Game 1 | Skin width, query arguments | §16 |
| Game 2 | A view in a hidden tab ran on, and on a phone could show through | Hidden tabs and covered pages hide the surface and pause |
| Game 2 | "Two views stack", Orbit's fields, `FynkelGame(restartOnReload:)`, `world.theme`'s import, flutter_test's `find` | §15, §17, §18, §19 |
| Game 2 | FynkelGame's doc promised a warning a closure never gives | Doc fixed |
| Audit | Android: Home counted as a memory warning | The plugin hears the OS's levels itself |
| Audit | A `FynkelGame` with no view never heard the app go away | It listens itself |
| Audit | A panic while drawing poisoned the whole world | Only that view's clock |
| Audit | Android: a surface attached while another view drew failed now and then | Configure tried again |
| Audit | Hot restart during a load leaks it | Written down (§18): debug builds only |

### Missing, until a second game asks

- **Camera collision** (game 1): an orbit or follow camera goes through
  walls, and a game can't fix it after the rig, which runs after every
  `onLateUpdate`. Game 1 sphere-cast itself and set `orbit.distance`.
  *Recommendation:* the most likely to be asked for again; every
  third-person game needs it. A rig option in fynkel_physics (the casts
  live there), Cinemachine's Deoccluder as the model: cast from the
  target to where the rig put the camera, and pull it in. Needs the rigs
  to let a package adjust the place they computed. Medium.
- **Model bounds** (game 2): no `Renderer.bounds` or `Mesh.bounds`, so a
  model can't be centred on a plinth or framed by a camera without
  numbers by hand. *Recommendation:* small, as the engine has each mesh's
  box for culling already: `Mesh.bounds` and a world-space
  `GameObject.bounds` over the renderers below it. Worth doing at the next
  ask, or as part of publishing work.
- **3D pictures in lists** (game 2): thumbnails of models for a grid.
  One `World` and a `HeadlessView` drawing each model and keeping its
  `png()` worked on the Mac (10 in 237 ms), but `HeadlessView` says it's
  for tests and tools, and `read` blocks the UI thread while it waits.
  *Recommendation:* first measure that way on the phones; if it holds,
  write it up in §17 as the way. An asynchronous `snapshot` only if
  the wait shows.
