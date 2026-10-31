# Stage 14 API pass: `fynkel_flutter` and `fynkel_measure`

Read at `bf27879a` (main). Throwaway tests in `/tmp/s14api/scratch-flutter/test/`
(`api_test.dart`, `rewrap_test.dart`, `route_test.dart`, `early_test.dart`,
`two_test.dart`), run with `flutter test`. Nothing in the repository was changed.
Not repeated: anything in `docs/stage-14-findings.md` or the guide's §21.

Ratings: **bug** 3, **misleading** 3, **awkward** 5, **unhelpful error** 1, **minor** 6.

---

### U1. A bare `FynkelView(root:)` that Flutter makes again destroys the game: **bug**

- **Where**: `packages/fynkel_flutter/lib/src/fynkel_view.dart:566-573` (`_worldOfRoot`: a root already alive is "shown there, and left to whoever made that world") with `:637` (`if (_owns) _rootWorld?.dispose()` in `dispose`).
- **What happens**: when a view that made its own world is replaced by a new `FynkelView` state for the same root, the new state is built first. It sees `root.isAlive`, takes the world without owning it, and then the old state's `dispose` frees that world. The game is destroyed, and the new view fails with `Bad state: this World was disposed`, reported only as "in FynkelView".
- **How a game hits it**: the plain Flutter pattern of a root kept in a `State`, with the view wrapped conditionally (`hud ? Stack(children: [view, Hud()]) : view`). Also a `Row` of unkeyed `SizedBox`es round keyed views. Nothing in the docs warns about it. The class doc says the root is "used once, when the view is first built", which reads as safe.
- **Fix**: an owned world goes when the last view showing it goes, not when the view that made it goes. `_WorldClock.views` already counts the views. Free the world in a post-frame callback and only when `views == 0`, or hand `_owns` to a view still showing it. This doesn't break existing code.
- **Verified**: test (`rewrap_test.dart`): toggling the wrapper gives `alive=false`, a `StateError` from `FynkelViewState._open`, and the game stops updating. `api_test.dart` T7 shows the same with two unkeyed views of one root.

### U2. `pauseWhenCovered` unpauses a game that paused itself while covered: **bug** (its doc promises otherwise)

- **Where**: `fynkel_view.dart:313-316` (doc: "Closing the last one goes on, unless the game was paused already, **or paused itself meanwhile**") against `:498-503` (`_letGo`: `if (world.paused) world.paused = false` whenever the view paused it).
- **What happens**: while the view holds the pause, the game's own `world.paused = true` changes nothing: `World.paused`'s setter returns early on the same value and emits no signal. The view can't tell that the game asked for a pause, so closing the dialog unpauses. Unpausing and then pausing again while covered is also lost. This matters for a game-over or "level complete" set while a dialog or settings page is up: the game runs again when the page closes.
- **Fix**: keep the view's pause apart from the game's. The world could pause while `paused || coveredBy > 0`, with `paused` meaning the game's own. Or the view could watch `onPausedChanged` while covered and drop `_pausedIt` on any change. The first changes what `world.paused` reads under a dialog, and so what `pausedListenable` shows. Arguably that's better: a pause panel wouldn't flash up under every dialog. It's a behaviour change, so a note under "What changed". If neither fix is made, at least correct the doc.
- **Verified**: test (`api_test.dart` T4): covered, then `world.paused = true`, then pop leaves `paused=false`. Pause/unpause/pause while covered, then pop, also leaves `paused=false`.

### U3. `tester.game<T>()` and a view rebuilt with a new root or `make`: **bug** (testing), silent misuse (view)

- **Where**: `packages/fynkel_flutter/lib/testing.dart:45` (`... ?? (view.widget as FynkelView).root`); `fynkel_view.dart:611` ("A new root alone changes nothing"); `fynkel_game.dart` (no `didUpdateWidget`: a new `make` or `source` is ignored until `restart`).
- **What happens**:
  - `tester.game<T>()` returns the current widget's `root`, not the root the view shows. With `build() => FynkelView(root: MyGame())`, which is easy to write, each rebuild makes a new `MyGame` that is never added. `tester.game` then returns that dead object and the test reads zeros.
  - A view given another root (`FynkelView(root: level == 1 ? Level1() : Level2())`), or a `FynkelGame` given another `make`, keeps the old game with no word in a debug build. A Flutter developer expects a changed argument to take effect.
- **Fix**:
  - Keep the shown root in `FynkelViewState` (say `FynkelViewState.root`) and have `game<T>()` read that.
  - In debug builds, report once from `FynkelView.didUpdateWidget` when `!identical(old.root, widget.root)`, with a message like: "FynkelView's root changed from X to Y, but a view shows the root it was first built with. Give the view a new `key` to show a new root, or make the root in a FynkelGame and restart it."
  - Do the same in `FynkelGame` for `make`, if it can be compared (`Level1.new` tears off equal).
  - None of this breaks existing code.
- **Verified**: test (`api_test.dart` T1: after a rebuild, `same=false secondAlive=false secondUpdates=0`; T2: a new root is ignored and nothing is reported). `make` was checked by reading.

### U4. `FynkelApp` hides every `FynkelView` setting: **awkward**

- **Where**: `packages/fynkel_flutter/lib/src/fynkel_app.dart:84` (`final view = FynkelView(root: game);`).
- **What's wrong**: a game started the recommended way has no access to `onStats`, `onError`, `errorBuilder`, `pauseWhenCovered`, `onScreen`, `resolutionScale` or `showErrors`. It can't show an fps counter, report a lost view to a crash reporter, show its own "can't draw" message (BR9's fix), run under a see-through pop-up, or opt out of the Android/iOS surface. The guide sends these to `FynkelView(...)`, so a `FynkelApp` game must be rewritten as `MaterialApp` + `FynkelGame` + `FynkelView` to use any of them.
- **Fix**: add `FynkelApp(view: (context, game) => FynkelView(root: game, onStats: ...))`, defaulting to today's view. Or forward the few that matter: `onStats`, `onError`, `errorBuilder`, `pauseWhenCovered`. Additive, so it doesn't break existing code.
- **Verified**: read.

### U5. Pages pushed from a `FynkelApp` can't reach the game: **awkward**

- **Where**: `fynkel_app.dart:79` (the `FynkelGame` is `MaterialApp.home`, below the `Navigator`); `fynkel_game.dart:40-41` (doc: "A widget below it … reaches the game with `FynkelGame.of`"); `fynkel_app.dart:13-17` (doc: the overlay can "push pages over the game with `Navigator.of(context)`").
- **What's wrong**: a settings or pause page pushed from the overlay is a sibling route, not below the `FynkelGame`. There `FynkelGame.maybeOf` is null and `FynkelGame.of` throws "found no FynkelGame around this context: make it in the builder of one". A pushed page can't be made in the builder. The game must be passed down by hand. The settings page is where quality, volume and controls live, so it is the page that most needs the game.
- **Fix**: have `FynkelApp` hold the game above the `Navigator`. One way is `MaterialApp(builder: (context, navigator) => _GameScope(state, navigator))`, an inherited scope that `FynkelGame.of` also looks for. Restart keeps rebuilding only the home page. Also make the error say "a page pushed with Navigator isn't below the FynkelGame of the page under it: pass the game to it". Neither change breaks existing code.
- **Verified**: test (`route_test.dart`: the pushed `Settings` page gets `FynkelGame.maybeOf<Counter>(context) == null`).

### U6. A bare `FynkelView` never reloads shaders and sounds on hot reload: **misleading**

- **Where**: `fynkel_game.dart:128-139` is the only caller of `World.reloadAssets`. `FynkelViewState` (`fynkel_view.dart`) has no `reassemble`. The guide promises it unconditionally: §14 "Hot reload takes a shader's new code" (`docs/writing-a-game.md:2777`) and §13b "Hot reload reads a changed sound file again" (`:2225`).
- **What's wrong**: a game shown by `FynkelView(root:)` without a `FynkelGame` (as the library doc suggests "inside a Flutter app", see U7), or by `FynkelView.world`, edits a shader, presses `r`, and sees nothing change.
- **Fix**: `FynkelViewState.reassemble` calls `world.reloadAssets()` when the view owns its world (`_owns`). Otherwise the guide says "under a FynkelGame or FynkelApp". Doesn't break existing code.
- **Verified**: read (grep: one caller).

### U7. Which widget to start with: the library doc points inside-an-app games at the bare view: **misleading**

- **Where**: `packages/fynkel_flutter/lib/fynkel_flutter.dart:1-3` ("[FynkelView] for one inside a Flutter app, and [FynkelGame] to make its root and start it over"). It disagrees with `fynkel_app.dart:17-18` and guide §17 ("A game inside a bigger app … uses … a `FynkelGame` … and a `FynkelView`").
- **What's wrong**: what the bare `FynkelView(root:)` lacks is written nowhere in one place:
  - no restart or F5;
  - no asset hot reload (U6);
  - the world exists only once the view has mounted. `Stack(children: [FynkelView(root: game), Stick(game.world.input)])` in a plain `State` throws `Hero (not added) has no world yet: … Add it first (world.add …)`. That advice is wrong for a Flutter developer, whose answer is `FynkelGame`.
  - it can lose the game when Flutter makes the view again (U1).
- **Fix**: library doc: "`FynkelApp` for a game that is the app; `FynkelGame` + `FynkelView` for a game inside one; `FynkelView(root:)` alone for a second view or a quick tool." List the bare form's limits in `FynkelView`'s doc. Optionally the "no world yet" StateError could add "in Flutter, make the root in a FynkelGame, whose world is there before its builder runs". Docs only, so nothing breaks.
- **Verified**: read; test (`early_test.dart` for the error).

### U8. `FynkelView.onScreen` is a bool among `on…` callbacks: **misleading**

- **Where**: `fynkel_view.dart:226`, `:355`.
- **What's wrong**: in Flutter, `onX` is a callback (`onPressed`, and on this widget `onFrame`, `onStats`, `onViewStats` and `onError`, which sits right beside it). `onScreen: false` reads like "no callback for when it's on screen", not "draw into a texture rather than straight to the screen". Developers will guess it wrong when reading other people's code.
- **Fix**: rename it, for example `drawsToScreen`, `directToScreen` or `platformLayer` (`true` by default). Keep `onScreen` as a deprecated parameter: make both `bool?` and resolve them in the initializer. That stays non-breaking for one release.
- **Verified**: read.

### U9. Two views of one world that both take input: nothing says so, errors come twice: **awkward**

- **Where**: `fynkel_view.dart:582` (each input view listens to `world.errors`) and `:1001-1004` (each input view sets `world.input`'s size and `world.input.camera` in its layout).
- **What's wrong**: the doc says to give `input: false` to all but one view, but nothing enforces it. With two input views, every game error is reported twice and shown in two panels. Also, whichever view laid out last sets the size and camera that every tap is aimed with, so taps on the other view hit the wrong place. This is easy to get wrong in split screen, where both halves look interactive.
- **Fix**: count input views per world in `_WorldClock` (as `views` is counted). In debug builds report once: "two FynkelViews of one world take input: give input: false to all but one". Doesn't break existing code.
- **Verified**: test (`two_test.dart`: one throw gave `reports=2`); the size and camera part by reading.

### U10. `FynkelApp`'s overlay isn't in a `Stack`, and `Stick`'s doc says to use `Positioned`: **awkward**

- **Where**: `fynkel_app.dart:39-41` (doc: "on a transparent Material") against `:98-101` (inside an `Align(topStart)` with loose constraints). `packages/fynkel_flutter/lib/src/stick.dart:7-8`: "`Stick(game.world.input)`, placed where it goes (`Positioned(left: 24, bottom: 24, child: ...)`)".
- **What's wrong**: `overlay: (context, game) => Positioned(...)`, the first thing a HUD with one corner widget or a `Stick` tries, fails with Flutter's "Incorrect use of ParentDataWidget … placed inside a Align widget". The overlay's doc doesn't say how it's laid out (loose, from the top-left). The reason is only in a code comment.
- **Fix**: put the overlay in a loose `Stack` filling the screen, so a `Positioned` overlay works and the rest stays as now. Or say in the doc "laid out loosely from the top-left; wrap `Positioned` widgets in a `Stack(fit: StackFit.expand)`". Neither breaks existing code.
- **Verified**: test (`api_test.dart` T3).

### U11. `MeasuredApp` isn't shaped like `FynkelApp`: **awkward**

- **Where**: `packages/fynkel_measure/lib/src/measured_app.dart:33-54` and `:210`.
- **What's wrong**:
  - It isn't generic: `make` returns `GameObject`, and `overlay`, `change`, `timings` and `onStats` get an untyped root. `overlay` also gets no `BuildContext`, where `FynkelApp`'s gets `(context, game)`. Every example casts (`overlay: (game) => Hud(game as ArenaGame)`, sheepdog, lab, arena).
  - It shows a bare `FynkelView(root:)` with no `FynkelGame`, so a HUD that uses `FynkelGame.of(context)` (the guide's "again" button) gets null or throws only under measurement.
  - The overlay is sized to the view rather than loose at the top-left, so the same HUD can lay out differently.
  - The constructor has no doc comment.
- **Fix**: `MeasuredApp<T extends GameObject>(T Function() make, {overlay: Widget Function(BuildContext, T)?, …})`, built on `FynkelGame` like `FynkelApp`. This breaks the overlay's signature in the examples' `tool/measured.dart` (8 files, all in-repo). To avoid that, add the new one as `builder:` and deprecate `overlay`.
- **Verified**: read; examples grepped.

### U12. `input: false` also hides the game's errors: **minor**

- **Where**: `fynkel_view.dart:327-331`, `:558`, `:582`.
- **What's wrong**: a view used only as a background behind a title menu (`input: false`, so the menu's buttons work) shows no errors over the game. If it's a `FynkelGame`'s only view, errors after the first frame go to the zone only (`FynkelGame._reportEarly` stops listening). The doc does say it, but tying two unrelated things to one flag is a surprise.
- **Fix**: have `showErrors` decide on its own, and dedupe per world (U9) so two views don't both show them. Changes the default for `input: false` views; that's a note, not a break.
- **Verified**: read.

### U13. `tester.frames`' doc: "the game updates once in each": **minor**

- **Where**: `testing.dart:26-27`.
- **What's wrong**: the game updates once per frame only at `targetFrameRate` 60 or 0. At 30 it updates every other frame, and covered by a dialog 10 times a second (`_WorldClock.due`). A test counting updates after `frames(n)` is off.
- **Fix**: "the world runs as on a 60 Hz display: once a frame at its default 60". Docs only.
- **Verified**: read.

### U14. Range asserts with no value or advice; release builds don't check: **unhelpful error**

- **Where**: `fynkel_view.dart:230`, `:252` (`assert(resolutionScale > 0 && resolutionScale <= 4)`); `packages/fynkel_flutter/lib/src/world_widget.dart:27` (`assert(shrinkBeyond == null || shrinkBeyond > 0)`).
- **What's wrong**: the message is `Failed assertion: … 'resolutionScale > 0 && resolutionScale <= 4': is not true.`, with no value. In release, `0` silently draws nothing, and NaN throws "Infinity or NaN toInt" every frame (reported once a second from `_sizeFromLayout`). `World.quality` checks the same kind of range with an `ArgumentError` that gives the value.
- **Fix**: `assert(..., 'resolutionScale must be above 0 and at most 4, not $resolutionScale')`, and clamp or ignore a non-finite value at use. Doesn't break existing code.
- **Verified**: test (`api_test.dart` T9: the assert text above); release behaviour by reading.

### U15. `Controls.fromJson` is a method that changes the controls: **minor**

- **Where**: `packages/fynkel_flutter/lib/src/controls.dart:81`.
- **What's wrong**: in Dart, `X.fromJson(json)` is a named constructor. `Controls.fromJson(saved)` won't compile, and `controls.fromJson(saved)` reads like it builds something.
- **Fix**: `load(json)` or `applyJson(json)`, keeping `fromJson` as a deprecated forwarder. Non-breaking.
- **Verified**: read.

### U16. The same idea named differently across the widgets: **minor**

- **Where**:
  - `fynkel_app.dart:21`, `fynkel_game.dart:45`, `measured_app.dart:33`;
  - `fynkel_view.dart:211-252`, `:275-279`.
- **What's wrong**:
  - `make` is positional for `FynkelApp` and `MeasuredApp` but named and required for `FynkelGame`.
  - What builds the UI is `overlay` (`FynkelApp`, over the view) or `builder` (`FynkelGame`, including the view). Defensible, but `MeasuredApp`'s `overlay` has yet another shape (U11).
  - The view's game is named in `FynkelView(root:)` but positional in `FynkelView.world(world)`.
  - `resolutionScale` (per view) and `Quality.renderScale` (per world) are the same knob under two names.
  - `FynkelGame` reads like the class a game extends (Flame's `FlameGame` is exactly that, and its widget is `GameWidget`), which gives `FynkelGame.of(context).game`.
- **Fix**: at least accept `FynkelGame(make, builder:)` positionally too, and call the view's knob `renderScale` with a deprecated alias. Leave the class names unless a rename is planned anyway, or note in the guide that `FynkelGame` is a widget. The aliases keep existing code working.
- **Verified**: read.

### U17. Docs that are stale or missing: **minor**

- **`fynkel_measure` library doc**: `packages/fynkel_measure/lib/fynkel_measure.dart:3` says the game's `lib/measured.dart`. Every example uses `tool/measured.dart`, which `bin/measure.dart:85` tries first. `measured_app.dart:11` already says `tool/`.
- **`fynkel_flutter` library doc** (`fynkel_flutter.dart:1-10`): doesn't name `Stick`, `askForKey`, `bundleSource` or `FynkelViewStats`.
- **No doc comment**:
  - `MeasuredApp`'s, `Measurement`'s, `StatsLine`'s and `BaselineSpinner`'s constructors (`measured_app.dart:33`, `measurement.dart:58`, `:211`, `:238`);
  - `Measurement.dispose` (`:113`);
  - `OnScreenClock.onViewStats` (`on_screen.dart:27`). Also, `next` called again before the first completes leaves the first future pending for good.
- **`FynkelView.source` ignored silently**: for a root already in another world, `source` is dropped with no report (`fynkel_view.dart:566-568`). For a `FynkelGame`'s root the same mistake is reported (`:541-553`). Tested: `api_test.dart` T6 passes with nothing reported.
- **Verified**: read; T6 by test.

### U18. `Stick` can't take the game's look: **minor**

- **Where**: `stick.dart:86-111`.
- **What's wrong**: ring and knob colours, border, shadow and the 13/30 knob ratio are fixed constants. A game with its own art has to copy the widget. Unity's `OnScreenStick` draws the game's own sprite.
- **Fix**: optional `ring`/`knob` widgets or colours, defaulting to today's look. Additive.
- **Verified**: read.

---

## Checked and fine

- **`FynkelGame.of<T>` / `maybeOf<T>`**:
  - `of` with no type argument finds a `FynkelGame<Arena>` (test T10).
  - The mismatch error names both types and says to ask for the made type and cast it.
  - `tester.game<Counter>()` of a `FynkelGame<GameObject>` works.
- **`testing.dart`**: the "no view" and "wrong type" errors say what was found and what to do. `frames`/`run` are rounded as documented.
- **`FynkelView` errors**:
  - the bounded-size `FlutterError` gives the constraints and what to wrap the view in;
  - `source:` on a `FynkelGame`'s root is reported with a clear `ArgumentError` and the view goes on;
  - an unsupported platform gets an `UnsupportedError` naming the three supported ones.
- **A world disposed under a live `FynkelView.world`**: no throw. The view keeps its last picture and stops updating the world, as `World.dispose`'s doc says (test T5/T5b).
- **Using the world before the view loads it**: through `FynkelGame` the world is there before `builder` runs (documented and true). Through `FynkelViewState.world` it's set in `didChangeDependencies`, before a `GlobalKey` can reach it.
- **`pauseWhenCovered`** for "paused before covered" and for stacked dialog plus page: correct (the repo's tests, and read).
- **`Controls`**: `[]`/`set` errors name the unknown name and list the known ones. `set` swaps rather than duplicating keys. `fromJson` skips unknown names and keys.
- **`WorldListenables.pausedListenable`** and **`ListenableSignal.changed`**: one instance each per world or listenable, as documented. `WorldTheme` defaults and docs match the code.
- **`FlutterColorToFynkel` / `FynkelColorToFlutter`**: P3 is handled as documented.
- **`FynkelApp` defaults**: dark theme, `restartOnReload: false`, `title: ''`. Sensible, and documented.
- **`FynkelView` defaults**: `autofocus: true`, `showErrors: kDebugMode`, `pauseWhenCovered: true`, `input: true`, `worldWidgets: true`, `onScreen: true`. Sensible for a game, apart from the naming (U8) and coupling (U12).
- **`FynkelViewStats`**: `freshFps` against `tickFps` is documented clearly. `onStats` against `onViewStats` is explained (per second vs cumulative per frame).
- **`fynkel_measure`**: `footprint`, `physicalFootprint`, `thermalState`/`thermalStates` and `bench`/`castTimes` docs match the code (null off Apple, PSS on Android).
