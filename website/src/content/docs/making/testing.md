---
title: "Testing a game"
description: "Run the game frame by frame in a test, with made-up input."
---

A game runs in a test with no window and no app around it, frame by
frame, with made-up input. Run the tests with **`flutter test`**, not
`dart test`: package:fynkel itself is plain Dart, but a game is a Flutter
package (it uses `ValueNotifier` and the rest of `foundation.dart`, and
its tests `flutter_test`), and only Flutter's tool resolves those.
`flutter test` builds the engine through the same hook as `flutter run`,
for the Mac it runs on.

```dart
// test/walker_test.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:my_game/game.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('D walks right', () {
    final world = World();
    addTearDown(world.dispose);
    final walker = world.add(Walker());
    world.input.simulate.keyDown(KeyCode.d);
    world.run(1); // a second at 60 fps
    expect(walker.position.x, closeTo(4, 0.1));
  });
}
```

package:fynkel/testing.dart gives a world `run(seconds)`, `tap(x, y)` and
`viewPoint(x, y)`. `world.input.simulate` takes `fingerDown`, `fingerMove`,
`fingerUp`, `resize` and the rest, so drags and swipes can be tested too; it's
what the view itself calls. A plain test has no view, so give the input a size
first, `world.input.simulate.resize(800, 600)`, and then `world.tap(400, 300)`
lands where you say, on the next frame.

To put one on an object, ask the camera where it is on screen:
`world.camera!.worldToScreenPoint(target.position, width: 800, height: 600)`.
`HeadlessView(world, width:, height:)` (package:fynkel) renders frames without
a window, for a picture of the game: `view.png()` draws a frame of the world
as it is now and gives it as a PNG, and
`File('shot.png').writeAsBytesSync(view.png())` saves it to look at, which
finds an unlit side or a camera too far fast.

To check pixels in a test, `view.read(view.frame()).rgba` draws a frame and
gives its pixels: RGBA, a byte each, rows from the top. It draws from the
world's main camera unless `view.camera` names another (each half of a split
screen). Dispose of the view when done (`addTearDown(view.dispose)`):

```dart
/// The red, green and blue at (x, y) in a frame drawn now, 0 to 255.
List<int> pixel(HeadlessView view, int x, int y) {
  final i = (y * view.width + x) * 4;
  return view.read(view.frame()).rgba.sublist(i, i + 3);
}
```

## Checking the whole picture

**To check the whole picture**, save it once and compare each run
against it: `view.expectFrame('test/frames/start.png')` (package:fynkel's
`testing.dart`) draws a frame and fails if it differs from the saved
picture by more than one GPU differs from another. The first time, or
after a change to the look that's meant, run `FYNKEL_BLESS=1 flutter test`
to save it, and look at it before keeping it. A failure leaves the picture
and the difference in `build/fynkel_failures/`.

`examples/arena/test` tests a whole game this way.

## A game that loads assets

needs to be told where they are: a test has no
app bundle. `World(source: ...)` takes a function from an asset's name to its
bytes, the files in the game's `assets/` folder here, and a game whose
`onLoad` is `async` is ready after `await world.add(game).loaded`, with no
frames run.

One whose `onLoad` awaits a routine (a fade, a `wait`, a tween) goes on only
in a frame, so there that would wait for ever: `await world.untilLoaded(game)`
(package:fynkel/testing.dart) runs a frame each 5 ms until it's loaded, as the
app would, and throws what the load threw:

```dart
// test/level_test.dart
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';

class Level extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    add(GameObject(name: 'floor', components: [MeshRenderer(Mesh.cube(), Material())]));
  }
}

void main() {
  test('the level loads', () async {
    final world = World(source: (name) async => File('assets/$name').readAsBytesSync());
    addTearDown(world.dispose);
    final level = world.add(Level());
    await level.loaded;
    expect(level.find('floor'), isNotNull);
    world.run(1);
  });
}
```

## Errors

**An error in the game fails the test** by itself, as it would be an
uncaught error: nothing listens to `world.errors`. To test that something
throws, listen first, and let the stream deliver before you look:
`world.errors` is a stream, so what the frames put in it arrives after
them, not during them. Without the `await`, the list is still empty and
the test passes whatever happened:

```dart
// test/errors_test.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:flutter_test/flutter_test.dart';

class Fragile extends GameObject {
  @override
  void onUpdate() {
    super.onUpdate();
    throw StateError('broke');
  }
}

void main() {
  test('a fragile object reports its error every frame', () async {
    final world = World();
    addTearDown(world.dispose);
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    world.add(Fragile());
    world.run(3 / 60);
    await pumpEventQueue(); // let the stream deliver what the frames sent
    expect(errors, hasLength(3));
  });
}
```

## Comparing positions

**Compare positions with a tolerance.** `Vec3` and `Quat` have
`closeTo(other)`, so `expect(a.position.closeTo(Vec3(1, 0, 0)), isTrue)`.
An object with a rigidbody needs it most: physics keeps poses in 32-bit
floats, so after a step its position reads `0.20000000298`, not `0.2`.

## Is it fair?

**Is it fair? A test can say.** Play whole rounds in a test with a
scripted player that only does the obvious thing, and one that does
nothing: the first should win, the second lose. Move the player as the
game would (`walk(...)`, `world.update(1 / 60)`), and loop until the round
is over. Sheepdog's (`examples/sheepdog/test/game_test.dart`, "is fun")
found the wolves herding the flock home better than no dog at all, which
reading the code hadn't.

## The view's shape

is `world.input.width` and `height`, in logical
pixels: a camera that frames a board to fit reads them. A `FynkelView`
gives them, and so does a `HeadlessView` (in its pixels) when nothing
else had; in a test with no view, `world.input.simulate.resize(390,
844)` sets them. With two views of one world, they're the size of the
one that takes input.

## Does it leak?

**Does a level change grow memory?** A test can tell, with a `HeadlessView`:
switch levels many times, and after each round draw a frame
(`view.read(view.frame())`) and read what the engine holds for drawing,
`view.stats.textureBytes` and `meshBytes`, with the number of objects,
`world.all<GameObject>(includeInactive: true).length`. Each the same every
time round (from the second: the first may load what stays) means nothing
piles up: a leak grows them a little each round.

The process's resident size (`ProcessInfo.currentRss`) isn't one to test: it
rises for a while as Dart's heap settles, and only a long run on a device
([Measuring](/making/measuring/)) tells settling from a leak.

`world.run(seconds)` moves on in whole frames of 1/60 s, so a routine's
`wait(0.5)` ends on the first frame at or past half a second: test what
it did after a little more than its waits, not exactly at them.
