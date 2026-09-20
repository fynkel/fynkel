# Input

The input contract: blueprint §5.1 (held state and edges), §9 "Input",
§19.2d item 7 (M3-07; C2, D3). Games read **named actions**, never keys:
the project file declares them, the world compiles them into a binding
table, hosts feed raw control events once per frame, and behaviours read
`Input.action2D('move')` and friends through the current `GameContext`.
The same actions come from the keyboard, a mouse, a gamepad, the touch
overlay or a recording — a gameplay test needs no window.

## Actions (`fynkel.project.json` → `inputActions`)

```json
"inputActions": {
  "move": { "type": "axis2d", "bindings": ["keyboard:WASD", "gamepad:leftStick", "touch:overlay/stick"] },
  "jump": { "type": "button", "bindings": ["keyboard:Space", "gamepad:south", "touch:overlay/jump"] },
  "pause": { "type": "button", "bindings": ["keyboard:Escape", "gamepad:menu"] }
}
```

| `type` | Reads as | Held | Edges |
| --- | --- | --- | --- |
| `button` | `held`, `pressed`, `released` (`axis` is 1 or 0) | any bound button down | one `pressed`/`released` per press of any bound button |
| `axis1d` | `axis` in `-1..1` (mouse wheel: unclamped steps) | non-zero | none |
| `axis2d` | `action2D` in the unit square (mouse delta: unclamped pixels) | non-zero | none |

Several bindings **compose**: key pairs and sticks are summed, then
clamped (a relative mouse control leaves the sum unclamped). `keyboard:WASD`
plus a stick pushed left reads `-1`, not `-2`.

### Binding grammar (`<device>:<control>`)

`fynkel_core::input::parse_binding` is the one place that knows the
controls; `fynkel validate` checks every binding with it (`FYN-P0009`,
anchored at `/inputActions/<name>/bindings/<i>`) and the runtime reports a
binding it cannot compile in the load report (`input.bindingErrors`).

| Device | `button` | `axis1d` | `axis2d` |
| --- | --- | --- | --- |
| `keyboard` | a key name: `A`–`Z`, `0`–`9`, `Space`, `Enter`, `Escape`, `Tab`, `Backspace`, `Delete`, `Insert`, `Home`, `End`, `PageUp`, `PageDown`, `CapsLock`, `ArrowUp/Down/Left/Right`, `ShiftLeft/Right`, `ControlLeft/Right`, `AltLeft/Right`, `MetaLeft/Right`, `Minus`, `Equal`, `BracketLeft/Right`, `Backslash`, `Semicolon`, `Quote`, `Backquote`, `Comma`, `Period`, `Slash`, `F1`–`F12` (physical keys: WASD is the same four keys on every layout) | `<neg>/<pos>`, e.g. `keyboard:Q/E` | a layout — `WASD`, `Arrows`, `IJKL` — or `<left>/<right>/<down>/<up>` |
| `mouse` | `left`, `right`, `middle`, `back`, `forward` | `wheel` (relative: scroll steps this frame, positive away from the user) | `delta` (relative: physical pixels of movement this frame, y up) |
| `gamepad` | `south`, `east`, `west`, `north` (A/B/X/Y — cross/circle/square/triangle), `leftShoulder`, `rightShoulder`, `leftTrigger`, `rightTrigger`, `leftStickButton`, `rightStickButton`, `dpadUp/Down/Left/Right`, `menu`, `options`, `home`; `start` = `menu`, `select`/`back` = `options` | `leftTrigger`, `rightTrigger`, `leftStickX/Y`, `rightStickX/Y` | `leftStick`, `rightStick`, `dpad` |
| `touch` | `overlay/<name>` — a button of the `TouchOverlay` | — | `overlay/<name>` — a stick of the `TouchOverlay` |

Gamepad names are the GameController framework's; every controller Apple
pairs (Xbox, PlayStation, MFi) maps to them.

## The rules (§5.1)

Raw controls carry **held** state (a button down, an axis value) and
**edges** (went down, went up). The world resolves them into action state
that a behaviour reads:

1. **Held state is what every fixed tick sees.** A key held through a
   50 ms frame is held on all three of its catch-up ticks.
2. **An edge is consumed by exactly one tick.** Edges captured since the
   last tick are pending; the next tick sees them as `pressed`/`released`
   and `World::step` consumes them (`Input::end_tick`). A press during a
   frame that runs three catch-up ticks triggers on the first and the other
   two see it held; a jump pressed once jumps once. A frame too short to
   tick (`plan.steps == 0`) carries its edges to the next tick — nothing
   is lost.
3. **`update`/`lateUpdate` see the frame's raw edges** (`framePressed`/
   `frameReleased`: went down or up during this host frame, whatever the
   ticks did) for presentation-only reactions. `fixedUpdate` and
   `afterPhysics` see the tick view.
4. **A `reset` releases everything** with `released` edges — the host
   sends one when the viewport loses focus, the loop is paused or the app is
   hidden, so nothing stays held across a pause.
5. **Relative controls start every frame at zero** (`mouse:delta`,
   `mouse:wheel`) and sum the frame's movement.

The wire: `fynkel.rec.InputEvent { device, control, phase (down | up |
axis | move | reset), x, y }` batches in (`FYNKEL_BUFFER_INPUT_EVENTS`,
`fynkel_world_capture_input`), `fynkel.rec.InputAction { name, kind, held,
pressed, released, frame_pressed, frame_released, x, y }` per action out
(`FYNKEL_BUFFER_INPUT_ACTIONS`, `fynkel_world_input_view`, the world's own
buffer — read in place, no copy). A malformed batch is
`FYNKEL_STATUS_PROTOCOL` and the world stands; an unknown device or phase
skips the record. Tests: `crates/fynkel_core/tests/input.rs` (the three
catch-up ticks, the zero-tick frame, composition and reset),
`crates/fynkel_native/tests/load.rs` (bind on load, capture and view through
the ABI), `packages/fynkel/test/input_test.dart` (the same from Dart, with
a `Reader` behaviour logging every callback).

## Reading actions (Dart)

```dart
@FynkelBehaviour(id: 'game.Mover')
class Mover extends Behaviour {
  @override
  void fixedUpdate(FixedUpdateContext context) {
    final move = Input.action2D('move');            // Vec2 in the unit square
    if (Input.pressed('jump')) jumps++;             // once per press
    if (Input.held('run')) speed = 2;               // every tick while down
    final zoom = Input.axis('zoom');                // -1..1 (wheel: steps)
    final s = Input.state('jump');                  // the whole ActionState, null when unbound
  }
}
```

`Input` is a static facade over `GameContext.current.input`
(`InputService`); outside a callback it throws like `Time`. An action the
project does not declare reads as nothing (`false`, `0`, `Vec2.zero`).
The world's table is `world.input` (`WorldInput`: `actions`, the same
readers, `refresh()`), decoded in place from the native view before each
tick and — with the frame's edges — before `update`; `LoadReport.
inputActions` lists what bound (`BoundAction { name, type, bindings,
overlayControls }`).

Tests without a world: `TestGameContext(input: TestInput(actions: [('jump',
ActionKind.button), ('move', ActionKind.axis2d)]))` — `press`/`release`/
`setAxis`, `endTick()` consumes the edges, `reset()` releases everything;
an undeclared action throws, so a test cannot pass on a typo.

## Recordings (`*.input.json`)

A host frame's dt and raw events, in order; the format every replay reads:

```json
{
  "fynkel": "input-recording",
  "formatVersion": 1,
  "frames": [
    { "dt": 0.016666668, "events": [ { "device": "keyboard", "control": "W", "phase": "down" } ] },
    { "dt": 0.016666668 },
    { "dt": 0.05, "events": [ { "device": "gamepad", "control": "leftStick", "phase": "axis", "x": -1, "y": 0 } ] },
    { "dt": 0.016666668, "events": [ { "phase": "reset" } ] }
  ]
}
```

`x`/`y` default to 0 (`down` reads as `x: 1`); a `reset` needs no device
or control. Replaying is `World.frame(dt, input: events)` per frame, so
the tick plan — and every edge rule above — is exactly what the recording's
frames imply, deterministic run to run.

- **Headless** — `fynkel test --headless --input test/walk.input.json`
  replays the file against the project's actions (the recording's end
  ends the run, as does `--steps`) and reports per action the
  ticks that saw `pressed`/`released`, the ticks it was held and its final
  state (`--json` → `input: { recording, frames, events, actions: [{ name,
  kind, pressed, released, held_ticks, held, x, y }], binding_errors }`);
  a binding the table cannot compile or an unreadable recording fails the
  run. Behaviours do not run here (Dart is not in the Rust host), so this
  proves the stream and the table; the Dart side proves the gameplay.
- **Dart** — `loadInputRecording(path)` (`package:fynkel/testing.dart`),
  `InputRecording.parse/encode`, `recording.replay(world, fixedUpdate:,
  update:)` → one `FramePlan` per frame. The starter's `test/game_test.dart`
  replays `test/walk.input.json` through `game.Mover` and asserts where the
  cube ended up and that a Space press across a 50 ms frame jumped once.
- **Recording** — `InputRecorder` receives every `(dt, events)` a
  `FynkelInputCapture(recorder: …)` hands to the loop; `recorder.recording.
  encode()` is the file.

## The Flutter host (`package:fynkel_flutter`)

- **`FynkelInputCapture`** collects one frame's raw events; `FynkelGameView(
  input: capture)` hands `capture.take(dt)` to `World.frame` every frame.
  Keys come from the viewport's `Focus` (physical keys by grammar name;
  repeats are not edges; the game view's `onKeyEvent` runs first, so a HUD
  can take Q for itself), mouse buttons from the button mask, movement as
  `mouse:delta` in physical pixels (y up), the wheel as `mouse:wheel`;
  touch pointers are the overlay's.
- **Focus and pause rules.** Flutter consumes its own interactions first —
  a focused text field keeps its keys; the viewport must have focus to
  receive them (`autofocus` by default, `FynkelGameViewState.hasFocus`).
  Losing focus, `setPaused(true)`, the app going hidden: the capture
  `reset()`s (everything up with edges on the next tick) and the pointer
  is released. While paused no frame runs, so the loop cannot read a
  `pause` action to resume — resume from a key (`onKeyEvent`) or a button.
- **Pointer capture** — `FynkelGameViewState.capturePointer(true)` hides
  the cursor and decouples it from movement (macOS: `NSCursor.hide`,
  `CGAssociateMouseAndMouseCursorPosition`, deltas from an `NSEvent` monitor
  scaled to physical pixels); Flutter's own move deltas are ignored while
  captured. A focus loss or pause releases it. `false` is returned where
  there is no capture (iOS, web).
- **Gamepads** — the Swift plugin (`FynkelInputPlugin`, in `fynkel_flutter`'s
  Darwin package) watches `GCController` on macOS and iOS alike: every
  connected extended gamepad's `valueChangedHandler` diffs its buttons
  and axes against the last values and sends the changes over the method
  channel `fynkel_flutter/input` (`events`: rows of `[device, control,
  phase, x, y]`), a disconnect sends every held button up; `gamepads`
  returns the count. Blueprint item 7 named `gilrs` for macOS; one
  framework for both Apple targets, with the controllers the OS already
  pairs, is the smaller dependency (no new crate on the runtime path).
- **`TouchOverlay`** — `touch:overlay/<name>` on screen: sticks bottom-left
  (`OverlayStick`: a ring with a knob, the vector clamped to the unit
  circle, y up, `(0, 0)` on release), buttons bottom-right
  (`OverlayButton`); `TouchOverlay.fromActions(report.inputActions, input:
  capture)` builds one control per overlay binding — an `axis2d` action's
  a stick, a `button`'s a button. Shown on iOS and Android unless
  `visible:` says otherwise; put it in the game view's `overlay` under the
  HUD. The starter and the showcase do.

Tests: `packages/fynkel_flutter/test/input_test.dart` — keys, the mouse
mask and deltas, the recorder, channel batches, a game view with the
overlay driving a real world (key down/up taken by the frame, the button's
edges, the stick's vectors, pointer capture through the mocked channel, a
pause resetting), the overlay hidden on macOS and built from bound actions.
The starter app on the iOS simulator: the stick walks the cube and the
button jumps it (M3-07 verification note in `status.md`).
