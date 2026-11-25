---
title: "Lights"
description: "The sun, lamps and spotlights."
---

![Dozens of coloured lamps lighting a dark room](/shots/lamps.webp)

`Light.sun` shines along its object's forward, the same everywhere;
`Light.sun(direction: …)` turns the object to it. `Light.ambient` lights
everything alike. `Light.point` and `Light.spot` are lamps.

## Lamps

A point light shines from its object's position every way; a spotlight
shines from there along the object's forward, in a cone. Both reach
`range` metres and fade smoothly to nothing there:

```dart
/// A warm lamp on a post, reaching 8 m.
GameObject lampPost(Vec3 at) => GameObject(
  name: 'lamp post',
  transform: Transform(position: at + Vec3(0, 2.5, 0)),
  components: [Light.point(color: Color.rgb(1, 0.8, 0.55), intensity: 10, range: 8)],
);

/// A torch that shines where its object faces: turn the object to aim it.
GameObject handTorch() => GameObject(
  name: 'torch',
  components: [Light.spot(intensity: 40, range: 20, angle: 30.deg, softness: 0.3)],
);
```

- **Brightness** is the sun's, at 1 m: a lamp of `intensity` 1 lights a
  surface 1 m away as a sun of 1 does, and a quarter as much at 2 m (light
  falls off with the square of the distance). A room lamp 2–3 m from what it
  lights wants 5 to 20; a torch across a room, 50 and up.
- **`angle`** is the cone's width, edge to edge, up to 179°. **`softness`** is
  how much of it fades, from the rim in: 0 is a hard edge, 1 fades all the way
  to the middle.
- **Every sun lights everything**, up to four of them.
- **A lamp lights everything it reaches**, whatever it falls on: a big floor,
  terrain or a room in one piece is lit by every lamp over it, however many
  overlap. What costs is how many overlap: each pixel adds up every lamp that
  reaches it. Without `Quality.lampHighlights` (as at low) lamps light a
  surface's colour but give shiny surfaces no highlight (suns still do), which
  keeps a crowded room fast on phones.
- **A frame takes 64 lamps** in view, the ones nearest the camera. A world
  with more than 64 point and spot lights, or more than four suns, says so
  once.
