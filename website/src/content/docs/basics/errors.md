---
title: "Errors"
description: "What happens when the game throws, and where to hear of it."
---

An exception in any of these calls (or a
routine, a handler, a tween) goes to `world.errors` with its object, and the
rest of the frame goes on. In a debug build the view also shows it over the
game, with the object and the line in your code it came from, and prints it to
the console, so a broken game is never just a black screen; a widget test
fails on it.

In a release build, or with `FynkelView(showErrors: false)`, it's the game's
to handle: listen to `world.errors`, or it's an uncaught error.

One call is different: **an error in `onLoad`** (an object's, or one of
its behaviours') **destroys the object**, since it never finished
loading; it goes to `world.errors` too, and its `loaded` future fails
with it. A level whose file is missing doesn't appear half-made.

**Your own mistakes throw Dart's errors**, at the line that made them:
an `ArgumentError` for a bad value (a NaN position, a rotation that isn't
`.normalized`, a negative radius), a `StateError` for a call at the wrong
time or on something gone (a destroyed object, a mesh still in use). A
file that can't be read throws a `FormatException`. `FynkelException` is
only for the engine's own failures: no GPU, a lost device, a panic.
