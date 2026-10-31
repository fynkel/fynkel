# Stage 14 API pass: package:fynkel features

This covers animator, model, motion, motion_kinds, particles, trails, shader, sky, cameras, splines, spline_objects, mesh_building, resources, text, math and color. Paths are relative to `packages/fynkel/lib/src/` unless they say otherwise. Every claim was checked either by reading the code or by a throwaway test in `/tmp/s14api/scratch-features/test/probe*_test.dart`. Findings already in docs/stage-14-findings.md or the guide §21 are left out.

Note for testers: `world.errors` is a broadcast stream. A test that does `update()` and then reads what it collected without an `await` sees nothing, so wait for a microtask first. My first probes missed errors because of this. That is not a finding, but it is easy to get wrong.

---

## F1 — Extruding a zero-size shape panics the engine (bug)
- **Where:** `resources.dart:193` `Mesh.extrude`, which calls `crates/fynkel_mesh/src/shapes.rs:261` `.expect("the built-in shapes are valid")`.
- **Now:** `Mesh.extrude(Shape2D.circle(0))` throws `FynkelException(panic): panicked: the built-in shapes are valid: NoTriangles` and prints a Rust panic backtrace to stderr. The engine still works afterwards.
- **Wrong:** a size of 0 can come from game data (say a radius from a slider), and it reaches an `expect` that assumes it can't happen. A panic is the wrong kind of error: it doesn't name the value and it looks like an engine crash. `Shape2D.circle` and `Shape2D.rect` don't check their sizes either, while `Mesh.sphere(radius: 0)` gives a clear "radius is 0; it must be more than 0".
- **Fix:** have `build()` return the `MeshError` and map it to `ArgumentError('a shape to extrude needs an area; Shape2D.circle(0) has none')`. Better still, check the sizes in `Shape2D.circle` and `Shape2D.rect` (more than 0 and finite), as the mesh factories do.
- **Breaks game code:** no. It only changes which error a call that already fails throws.
- **Verified:** test (probe2, "extrude circle 0").
- **Rating:** bug.

## F2 — An odd VirtualCamera value, or a NaN camera shake, turns off the whole camera system for good (bug)
- **Where:** `cameras.dart:56` (`VirtualCamera({… this.fovY = 60.deg, this.orthographicSize = 5})`, fields set with no check) and `cameras.dart:472` (`Cameras.shake(strength, …)`, strength not checked). Both fail later, in `_put` at `cameras.dart:612` and in `_shake`. The world loop at `world_loop.dart:194–213` then turns the system off.
- **Now:** `VirtualCamera(fovY: 60)` (degrees from habit) is accepted. Each frame after that reports `Invalid argument(s): Camera.fov_y: 3437.7468 is outside 1..=179` to `world.errors`, with no object. After 5 frames comes `Bad state: _CameraWorld threw in 5 frames in a row, so it was turned off`. From then on no rig, virtual camera, blend or shake runs again in that world. `world.cameras.shake(double.nan)` goes the same way, reporting `a shake of [NaN, NaN, NaN] isn't finite` every frame.
- **Wrong:** one bad number given to one virtual camera stops every camera feature for the rest of the session. The error names neither the VirtualCamera nor its object, uses the engine's snake_case name, shows a degree value the user never typed, and names a private class (`_CameraWorld`). `Camera(fovY: 60)` fails at once with the same message, so the VirtualCamera path is the only one that defers it.
- **Fix:** check `fovY` (more than 0 and less than π) and `orthographicSize` (more than 0, finite) in the VirtualCamera constructor and setters. Check `strength` (finite, 0 or more) in `Cameras.shake`, and make it say `VirtualCamera.fovY: 60 is radians; 60.deg is likely what you meant`. Do the same in `GameObject.shake` (`motion.dart:1030`), where NaN currently fails at the frame with `Transform.position: NaN or infinity`. Optionally also catch per virtual camera, as `_safely` already does per rig, so one bad camera can't switch the rest off.
- **Breaks game code:** no. Code that hits this is already broken.
- **Verified:** test (probe4 for the VirtualCamera, probe5 for the NaN shakes, both reading `world.errors` after a microtask).
- **Rating:** bug.

## F3 — SplineAnimate throws every frame once its container's object is destroyed (bug)
- **Where:** `spline_objects.dart:446` `SplineAnimate.onUpdate` calls `container.length(...)`, which reads `gameObject.worldTransform` at `spline_objects.dart:42`.
- **Now:** destroy the track while a cart rides it, and every frame reports `GameObject#1 was destroyed, so its worldTransform can't be read`.
- **Wrong:** a track getting removed is ordinary (the level unloads, the track is rebuilt). Follow, LookAt and Orbit hold still when their target goes; SplineAnimate floods the error stream instead, and the message points at the track rather than the animate.
- **Fix:** in `onUpdate`, when `container.gameObject` is destroyed, stop (and fire the end as for a finished `once`) or hold still, the way Follow does. Document which one it does.
- **Breaks game code:** no.
- **Verified:** test (probe, "splines").
- **Rating:** bug.

## F4 — Animator.current returns a blend's name that the rest of Animator rejects (misleading)
- **Where:** `animator.dart:93` `current`, `:101` `isPlaying`, `:105` `weight`, `:43` `crossFade`, `:32` `play`. The error is thrown around `:264`.
- **Now:** after `blend('move', [...])`, `current` is `'move'`, but `isPlaying('move')`, `weight('move')`, `crossFade('move')` and `play('move')` all throw `Invalid argument (clip): not one of the model's clips (Survey, Walk, Run): "move"`.
- **Wrong:** `isPlaying(a.current!)` is the obvious check and it throws. `crossFade(current)` to return to a blend is natural too. The message also says `clip` where the parameter is `name`.
- **Fix:** make `isPlaying` and `weight` accept blend names (sum of the blend's clips, or the blend's own weight). Make `crossFade`/`play` with a blend name go back to that blend, or say "'move' is a blend; call blend('move', …) to change it". At the least, document on `current` that it can be a blend's name.
- **Breaks game code:** no. It widens what is accepted.
- **Verified:** test (probe, "animator blend names", Fox.glb).
- **Rating:** misleading.

## F5 — Error messages print "Instance of 'X'" for the value (unhelpful error)
- **Where:** every `ArgumentError.value(obj, …)` whose value isn't a number or string: `resources.dart:1405` (shader), `resources.dart:1053` (values map), `resources.dart:388` (combine list), `shader.dart:180`, `shader.dart:440`, `:452`, `:515` (material values), `splines.dart:245`, `spline_objects.dart:212` (offset), `motion_kinds.dart:87`, the fog height check, and `animator.dart:281/283/292`.
- **Now (examples):** `m.set('tint', Vec2(1, 1))` gives `Invalid argument (tint): is no value for Shader(probe.wgsl)'s tint, which is vec3<f32>: Instance of 'Vec2'`. `Material(values: {'a': 1.0})` gives `…: _Map len:1`. `Mesh.combine([])` gives `Instance(length:0) of '_GrowableList'`. The fog height check gives `Invalid argument (fog): height must be above 0: Instance of 'Fog'`.
- **Wrong:** `ArgumentError.value` uses `Error.safeToString`, which ignores `toString` for anything that isn't a number, bool, string or null. The value the user passed is the most useful part of the message and it is lost. Vec2, Vec3, Fog and Shader all have good `toString`s that go unused.
- **Fix:** add one internal helper, `ArgumentError('$name: $message; got $value')`, which calls `toString`, and use it at these sites.
- **Breaks game code:** no (only the message text changes).
- **Verified:** test (probe, probe2) and read.
- **Rating:** unhelpful error.

## F6 — Particle errors lump settings together and leave out the value; the cone angle speaks degrees (unhelpful error)
- **Where:** `crates/fynkel_world/src/particles.rs:589`, `:609`, `:622`, `:677`, and `particles.dart:1332`.
- **Now:**
  - `ps.simulationSpeed = -1` gives `a particle system's start delay, rates over time and distance and simulation speed can't be below 0`.
  - `Burst(probability: 2)` gives `a burst's time and count can't be below 0, its interval is at least 0.01 s, and its probability is 0 to 1`.
  - `ParticleShape.cone(angle: 25)` gives `a cone's angle is 0 to under 90°`.
  - `startRotation = Range(double.nan)` gives `a ParticleSystem's ranges must be finite; got Range(NaN, NaN)`.
- **Wrong:** the user has to work out which of four settings is wrong, and the value isn't shown. The cone angle is the worst case: the API takes radians, the user passed 25 thinking in degrees, and the message's "90°" confirms their wrong model. The range message doesn't say which range.
- **Fix:** one message per setting, with its value, for example `simulationSpeed is -1; it can't be below 0`, `a burst's probability is 2; it's 0 to 1`, `ParticleShape.cone angle is 25 (radians); it's 0 to under π/2 (90.deg)`, `startRotation must be finite; got Range(NaN, NaN)`. The mesh shape messages already follow this style ("segments is 2; it must be from 3 to 1024").
- **Breaks game code:** no.
- **Verified:** test (probe, probe3) and read.
- **Rating:** unhelpful error.

## F7 — `Mesh.custom` count errors talk in floats where the caller passed vectors (unhelpful error)
- **Where:** `resources.dart:493` (`must be $per per position: ${count * per}`).
- **Now:** 4 positions and 3 Vec2 UVs give `Invalid argument (uvs.length): must be 2 per position: 8: 6`. Normals behave the same way.
- **Wrong:** the caller passed a `List<Vec2>` of 3. "2 per position", "8" and "6" are the flattened float counts, so the message doesn't match their code, and the `: 8: 6` order is hard to read.
- **Fix:** in the list-taking factory, say `uvs has 3; it needs one per position, 4`. Keep the float wording for the flat setters (`setUVs` and the like).
- **Breaks game code:** no.
- **Verified:** test (probe, "custom uv count").
- **Rating:** unhelpful error.

## F8 — Zero-direction errors don't name the argument; MeshBuilder's zero scale shows a NaN vector (unhelpful error)
- **Where:** `math.dart:154` (`'$this has no direction'`), reached from `Ray` (`math.dart:359`), `Plane` (`:386`), `Quat.axisAngle` (`:217`) and `rotateBy`. Also `mesh_building.dart:251` `addMesh`.
- **Now:** `Ray(Vec3.zero, Vec3.zero)` gives `Vec3(0.0, 0.0, 0.0) has no direction`. `MeshBuilder().addMesh(cube, scale: Vec3(1, 0, 1))` gives `Vec3(NaN, NaN, NaN) has no direction`.
- **Wrong:** neither says which argument it was (Ray's direction, Plane's normal, the axis). The addMesh one shows a vector the user never wrote; the real cause is the 0 in the scale (an inverse-transpose for the normals). `Mesh.combine` with the same scale may meet the same thing.
- **Fix:** check at the call sites: `Ray.direction is Vec3.zero; it needs a direction`, `Quat.axisAngle axis is …`. In addMesh, either refuse `scale` with a 0 (`scale has a 0; a flat copy has no normals`) or handle it by keeping the normals un-normalized there.
- **Breaks game code:** no.
- **Verified:** test (probe, "math messages" and "builder addMesh flat scale").
- **Rating:** unhelpful error.

## F9 — Animator uses `duration:` where the rest of the API says `seconds:` (awkward)
- **Where:** `animator.dart:43` `crossFade(name, {duration = 0.25})`, `:68` `blend(…, duration)`, `:80`/`:337` `stop({duration = 0})`, and the message at `:125`.
- **Now:** `moveTo(…, seconds:)`, `Blend(seconds:)`, `cameras.shake(seconds:)`, `wait(seconds:)`, debug draws and audio fades all use `seconds`. Animator alone uses `duration`.
- **Wrong:** the user types `crossFade('Run', seconds: 0.2)`, gets a compile error, and has to look it up. Unity's `CrossFade` does call it `normalizedTransitionDuration`/`fixedTransitionDuration`, but the house style here is `seconds` everywhere else.
- **Fix:** rename to `seconds`. While doing so, also reject infinity (`crossFade(duration: double.infinity)` is accepted today, so the fade never ends).
- **Breaks game code:** yes, for callers that pass `duration:`. To keep it working, accept both for a release (`{double? seconds, @Deprecated('use seconds') double? duration}`) and throw if both are given.
- **Verified:** read. The infinity case was tested (probe).
- **Rating:** awkward.

## F10 — `ParticleSystem.lit` says it lights a trail's points, but trails can't be lit (misleading)
- **Where:** `particles.dart:1144–1148` ("Each particle (or a trail's point) is lit once…").
- **Now:** TrailRenderer has no `lit`, its doc (`trails.dart:9`) says "drawn … as particles are: unlit", and FyTrailSettings has no lit field. Particles have no trail module of their own either.
- **Wrong:** a reader looks for a way to light a trail and there isn't one.
- **Fix:** drop "(or a trail's point)", or add `TrailRenderer.lit` if that's planned.
- **Breaks game code:** no.
- **Verified:** read (particles.dart, trails.dart, the settings struct).
- **Rating:** misleading.

## F11 — Public docs point to files that don't exist or to internal plans (misleading)
- **Where:** `shader.dart:3` ("docs/guide.md, \"Shaders\""; there's no docs/guide.md, the guide is docs/writing-a-game.md §14) and `motion.dart:157` ("docs/stage-4.md §3.2"). The same internal references are also in library comments at `cameras.dart:3`, `debug.dart:3`, `layers.dart:3`, `input.dart:3`, `pointing.dart:3` and `game_object.dart:246/343`; the dartdoc ones (`///`) are the ones users see.
- **Wrong:** someone reading the `Shader` dartdoc follows a dead link. Stage plans aren't user documentation.
- **Fix:** point to `docs/writing-a-game.md` and its section, or drop the reference.
- **Breaks game code:** no.
- **Verified:** read, with an `ls docs/`.
- **Rating:** misleading.

## F12 — Missing everyday helpers that the library itself works around (awkward)
- **Where:** `color.dart` (no `withAlpha`), `math.dart` Vec2 (no `normalized`, `normalizedOrZero`, `distance`), and Quat (no read-back to euler angles, no `fromToRotation`, no `rotateTowards`).
- **Now:** the library writes `Color.rgb(c.r, c.g, c.b, a)` by hand at `resources.dart:1377` and `color.dart:52`/`:57`, and the lab does too (`see_through.dart:70`). `(Vec2(3, 4) as dynamic).normalized` throws NoSuchMethodError. Examples work out yaw with `math.atan2` by hand.
- **Wrong:** these are the first things a Unity user reaches for (`color.a = …`, `Vector2.normalized`, `transform.eulerAngles.y`, `Quaternion.FromToRotation`, `RotateTowards`).
- **Fix:** add `Color.withAlpha(double)`, Vec2 `normalized`/`normalizedOrZero`/`distance` mirroring Vec3, `Quat.eulerAngles` (Vec3 in radians, the inverse of `Quat.euler`), `Quat.fromTo(a, b)` and `Quat.rotateTowards(from, to, maxRadians)`.
- **Breaks game code:** no (it only adds).
- **Verified:** read, plus a test for Vec2.
- **Rating:** awkward.

## F13 — `Cameras.shake` defaults `seconds`; `GameObject.shake` requires it (minor)
- **Where:** `cameras.dart:472` `shake(strength, {seconds = 0.4, …})` against `motion.dart:1030` `shake(strength, {required seconds, …})`.
- **Wrong:** two calls with the same name and shape behave differently. Separately, `cameras.shake(1, seconds: 0)` is silently ignored, while the motion helpers check `seconds`.
- **Fix:** pick one rule: either both default to 0.4 or both require it. Make `seconds <= 0` throw.
- **Breaks game code:** only if `required` is added to Cameras.shake. Giving GameObject.shake a default breaks nothing.
- **Verified:** read and test (probe, "cameras shake seconds 0").
- **Rating:** minor.

## F14 — `Mesh.lathe` calls its slices `sides`; the other round meshes say `segments` (minor)
- **Where:** `resources.dart:222` `Mesh.lathe(profile, {int sides = 24})` against `Mesh.sphere`, `Mesh.cylinder`, `Mesh.cone` and `Mesh.torus`, which use `segments` for slices around.
- **Fix:** rename to `segments`, keeping `sides` as a deprecated alias for a release.
- **Breaks game code:** yes, for `sides:` callers; the alias covers it.
- **Verified:** read.
- **Rating:** minor.

## F15 — Particle start ranges accept negative sizes and min > max (minor)
- **Where:** the particle setters (`startSize`, `startSpeed`, …) and `particles.dart:1332`, which checks only that ranges are finite.
- **Now:** `ps.startSize = Range(-1)` and `ps.startSpeed = Range(5, 1)` are both accepted with no error.
- **Wrong:** a negative size draws nothing (or a flipped quad). A min above the max is very likely a swap. Lifetime and the rates are checked, so this is inconsistent.
- **Fix:** refuse a negative `startSize` or `startLifetime` (`startSize is Range(-1, -1); sizes can't be below 0`). Either swap or refuse min > max; document which.
- **Breaks game code:** only for code that set nonsense, which drew nothing anyway.
- **Verified:** test (probe).
- **Rating:** minor.

## F16 — `ColorGradient.fadeInOut` takes positional optionals, and its error describes stops the caller never wrote (minor)
- **Where:** `color.dart:56` `fadeInOut([Color color = Color.white, double fadeIn = 0.2, double fadeOut = 0.4])`, and `color.dart:44`.
- **Now:** `fadeInOut(Color.white, 0.6, 0.6)` gives `a gradient's stops are 0 to 1, in order; got [0.0, 0.6, 0.4, 1.0]`.
- **Wrong:** `fadeInOut(c, 0.3)` doesn't say which number is which. Everything else in the API names its numbers. The error shows a derived stop list (0.4 is `1 - fadeOut`) instead of saying `fadeIn + fadeOut must be at most 1; got 0.6 + 0.6`.
- **Fix:** named parameters `{fadeIn = 0.2, fadeOut = 0.4}` with the color kept positional, plus its own check and message.
- **Breaks game code:** yes, for positional callers. Keep the old form working for a release by adding the named form as a new factory, `ColorGradient.fade(...)`, and deprecating the old one.
- **Verified:** test (probe) and read.
- **Rating:** minor.

## F17 — `Spline.positionAt(NaN)` returns a NaN position silently (minor)
- **Where:** `splines.dart:288` (and the other `…At` methods that take a distance).
- **Now:** `positionAt(double.nan)` returns `Vec3(NaN, NaN, NaN)`. The NaN only surfaces later, wherever the position is used.
- **Fix:** `if (!distance.isFinite) throw ArgumentError('distance must be finite; got $distance')`. That costs little and is in the style of `SplineAnimate.distance` (`spline_objects.dart:390`), which already checks.
- **Breaks game code:** no.
- **Verified:** test (probe).
- **Rating:** minor.

## F18 — "a mesh needs at least one triangle" for a lathe on its axis or a sweep of a zero shape (minor)
- **Where:** `crates/fynkel_mesh/src/lib.rs:217`, reached from `Mesh.lathe` and `Mesh.sweep`.
- **Now:** `Mesh.lathe([Vec2.zero, Vec2(0, 1)])` (the whole profile at x = 0) gives `a mesh needs at least one triangle`.
- **Wrong:** true, but it doesn't say why. For the lathe it would help to say "the profile lies on the axis (x = 0), so turning it makes no surface", and for the sweep, "the shape has no size".
- **Fix:** check those cases in Dart (or in the shape builders) with a message that says why.
- **Breaks game code:** no.
- **Verified:** test (probe2).
- **Rating:** minor.

---

## Count by rating
- bug: 3 (F1, F2, F3)
- misleading: 3 (F4, F10, F11)
- unhelpful error: 4 (F5, F6, F7, F8)
- awkward: 2 (F9, F12)
- minor: 6 (F13–F18)

## Checked and found fine
- **Mesh factory messages:** they give the setting, the value and the range, for example "segments is 2; it must be from 3 to 1024", "radius is -1; it must be more than 0" and "inner is 1; it must be less than 0.5". The index and count errors in `setTriangles` and `setColors`, `Texture2D.rgba` size checks, and the StateError for changing a built-in mesh are clear too.
- **Material:** its range checks name the setting and give the value. The rule that the colour's alpha doesn't make a material see-through but `opacity:` does is surprising, but it's documented. `set` on a material without a shader is a clear error, and `set` with an unknown name lists the shader's names.
- **Motion:** the `tween` int hint (0 against 0.0) and the StateError when `colorTo` finds no renderer are good. Seconds and speed are checked (the both/neither message could say which, but it's fine).
- **Fog and sky:** the fog start/end check ("fog.end 5 must be beyond its start 10") and the sky value checks are fine.
- **Math:** `Quat.lookRotation` along up is handled. Radians plus `.deg` are used throughout, as the guide says.
- **Particles:** the checks on duration, softness and `simulate(-1)` and `simulate(NaN)` are fine. Setters revert the old value when the engine refuses. The rule that nothing is drawn without a material is documented in the guide §13c.
- **TrailRenderer** names follow Unity: `time`, `minVertexDistance`, `widthMultiplier`, `widthCurve`, `colorGradient`, `emitting`, `positionCount`, `clear`.
- **Splines:** the errors for sweeping with fewer than 2 knots or a zero length are clear. SplineInstantiate's checks on `spacing`, `count` and `offset` are right apart from F5's display problem, and the different spaces it uses are documented.
- **Model, Text and Animator:** apart from F4 and F9, model loading and instantiating, `Mesh.text`, the Animator's play/loop defaults and the clip-name error that lists the clips are fine. That a layer's `strength` is clamped is documented.
