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
| 7 | Mini-golf made for phones, played on one | Touch, turning the phone, safe areas, background mid-roll, 15 minutes on the Note10+ |
| 7b | Game 7 again, to look its best on a phone | Light, sky, shadows, materials, shaders, outlines, what each costs on the Note10+; then the user's own round on the iPhone |

1 and 2 first, to see how useful the logs are, then the rest.

**From game 7 on, a phone too.** The bugs that got past games 1–6 were
found on phones (a hidden tab showing through, a stutter as a page slid
in), so game 7's agent also played its game on the Note10+ over adb:
taps and swipes by script, the phone turned, Home and back, screenshots
and the log, and a 15-minute run. What only an eye can judge it lists
for the user, who plays it on the iPhone.

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

Games 1 and 2 and the audit, 2026-11-16, fixed on the `playtest-fixes`
branch; games 3 and 4, 2026-11-16, fixed on `playtest-fixes-2` the next
day; games 5 and 6, 2026-11-17, on `playtest-fixes-3`; game 7 the same
day on `playtest-fixes-4`; game 7b the same day on `playtest-fixes-5`.
Each bug got a failing test first, and each game's own repro was run
against the fix. All seven games found engine bugs, so the stop rule's
count hasn't started. Game 7b is game 7 again, not a game of its own for
the count.

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
| Game 3 | A joint never broke from an impulse, however big: Rapier keeps the last substep's impulse, after the blow was stopped | Fixed: each breakable joint solves for the blow before a step (Rust and Dart tests; §23) |
| Game 3 | `CharacterJoint(axis: up)`, a chain link, threw: the swing axis was up too | The swing axis is X when the axis is along Y |
| Game 3 | Dragger and Orbit fight; `isSleeping`, explosions, ForceModes, combines, motor units, `swingAxis`, `movePosition` from an event, one friction | §15, §16 |
| Game 3 | The lab's "no capsule mesh" | The pill is a capsule |
| Game 4 | An unawaited `preload` threw, uncaught, when the game was left during it | Fixed: it ends quietly when its holder is gone |
| Game 4 | Model bounds, the second ask (game 2's) | `Mesh.bounds`, `GameObject.bounds`, `Bounds` (ABI 56) |
| Game 4 | A `HeadlessView` left the input 0 × 0 | It sizes it, unless something had |
| Game 4 | A widget test couldn't wait for a level a button makes | `tester.until(condition)` |
| Game 4 | JSON levels couldn't be read through the world's source | `World.source` |
| Game 4 | Who holds a preload, a memory test, saving safely, `world.audio`'s import, `_Moving` in an error, the iOS 120 Hz key | §3, §6, §12, §13b, §18, §19 |
| Game 5 | A `punchScale` or `shake` given while one ran walked the object away (a recoil every shot: a negative scale) | Fixed: they share the first one's rest (test; §8, §23) |
| Game 5 | In a test, frames and `audio.render` both move the sound on | §13b: `render` is the clock; listen instead of running frames |
| Game 5 | The mix is cut flat at full scale: four sounds at half volume crackle | §13b says leave room; a limiter listed below and in §21 |
| Game 5 | `Texture2D.rgba`, `ColorBySpeed`'s arguments, the music's `clip`/`isPlaying`/`volume`, the sound stats | §12, §13b, §13c |
| Game 6 | A tween's `.then`, given in a routine that was then stopped (its object sold, the world disposed), failed with an uncaught `RoutineStopped` | Fixed: it hangs quietly, as the rest of a stopped routine; `onComplete` runs (test; §9, §23) |
| Game 6 | Under a page that covers it whole, or in a hidden tab, the world stopped dead, unscaled time too, even with `pauseWhenCovered: false`: Flutter muted the view's ticker | Fixed: its own ticker; paused there, 10 frames a second as under a dialog, nothing drawn (widget test; Note10+; §17, §23) |
| Game 6 | `Mesh.plane()` is 1 × 1, testing `onApplicationPause`, `measure --help` built the app first, measure's modes | §5, §19; `--help` says so at once |
| iPhone check | While a page slid in over the view, the game, still in sight, was already paused and drawn 10 times a second: it stuttered | Fixed (#207): covered once the page is all in (widget test; iPhone; §17, §23) |
| Game 6 | An hour on the Mac: footprint flat at 433–437 MB, 60 fps, 18 KB a frame; 24 game hours in a test flat; 30 of 30 `kill -9`s mid-save loaded | No finding. About 194 MB of GPU memory unnamed, as stage 17 knows |

| Game 7 | A game that paused itself, then pushed its pause page, was drawn at its full rate under it (54 fps on the Note10+, not 10) | Fixed: covered and paused, by whoever, is 10 a second (widget test; the game's repro; §17, §23). Unpaused by the game under the page, it runs: by design, §17 says so |
| Game 7 | `limitVelocity` takes a `LimitVelocity`; `GameObject.bounds` can be null | §13c, §12 |
| Game 7 | The pocket_hole example's Android activity was a `FlutterActivity`, where §1 says a game's is a `FynkelActivity` | Now a `FynkelActivity` |
| Game 7 | Nothing on turning the phone: `fovY` is top to bottom, Android's cutout band in landscape, `SafeArea` | §17 "Turning the phone" |
| Game 7 | On the Note10+: every hole, the best total kept, the same swipe the same result, turned mid-putt, Home mid-roll (the ball where it was), 15 minutes at 59.9 fps median, 253 MB | No finding |
| Game 7b | A new world was a dark grey void: no sky, the sides the sun missed black, no shadows | A new world has `Sky.daylight`; a sun casts shadows unless told not to (tests; examples keep their own look; §2, §5, §5b, §23) |
| Game 7b | Saw-toothed shadows at low: one 1024 map spread over 40 m | Low draws soft shadows to 20 m, the same map (Dart and Rust tests; the low goldens; §5) |
| Game 7b | "A camera asks for bloom…" said once a world: 40 lines in one test file | Once a run for each quality (test; §5) |
| Game 7b (the user's round) | A fast ball jumped mid-roll: Rapier kicks a ball up where it crosses from one collider to the next | A floor a ball rolls fast on is one `MeshCollider` whose pieces share whole edges; walls without friction (physics test; §16) |
| Game 7b | Sharpness is resolution over distance; AO's radius for a small scene; outlines in metres and drawn twice; `Shader(source)`; `uvScale` and the texture slots; what a sphere cast's hit says; a phone's GPU ms can't compare two looks; the look scattered through §5 | §5, §5b (new: making it look good, with costs), §14, §16, §20 |
| Game 7b | On the Note10+: 59.9 fps with the full look; GPU work ×3 over the plain look (clock 179 → 571 MHz), mostly its own shaders and garden; the user's round 2 (no screen effects) 5 % lighter | No finding |

### Missing, until a second game asks

- **Camera collision** (games 1 and 7): an orbit or follow camera goes through
  walls, and a game can't fix it after the rig, which runs after every
  `onLateUpdate`. Game 1 sphere-cast itself and set `orbit.distance`;
  game 7 raised its own camera over a wall or windmill in the way.
  *Recommendation:* the most likely to be asked for again; every
  third-person game needs it. A rig option in fynkel_physics (the casts
  live there), Cinemachine's Deoccluder as the model: cast from the
  target to where the rig put the camera, and pull it in. Needs the rigs
  to let a package adjust the place they computed. Medium.
- ~~**Model bounds**~~ (games 2 and 4): built at the second ask, above.
- **`PhysicsMaterial.staticFriction`** (game 3): one friction serves both;
  a Unity user looks for two. §16 says there's one. *Recommendation:*
  leave it unless a game needs a body that sticks until pushed hard and
  then slides; Rapier has one friction too.
- **A limiter on the mix** (game 5): sounds together past full scale
  are cut flat, so 32 voices can crackle; the game guessed volumes of
  0.3–0.5 by hand. Unity doesn't limit either. *Recommendation:* the
  cheapest of these to build (a soft knee on the master, a few lines in
  the mixer) and it ends a whole class of crackle; worth doing at the
  next ask, or sooner if a device check hears it.
- **Contact darkening at low** (game 7b): ambient occlusion is drawn at
  high only, so on a phone a wall's foot on the side away from the sun
  has no contact; the game laid strips of transparent black along each
  wall. *Recommendation:* wait for a second ask; Unity's answer is baked
  light, which Fynkel doesn't have. A cheap AO at low (half size, fewer
  samples) is the likelier build.
- **Model vertex colours, a mesh's surface as a particle shape, wind**
  (game 7b; all in §21 already): its garden was made of primitives in a
  `MeshBuilder` and stood still. *Recommendation:* each waits for its
  second ask.
- **Two fingers on a phone by script** (game 7): `adb shell input` has
  one finger and the Note10+'s SELinux refuses `sendevent`, so pinch and
  twist were checked in a world test with `input.simulate` and by hand.
  Not the engine's; the next phone game could use a test build with
  Flutter's `integration_test` instead.
- **3D pictures in lists** (game 2): thumbnails of models for a grid.
  One `World` and a `HeadlessView` drawing each model and keeping its
  `png()` worked on the Mac (10 in 237 ms), but `HeadlessView` says it's
  for tests and tools, and `read` blocks the UI thread while it waits.
  *Recommendation:* first measure that way on the phones; if it holds,
  write it up in §17 as the way. An asynchronous `snapshot` only if
  the wait shows.
