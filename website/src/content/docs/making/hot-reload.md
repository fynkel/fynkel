---
title: "Hot reload"
description: "What a reload changes, and starting the game over."
---

- **Hot reload** (`r`) changes methods at once, in objects and behaviours
  alike: the next frame runs the new `onUpdate`.
- It doesn't run `onLoad` or `onStart` again, nor change a field that's
  already set, and a routine that's running keeps its old code.
- **F5** (in a debug build) starts the game over with the code as it is now: a
  new root, a new world. That's how a changed `onLoad`, field or routine takes
  effect, in a second, without restarting the app.
- On a phone, which has no F5, `FynkelApp(Game.new, restartOnReload: true)`
  makes every hot reload start the game over; a `FynkelGame` ([A game inside
  an app](/flutter/in-an-app/)) takes `restartOnReload: true` too.
- **Hot restart** (`R`) starts the whole app again. What the engine holds for
  the old app is freed, except a load in progress at that moment
  (`assets.instantiate`, `preload`, a texture or sound): its native memory
  stays until the app stops. Only a debug build hot restarts, so a shipped
  game never meets this; on a phone, restarting in the middle of a big
  `preload` again and again adds up.
