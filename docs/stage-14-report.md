# Stage 14 report: make it solid

[The stage 14 plan](stage-14.md), and what the audit found and what came
of each row in [the findings](stage-14-findings.md). Every phase is
merged (#147 to #185) but two of phase 5's points, below.

## The result

Everything built in stages 1 to 13 was read again, by area, and tried
together by two newcomers writing games from the guide. What could take
a game down is fixed, and so is every bug the audit confirmed. The
systems now run together in one scene, tested at each level and in a
soak that draws. How fast the engine is gets measured, part by part on
each device, and CI holds it there. A game is easier to write and to
test.

| Phase | What | PRs |
|---|---|---|
| 1 | The audit: 8 readers, 2 fresh developers, 5 checkers; about 95 claims, about 80 held, most with a failing test | #147 |
| 2 | Every confirmed bug, worst first: the app going down (a mixer panic, a malformed model, a particle shader, an oversized view, sound after dispose, uncapped emits, long calls not marked blocking, the shader thread's errors), then physics, objects and cameras, meshes, particles, animation and sound | #148–#152 |
| 3 | All of it together: built-in systems through `WorldSystem` (DC5); the lab's "All of it together" at each level; the soak draws; Sheepdog's fences and path along splines; `audio.stopOneShots`; the view's platforms a host each (BR11); `abi.rs` split by area (BR10) | #153–#156 |
| 4 | Fast, and staying fast: benchmarks and the soak's memory held in CI; `world.profile`; cost curves; a budget per part of the frame on the Mac, the Note10+ and an iPad; a comparison with Unity on the same scene; each GPU pass timed; then the costs found, fixed (below) | #157–#175, #177 |
| 5 | Easy to write a game with: `Stick` shipped; a view's error hook; the guide's gaps; testing helpers (`tester.frames`/`run`/`game`, `expectImage`, `view.expectFrame`); every guide sample run, not only compiled; Pocket Hole and hello in CI; a minute's walk as a test; the fresh developers again | #179–#185 |

Alongside it: 3D text (#176), and a game's character that stopped for a
frame now and then, found by the user's game (#178, with its test, GT13).

## Measured

- **What the frame costs**, Mac (fixed in phase 4): plain objects 1.3 →
  0.002 ms at 32,000; 50 raycasts into 3,000 objects 11.3 → 1.5 ms; a
  turning sky 7.6 → 0.5 ms; a road swept again 0.100 → 0.058 ms and no
  allocations; 10,000 see-through particles sorted 0.60 → 0.10 ms; hidden
  animators 34 → 4 µs for 200; a material value set 0.62 → 0.12 µs; a
  particle setting with 1,000 systems 3.9 → 0.9 µs.
- **The curves**: every cost grows in step with its count or slower
  (objects, lamps, particles, sounds, bodies), once DC4 was fixed.
- **The budgets**: the Note10+ is within every part's budget at low in
  the busiest scene there is (59.7 fps). The Mac takes a tenth of them.
- **Against Unity**, Sheepdog's field built the same in both: even on
  the Note10+ at low. Elsewhere Unity's GPU did the frame in about half
  the time. Three fixes followed:
  - soft particles read a depth of their own;
  - lit particles are lit once each;
  - the scene image is 10 bits a colour.

  The first two took the iPad (A10) at medium from 15.3 to 30.0 fps
  (Unity 39.6), and at high from 10.9 to 18.7 (Unity 28.3). With the
  third, the Note10+ at medium went from 32.2 at the start to 45.3
  (Unity 52.1).
- **The newcomers**: in phase 1 both games built and passed their tests
  first time; their logs listed about twenty things in the guide or API
  that held them up. In the second round both games again built and
  passed first time. Their developers hit only what's listed in the
  findings' "Fresh developers again", now fixed in the guide.
- **The guide**: its 74 code blocks compile, and 43 of its objects and
  functions run a second each without an error (48 tests, from 5).

## Not measured

- **The iPhone**: its budgets, Sheepdog at 120 and high (80 before),
  and the runs owed by stages 12 and 13. It was only reachable
  wirelessly, and those runs sent no log lines.
- **Stage 13's mesh change on the Note10+**: 0.82 ms sent and about 0.45
  set, against 1 ms together. Still **missed**.

## Left open

- **Two of phase 5's points** were never made rows:
  - **one pass over the public API** (names, defaults, error messages);
  - **extending it**: a game's own behaviours (§4) and shaders (§14)
    are taught, but not writing a package on `WorldSystem` and
    `extend.dart`.
- **RN8**, the renderer's rewrite (`renderer.rs` as passes of their
  own), moved to a later stage with RN7.
- **The rest of RN12**, bloom's chain and the finish pass (Unity is
  still ahead at medium and high); **RN5 ½**; **WO10 ½** (one moved
  object still recomputes every world matrix).
- **Small rows**: AU7 (a streamed voice copies its file each start),
  RN3 (Android's pipeline cache), BR5 (an iPhone check), BR6, BR7, PH6,
  PH7, RN9 ½, DF13, BR8 ½ (the ABI's written rule).
- **GD9 ½**: a game's tests can't count what a frame allocates. Under
  `flutter test` the counter has no VM service and can't start its
  isolate, so it needs a `dart` process of its own.
- **GT12 ½**: CI is one job of about 56 minutes on one runner.

## Next

The order after this stage is the user's to set. These are open:
- the two phase 5 points;
- the open rows above;
- the iPhone runs;
- animation 2 (stage 15 in the old order);
- cutting meshes (stage 13's phase 4).
