# The feature lab

One scene for each feature of Fynkel, to try it by hand: each has buttons,
switches and sliders for what the feature does, a readout of its state, and
a log of what happened. A scene whose name ends in "the basics" is the
smallest one for its feature; start with those. Each new feature adds its
own scene here.

The scenes, in the lab's order:

| Scene | What to try |
| --- | --- |
| **Objects and the frame** | |
| Components | Pick a crate, put Spin, Bob and Pulse on it and take them off |
| Lifecycle | Turn objects and behaviours on and off, destroy and rebuild, a buggy cube |
| Time | The time scale, pausing, maxDelta, and a hitch |
| Routines | A spawner, a traffic light with a `finally`, and a race against a timeout |
| Signals: the basics | A doorbell with one signal, and lamps that listen with `on` |
| Signals in a game | Enemies' `died`, a scoreboard, `listen` against `on`, a door awaiting `next` |
| **Input** | |
| Input: fingers and keys | Every gesture logged, a dial to pinch and twist, a puck on WASD and space |
| Input on objects | Pads that light, sink, count clicks and bubble; crates you drag |
| Input: Flutter controls and pause | Sliders and a button that drive the game's axis and action; `world.paused` |
| **Moving things** | |
| Motion: the basics | One cube: move, spin, grow, colour, and all four in a sequence; tap it to punch |
| Motion: eases | Every ease side by side, in, out and in-out; seconds, time scale, complete |
| Motion: tweens and sequences | A patrol sequence, a SmoothDamp follower, a punched crate, a door awaited in a routine, a race |
| Moving: the basics | `position` against `localPosition`, `translate` and `rotate` in the cube's space or the world's |
| **Shapes, pools, colours and cameras** | |
| Shapes: the basics | The built-in meshes at Unity's sizes, and a quad's back |
| Pooling: the basics | A fountain of balls from a `Pool`, and an `ObjectHandle` that lets go |
| Colour: the basics | One shared material, a cube's own colour (`renderer.color`), `Material.copy` |
| Cameras: the basics | Two virtual cameras on a cube: switch (blend or cut), orthographic, shake |
| Cameras: rigs | Chase (Follow + LookAt), orbit and corner cameras on a running ball; tap the floor |
| Layers and debug: the basics | A ray through two layers, a camera that leaves one out, debug drawing |
| **Physics** (package:fynkel_physics) | |
| Fixed step: the basics | `onFixedUpdate` at 50 a second, drawn as it is or between steps |
| Physics queries: the basics | A raycast sweeping over colliders; a sphere cast, a trigger, a skipped layer |
| Rigidbodies: the basics | A cube and a ball that fall and roll; `addForce`, gravity off |
| Rigidbodies: a stack | Ten boxes that sleep once still; a thrown ball; interpolation and the step's thread |
| Rigidbodies: a ball through walls | A fast ball through thin walls, and continuous collision detection |
| Triggers and collisions: the basics | A trigger slab that lights up, a ball that hears the floor |
| The layer matrix | `ignoreLayerCollision` and `ignoreCollision` |
| Mesh colliders: the basics | A ball rolling down a model's real shape, or its hull |
| Character controller: the basics | Walk, jump, climb a low step but not a high one, push a crate |
| Character controller: a course | Stairs, ramps, the slope limit and step offset, coins to pick up |
| Riding a platform | A player that rides sliding and turning platforms |
| Joints: the basics | A door on a hinge with a spring, a chain, a crate on a spring |
| Joints: a bridge | Planks on hinges; breakable joints and `onJointBreak` |
| Joints: a chain and a weight | Ball joints you drag by the weight |
| Joints: a character joint | Two arms on character joints, with and without limits |
| **Add-ons** | |
| Add-ons: the basics | A package's `WorldSystem` that counts components; an invisible button made of a collider |

The scenes use the lab's helpers (`LabScene`, its `shape()`,
`floorCollider()`, `log` and `controls`), so they don't copy into a game
as they are: the guide (https://fynkel.com) has the code to copy.

## Running it

```sh
cd examples/lab
flutter run -d macos     # or an iPhone, an Android phone, a simulator
```

On a phone the panel sits under the game; the button at the top right
hides it.

## Tests

- `flutter test` runs each scene for a few hundred frames, uses every
  control, and checks nothing went wrong (except the buggy cube's errors,
  which are the point).
- `flutter test integration_test -d <device>` opens and closes every scene
  again and again on a real device or simulator, with the real plugin.

## Adding a scene

Make a `LabScene` in `lib/scenes/` (its `title`, `step`, `about`,
`controls` and `build`), log to `log`, show state in `readout`, and add it
to the list in `lib/scenes.dart`.
