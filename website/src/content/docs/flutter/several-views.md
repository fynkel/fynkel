---
title: "Minimap and split screen"
description: "Several views of one world."
---

**Several views of one world**: a minimap, split screen. Each is a
`FynkelView` of the same game, with a `camera:` of its own; without one,
a view draws from the main camera (`world.camera`). However many views
show the world, it updates once a frame. Its meshes and textures are on
the GPU once; each view only adds its own frames. A view whose camera is
gone (destroyed, inactive) draws black rather than the main picture.

```dart
// lib/minimap.dart
import 'package:flutter/material.dart' hide Color, Material, Transform;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// A player and a camera high above it, looking straight down.
class Field extends GameObject {
  late final GameObject player;
  final map = Camera(orthographic: true, orthographicSize: 8);

  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(transform: Transform(position: Vec3(0, 4, 9)), components: [Camera()]));
    player = add(GameObject(components: [MeshRenderer(Mesh.cube(), Material())]));
    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(0, -1, 0), up: Vec3(0, 0, -1))),
        components: [map, Follow(player, offset: Vec3(0, 20, 0))],
      ),
    );
  }
}

class FieldPage extends StatelessWidget {
  const FieldPage({super.key});

  @override
  Widget build(BuildContext context) => FynkelGame(
    Field.new,
    builder: (context, game) => Stack(
      fit: StackFit.expand,
      children: [
        FynkelView(root: game),
        Align(
          alignment: Alignment.topRight,
          child: SizedBox(
            width: 160,
            height: 160,
            child: FynkelView(root: game, camera: game.map, input: false, worldWidgets: false),
          ),
        ),
      ],
    ),
  );
}
```

The main camera is the first one added (the lowest id), so add the game's own
camera before the minimap's. `input: false` keeps taps on the minimap from
reaching the game or the view under it. For split screen, put two views in a
`Row`, each with the camera that follows its player.

Give `input: false` to all but one: that one takes the keys for both players,
and aims what's under a finger from its own camera (`world.input.camera`).
`worldWidgets` stays on there, so each half shows the name tags placed by its
own camera.

The lab's "Minimap" and "Split screen" scenes show both, with each view's
frames per second.
