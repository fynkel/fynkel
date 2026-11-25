---
title: "Skinned characters"
description: "Characters bent by a skeleton, and things held in a hand."
---

![A knight and an archer, skinned glTF characters, standing in grass](/shots/skinned.webp)

A skinned character, one whose mesh bends with a skeleton (Mixamo's, KayKit's,
most from Blender), needs nothing more: its joints are its copy's objects, the
clips move them, and the engine bends the mesh to them on the GPU, its shadow
too.

Every copy bends with its own joints. So holding something is the same as for
a model whose parts move whole (`knight.glb`, `rogue.glb` and `sword.glb` are
in examples/lab/assets/kaykit, CC0; the knight and rogue have walk, run and
jump clips, `Jump_Start`, `Jump_Idle`, `Jump_Land`):

```dart
class Rogue extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final rogue = add(await assets.instantiate('rogue.glb'));
    final sword = await assets.instantiate('sword.glb');
    rogue.find('handslot.r')!.add(sword); // KayKit's hands have a slot
    rogue.animator!.play('1H_Melee_Attack_Chop');
  }
}
```

- A file often carries every weapon its character can hold, all shown: turn
  off the ones you don't want (`find('2H_Sword')!.active = false`).
- A skin may have up to 256 joints, and a vertex follows up to four. A skin
  with more isn't bent (the mesh shows as it was bound), and a debug build
  prints why as the model loads, as it does whatever else a model skips
  (`model.warnings`).
- The lab's "Animation: skinned" shows two KayKit characters.
