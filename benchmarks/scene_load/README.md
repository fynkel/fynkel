# Scene load: sync against async (ADR-0038 step 4)

How long the game stops when a level loads. The benchmark cooks a
project into a temporary cache, then loads one scene on a fresh world
five times each way:

- **sync**: `fynkel_world_load_scene`, the whole load in one call (one
  frame);
- **async**: `fynkel_world_load_scene_async`. The world steps every
  frame (60 Hz) while a background thread reads the scene and its
  assets, then `fynkel_world_load_activate` spawns it in one frame.

```sh
# a copy of the tracked files only (the cook may write sidecars)
git archive HEAD examples/reference_game | tar -x -C /tmp
caffeinate -du cargo run --release -p fynkel_native --example scene_load -- /tmp/examples/reference_game
```

## Reference game, Playground (2026-09-28, Apple M-series, release)

| | ms |
| --- | --- |
| sync: the load call (one frame) | 15.68 |
| async: background work (2 frames) | 39.99 |
| async: longest frame while loading (a step) | 0.03 |
| async: the activation (one frame) | 0.82 |

The longest stop goes from **15.7 ms to 0.8 ms**. The background work
takes longer in wall time because the loop checks once per 60 Hz frame.

Not measured here: the GPU. Without a background load, the renderer
uploads meshes and textures during the first frames after the load.
With one, it uploads them while the load runs, within
`warm_budget_us` (2 ms) per frame
(`the_renderer_warms_what_a_background_load_made_resident` in
`crates/fynkel_render/tests/project.rs`). Script creation (Dart
`onCreate`) happens in the activation frame, as in Unity.
