---
title: "Keys, mouse and touch"
description: "Poll input, listen to gestures, or let the touched object handle it."
---

`input` is on every object and behaviour. Poll it, as Unity's `Input`:

```dart
class Walker extends GameObject {
  @override
  void onUpdate() {
    super.onUpdate();
    final move = input.axis2('move'); // W, A, S, D and the arrows
    translate(Vec3(move.x, 0, -move.y) * (4 * time.delta));
    if (input.action('jump').wasPressed) jump(); // space
    if (input.isPressed(MouseButton.right)) rotate(Vec3.up, time.delta);
  }

  void jump() => moveBy(Vec3(0, 1, 0), seconds: 0.25, yoyo: true, loops: 2);
}
```

## Gestures and events

Or listen to events, Lean Touch's way:

```dart
class Gestures extends GameObject {
  @override
  void onStart() {
    super.onStart();
    on(input.onTap, (finger) {
      if (finger.over case final hit?) debug.sphere(hit.point, 0.2, seconds: 1);
    });
    on(input.onSwipe, (swipe) => print('swiped ${swipe.direction} at ${swipe.speed} px/s'));
    on(input.onPinch, (pinch) => localScale = localScale * pinch.scale);
    on(input.onKeyDown, (press) => print('${press.key} ${press.shift ? 'with shift' : ''}'));
  }
}
```

## Taps on objects

Or let the object under the finger handle it, as Unity's pointer
interfaces do:

```dart
class Button3D extends GameObject {
  Button3D() : super(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFF40A060)))]);

  @override
  bool onClick(Finger finger) {
    punchScale(Vec3.one * 0.3, seconds: 0.2);
    return true; // taken: the parent doesn't get it
  }

  @override
  void onPointerEnter(Finger finger) => get<MeshRenderer>()!.material!.color = Color.white;

  @override
  void onPointerExit(Finger finger) => get<MeshRenderer>()!.material!.color = Color(0xFF40A060);
}
```

## In detail

A Flutter widget over the game (a HUD button) takes its own taps; they don't
reach the game.

### A Finger

A `Finger` is one touch, or the mouse: `position`, `startPosition`,
`delta`, `totalDelta`, `velocity`, `age`, `tapCount`, `isPressed`,
`isDragging`, and `over` (what's under it in the world: `gameObject`,
`point`, `normal`). `input.fingers` lists them. `position` is a `ViewPoint`,
for `screenPointToRay`; `position.toVec2()` for a `Vec2`.

**Where the mouse is**, pressed or not, is `input.mousePosition` (or else
the first finger down; null with neither): aiming at it is
`camera.screenPointToPlane(input.mousePosition!, Plane(Vec3.up, Vec3(0, y,
0)))`. In a test, `input.simulate.hover(id, x, y)` moves it.

### Events

`onFingerDown`, `onFingerUp`, `onFingerMove`, `onHover`, `onTap`
(a double tap is two, the second with `tapCount` 2), `onLongPress`,
`onDragStart`, `onDrag`, `onDragEnd`, `onSwipe`, `onPinch`, `onTwist`,
`onScroll`, `onKeyDown`, `onKeyUp`. `onPinch` and `onTwist` are given a
`Pinch`: its `scale` is how much further apart the fingers are than last
frame (zoom by it), and its `twist` how far they turned, in radians,
**clockwise** on the view, where the world's angles turn the other way: with
the camera looking along -Z, `rotate(Vec3(0, 0, 1), -pinch.twist)` turns an
object with the fingers.

A pinch's fingers drag too: `onDrag` comes for each of them, so a camera
that pans on a drag and zooms on a pinch skips the drags while
`input.fingers.length > 1`.

### On objects

`onPointerDown`, `onPointerUp`, `onPointerEnter`,
`onPointerExit`, `onClick`, `onLongPress`, `onDragStart`, `onDrag`,
`onDragEnd`, on behaviours too. They go to the object under the finger, then
up to its parents until one returns true. What takes a drag's start gets the
rest of that drag. With physics, a collider takes taps too, even with
nothing drawn (an invisible button); triggers don't
(`physics.pointerHitsColliders = false` turns it off).

### Keys

Keys are `KeyCode`s, by where they are on the keyboard, so W, A, S, D
stay put on any layout. `input.bindAxis2` and `input.bindAction` add your
own names.

### Rebinding

Rebinding is binding again: `input.bindAction('dash', [key])` changes
the same `InputAction`, so code that kept it sees the new keys. A key held
at that moment that's no longer bound lets go (`wasReleased` comes next
frame); a `press()` goes on holding.

On a controls page, `keyCodeOf(event)` (from `package:fynkel_flutter`) turns
the key pressed in a "press a key" dialog into the game's, and `key.label`
names it for the player (`Space`, `W`, `↑`). [A controls
page](/flutter/rebinding/) has that dialog and a row of a controls page.

## In onFixedUpdate

**In `onFixedUpdate`**, `wasPressed` and `wasReleased` (an action's too)
mean since the last fixed step, so each press is seen once there: a frame
that runs two fixed steps doesn't jump twice, and one that runs none
passes the press to the next. Unity's `GetKeyDown` in `FixedUpdate`
misses or repeats presses; here it doesn't.
