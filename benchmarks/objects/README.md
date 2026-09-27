# Objects at scale (foundation 3.9)

A headless benchmark of what a game does with many objects, the gate for
foundation rows 3.1–3.4 (`docs/foundation.md`). It drives the native
world the way a host does (`fynkel_world_instantiate`,
`fynkel_world_step`):

```sh
cargo run --release -p fynkel_native --example objects            # 50 000 entities
cargo run --release -p fynkel_native --example objects -- 20000   # fewer
```

It copies the fixture project to a temporary directory, adds a 6-entity
prefab and cooks it. The prefab is an Enemy root with a mesh, a Body with
a mesh holding a Head and an Arm, Legs, and a Marker whose `fynkel.Follow`
follows "Head" by name. Then it:

1. instantiates the prefab until the world holds about 50 000 entities
   (8 333 instances on a 100-wide grid): the time per call, early and
   late, and the memory the process gained;
2. ticks idle 100 times, and prints the phases of the last tick;
3. ticks 100 times with 1% of the roots moved each tick;
4. reparents 1 000 roots under others, keeping their world pose, in one
   batch;
5. makes one object inactive, then spawns one entity under a parent per
   tick, 100 times;
6. destroys every instance in one batch, ticks idle again, and prints the
   memory left.

## Baseline — 2026-09-28, Apple M3 Max, release build

Before any Step 3 fix (commit after `b7264e7`):

| | 50 000 entities |
|---|---:|
| instantiate, all 8 333 | 55.4 s |
| instantiate, each (first 1 000) | 5.5 ms |
| instantiate, each (last 1 000) | 7.7 ms |
| memory after instantiating | +24 MiB |
| first tick after instantiating | 755 ms |
| tick, idle | 6.7 ms |
| — its phases | Transforms 2.9 ms, Physics 2.4 ms, Follow 1.4 ms |
| tick, 83 roots moved | 9.0 ms |
| tick with 1 000 keep-world reparents | 2 329 ms |
| tick with a parented spawn, one object inactive | 8.0 ms |
| tick destroying every instance | 11.7 ms |
| memory at the end, everything destroyed | +57 MiB |

What it shows:

- **Instantiate (3.1).** Each call costs milliseconds and gets slower as
  the world grows: it re-reads and re-parses the cooked prefab and
  registers its assets again. Unity's `Instantiate` is an in-memory copy.
  Memory is not given back when everything is destroyed.
- **An idle tick (3.3).** Nothing moves, yet 6.7 ms goes, 40% of a 60 Hz
  frame. `propagate` walks every node. The physics phase costs 2.4 ms
  with no collider in the scene (a new row, 3.12). Follow resolves its
  target by name world-wide every tick (3.5).
- **Reparenting (3.3).** A keep-world `SetParent` runs a full transform
  pass per command: 1 000 of them take 2.3 s.
- **A parented spawn with one inactive object (3.4)** re-walks the whole
  hierarchy for the active flags.
