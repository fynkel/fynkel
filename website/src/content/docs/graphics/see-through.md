---
title: "Glass, water and glows"
description: "See-through materials: transparent and additive."
---

![Water and panes of coloured glass](/shots/see-through.webp)

A material's `opacity` (its colour's alpha) is how much of it there is.
Made with one below 1, the material is see-through:

```dart
/// Tinted glass, and a pool's water: a third and a half of them there.
final glass = Material(color: Color.rgb(1, 0.7, 0.3), opacity: 0.35, roughness: 0.1, doubleSided: true);
final water = Material(color: Color.rgb(0.12, 0.44, 0.55), opacity: 0.55, roughness: 0.05);

/// A spark: black, so all it shows is its glow, added to what's behind.
final spark = Material(color: Color.black, emissive: Color.rgb(1, 0.4, 0.1), blending: Blending.additive);

/// A ghost fading out over a second.
void fadeOut(GameObject ghost) {
  final material = ghost.get<MeshRenderer>()!.material!..blending = Blending.transparent;
  ghost.tween(1.0, 0.0, seconds: 1, onUpdate: (v) => material.opacity = v);
}
```

- **`Blending.transparent`** lays the surface over what's behind it: at an
  opacity of 0.35, about a third of it and two thirds of what's behind.
  Everything it shows fades with it, its highlights and glow too (glTF's
  BLEND, Unity's Fade). A model's BLEND materials load as this.
- **`Blending.additive`** adds the surface's light, times its opacity, to
  what's behind: glows, flames, sparks and beams. Black adds nothing, so give
  it an `emissive`. It brightens what's behind, so it shows best against the
  dark.
- **`blending`** is picked for you only by `Material(opacity:)` below 1.
  Setting `opacity` later changes the alpha alone: a material made opaque
  stays opaque (`blending` changes it), and its alpha is ignored.
- **They're drawn after everything solid**, the furthest object first (by its
  middle), each over what's behind it, and after the sky. Within one object
  the order is its triangles': a glass jar can draw its far side over its near
  one. If that shows, split the object in two. Two see-through objects that
  cross each other can't both be in front, so one of them is wrong where they
  cross.
- **They hide nothing.** What's drawn later shows through them, so they don't
  write to the depth buffer; a solid object behind one is still drawn, and a
  solid one in front of it hides it.
- **They take light and fog** as solid surfaces do, and **cast no shadow**.
- With an `alphaCutoff` too, what's below the cutoff isn't there, and the rest
  is see-through.
- Many see-through objects on top of each other cost more than solid ones:
  each pixel is drawn once per layer. The lab's 'Look: glass, water and glows'
  scene adds 400 panes to measure it.
