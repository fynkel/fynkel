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
follows its own Head (an object reference, ADR-0035). Then it:

1. instantiates the prefab until the world holds about 50 000 entities
   (8 333 instances on a 100-wide grid): the time per call, early and
   late, and what the live heap gained;
2. ticks once (the instances' first tick) and 100 times idle, printing
   the phases of each;
3. ticks 100 times with 1% of the roots moved each tick;
4. reparents 1 000 roots under others, keeping their world pose, in one
   batch;
5. makes one object inactive, then spawns one entity under a parent per
   tick, 100 times;
6. destroys every instance in one batch and ticks idle again;
7. runs the whole instantiate–tick–destroy cycle three more times, with
   the live heap after each: a leak grows it every cycle.

Each tick drains the events, as a host does. Memory is the live Rust heap
(`FynkelWorldStats::native_bytes`, the engine's counting allocator), not
the process's resident size, which also holds freed memory the allocator
keeps for reuse.

## Results — Apple M3 Max, release build, 50 000 entities

| | baseline (2026-09-28) | after 3.1 | after 3.3/3.4 | after 3.5 |
|---|---:|---:|---:|---:|
| instantiate, all 8 333 | 55.4 s | 113 ms | 117 ms | 109 ms |
| instantiate, each (first 1 000) | 5.5 ms | 18 µs | 19 µs | 19 µs |
| instantiate, each (last 1 000) | 7.7 ms | 13 µs | 13 µs | 12 µs |
| live heap after instantiating | — | +20 MiB (420 B per entity) | the same | the same |
| first tick after instantiating | 755 ms | 762 ms (Follow 748 ms, 3.5) | 759 ms (Follow 750 ms) | 7.9 ms (Transforms 6.5, Follow 1.3 ms) |
| tick, idle | 6.7 ms | 6.6 ms | 1.7 ms | 1.3 ms |
| — its phases | Transforms 2.9, Physics 2.4, Follow 1.4 ms | the same | Transforms 0.2, Physics 0.1, Follow 1.4 ms | Transforms 0.15, Physics 0.09, Follow 1.0 ms |
| tick, 83 roots moved | 9.0 ms | 8.9 ms | 2.4 ms | 1.3 ms |
| tick with 1 000 keep-world reparents | 2 329 ms | 2 241 ms | 3.1 ms | 2.3 ms |
| tick with a parented spawn, one object inactive | 8.0 ms | 7.8 ms | 1.7 ms | 1.3 ms |
| tick destroying every instance | 11.7 ms | 11.9 ms | 11.0 ms | 7.2 ms |
| a further instantiate–tick–destroy cycle | — | 878 ms, live heap +0 KiB | 870 ms, +0 KiB | 114 ms, +0 KiB |

The baseline measured the process's resident size, not the live heap:
+24 MiB after instantiating and +57 MiB with everything destroyed. The
live heap after the first cycle stays at its high-water mark (+44 MiB:
the ECS keeps the capacity it grew to) and later cycles reuse it.

What it shows:

- **Instantiate (3.1, fixed).** Each call re-read and re-parsed the
  cooked prefab, registered its assets again, took one more residency
  token (released only by a replace load), and propagated transforms and
  active flags over the whole world, so it got slower as the world grew.
  Now the world keeps each prefab in memory after its first instantiate
  (assets registered once, one token per prefab) and settles only the
  new subtree: an in-memory copy, as Unity's `Instantiate`.
- **The first tick after instantiating (3.5, fixed).** Follow resolved
  every new follower's target by name over the whole world: 748 ms for
  8 333, and every instance followed the first "Head". Now the target is
  an object reference resolved once when the prefab is instantiated, so
  each Marker follows its own Head and the first tick is the transforms
  of the new objects (6.5 ms) plus one Follow pass (1.3 ms).
- **An idle tick (3.3, fixed; 3.12).** Nothing moved, yet 6.6 ms went,
  40% of a 60 Hz frame. `propagate` walked every node three or four
  times per tick, once of them inside the physics phase, which is why
  physics looked slow (3.12: its own step took 50 µs); and every tick
  decomposed every world matrix for interpolation. Now `propagate`
  scans the dirty flags and walks only dirty subtrees, and only an
  object that moved is decomposed. What is left is Follow: 8 333
  followers placed every tick, about 120 ns each (reading the target,
  decomposing their own matrix, writing the pose back). Skipping a
  follower whose target and own pose did not change would remove most
  of it; not done yet.
- **Reparenting (3.3, fixed).** A keep-world `SetParent` ran a full
  transform pass per command so the poses it kept were current; now it
  reads them from the chain of local transforms: 1 000 of them take 3 ms.
- **A parented spawn with one inactive object (3.4, fixed)** re-walked
  the whole hierarchy for the active flags; now each change refreshes
  its own subtree.
