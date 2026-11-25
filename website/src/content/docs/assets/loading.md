---
title: "Loading assets"
description: "Models, textures and files of your own, and levels from Blender."
---

Models (`.glb`), textures (`.png`, `.jpg`) and sky images (`.hdr`, [Sky and fog](/graphics/sky-and-fog/))
are loaded by name, from the app's `assets/` folder (list it under
`flutter: assets:` in `pubspec.yaml`):

```dart
class Tree extends GameObject {
  Tree(Vec3 at) : super(transform: Transform(position: at));

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.instantiate('props/tree_oak.glb'));
    model.find('leaves')?.get<MeshRenderer>()?.color = Color.rgb(0.8, 0.5, 0.2); // this tree's alone
    final bark = await assets.texture('bark.png');
    add(
      GameObject(
        transform: Transform(scale: Vec3(0.3, 2, 0.3)),
        components: [MeshRenderer(Mesh.cube(), Material(baseColorTexture: bark))],
      ),
    );
  }
}
```

## In detail

- `assets.instantiate(name)` gives a new copy of the model, a tree of objects
  sharing its meshes and textures with every other copy. Find its parts by
  name once it's added (`add(copy)` first): `find` throws on an object not in
  a world yet.
- `copy.bounds` is the box in the world round what it draws (null when nothing
  in it draws a mesh, so `copy.bounds!` for a model): its `size` is the
  model's size, to scale it to fit (`Vec3.all(1 / size.y)` makes it a metre
  high), and its `center` less `copy.position` is how far its middle is from
  its pivot, which a model needn't have in its middle. `mesh.bounds` is a
  mesh's, in its own space.
- **A texture made in code**, rather than a file: `Texture2D.rgba(width,
  height, pixels)`, 8-bit RGBA, rows from the top. A soft dot for sparks, say,
  or a colour grade's table ([Screen effects](/shaders/effects/)):

```dart
// lib/textures.dart
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';

Texture2D softDot([int size = 32]) {
  final pixels = Uint8List(size * size * 4);
  for (var y = 0; y < size; y++) {
    for (var x = 0; x < size; x++) {
      final d = Vec2((x + 0.5) / size * 2 - 1, (y + 0.5) / size * 2 - 1).length;
      final a = (255 * (1 - d).clamp(0.0, 1.0)).round();
      pixels.setAll((y * size + x) * 4, [255, 255, 255, a]);
    }
  }
  return Texture2D.rgba(size, size, pixels);
}
```

- Each file is loaded once, off the UI thread, and shared. It's freed when the
  last object that asked for it is destroyed.
- `assets.preload([...])` loads ahead, so a level appears at once later.
  What's preloaded counts as asked for by the object whose `assets` it is, and
  stays until that object goes: preload the next level from the level that's
  on (as [Levels](/making/levels/) does), not from the root, or the root ends
  up holding every level it ever preloaded. It needn't be awaited; if its
  object goes first, it ends quietly.
- A file of the game's own, as a level's JSON, is read through the world's
  source, so a test reads it as the app does: `jsonDecode(utf8.decode(await
  world.source!('levels/1.json')))`. It's read each time, not kept.
- A package can add a kind of asset of its own (a sound package's `.wav`):
  it's loaded, preloaded, shared and freed in the same way.
- A level that's `async` in `onLoad` appears only when all of it is there:
  `await add(level).loaded` waits for that.

## Listing folders

**Flutter's asset folders aren't recursive.** `- assets/` lists the files
in `assets/`, not those in `assets/props/`: list each folder,

```yaml
flutter:
  assets:
    - assets/
    - assets/props/
```

as [examples/coins](https://github.com/fynkel/fynkel/blob/main/examples/coins) does. A file in a folder that isn't listed isn't in
the app, and loading it fails with its name.

## A level from Blender

Make the level in Blender and export it as a `.glb`
(File → Export → glTF 2.0, glTF Binary, +Y Up). Every Blender object comes in
as an object of the same name, placed as it was, empties too: name the places
the code needs (`start`, `gate`) and `find` them ([Levels](/making/levels/)
has a level that does).

The meshes, their materials and the animations come in; the file's lights and
cameras are skipped (`Model.warnings` says what was), so light the level from
code ([Lights](/graphics/lights/)). To walk on it, give it colliders:
`MeshCollider.addToParts(level)` ([Colliders](/physics/colliders/)) gives
every part its own shape.

A model's clips and how to play them are [Playing clips](/animation/clips/).
