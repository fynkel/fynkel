# The feature lab

One scene for each feature of Fynkel, to try it by hand: each has buttons,
switches and sliders for what the feature does, a readout of its state, and
a log of what happened. Every step of `docs/stage-4.md` adds its own
scene(s) here.

| Scene      | Step | What to try                                                                 |
| ---------- | ---- | --------------------------------------------------------------------------- |
| Components | 1    | Pick a crate, put Spin, Bob and Pulse on it and take them off               |
| Lifecycle  | 2    | Turn objects and behaviours on and off, destroy and rebuild, a buggy cube   |
| Time       | 2    | The time scale, pausing, maxDelta, and a hitch                              |
| Routines   | 3    | A spawner, a traffic light with a `finally`, and a race against a timeout   |
| Signals: the basics | 4 | A doorbell with one signal, and lamps that listen with `on` |
| Signals in a game | 4    | Enemies' `died`, a scoreboard, `listen` against `on`, a door awaiting `next` |
| Input: fingers and keys | 5 | Every gesture logged, a dial to pinch and twist, a puck on WASD and space |
| Input on objects | 5 | Pads that light, sink, count clicks and bubble; crates you drag |
| Motion: eases | 6 | Every ease side by side, in, out and in-out; seconds, time scale, complete |
| Motion: tweens and sequences | 6 | A patrol sequence, a SmoothDamp follower, a punched crate, a door awaited in a routine, a race |

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
