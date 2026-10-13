# Stage 6, step 6: the fixes

Everything steps 1 to 5 found, in the order it gets fixed: the worst
first, one PR each, with the examples, the lab and the guide changed in
the same PR, and a line in the guide's notes for each breaking change.

The sources:

- **F1–F35**: the findings ([stage-6-findings.md](stage-6-findings.md)).
- **A1–A8, B, C**: the API review ([stage-6-api.md](stage-6-api.md)).
- **R1–R21**: the architecture review's items
  ([stage-6-architecture.md](stage-6-architecture.md)).
- **S**: the scale checks ([stage-6-scale.md](stage-6-scale.md)).

A PR's line gets its number when it opens, and **done** when it's merged.

| # | PR | Fixes |
| --- | --- | --- |
| 1 | #69, **done**: **Holes in Dart**: the view's frames never stop, a dispose inside an update waits for it, a kept `Physics` can't write into freed memory | R1 (and so F25), R2, R3 |
| 2 | #70, **done**: **Holes in the engine**: GPU errors and a lost device, the physics thread's aliasing, panics in the freeing calls | R4, R5, R19 |
| 3 | #71, **done**: **The Flutter blocker and clean endings**: `FynkelApp` on `MaterialApp`; `World.dispose` destroys the roots first; a routine's microtasks always go to the world's queue | F1, F7, F29, B |
| 4 | #72, **done**: **The hierarchy in Dart**: parent, children and shown kept in Dart, changed only through `World` | R6, R16 |
| 5 | #73, **done**: **Moving as Unity**: `position` is the world's, `translate`/`rotate` take a `Space`, `parent =` keeps the world position | A2, A5 |
| 6 | #74, **done**: **The other API decisions**: radians with `.deg`, `isPressed`, the routine words, the camera's helpers on `Camera`, `gameObject` and `RaycastHit`, `world.all` skipping hidden objects | A1, A3, A4, A6, A7, A8 |
| 7 | #75, **done**: **Only what moved**: the core's moved log, physics follows only it; per-frame lists that keep their storage | R7, F9, R9 |
| 8 | #76, **done**: **Drawing at scale**: lighter built-in sphere, plus capsule, cylinder and quad meshes; culling; sort keys computed once; a material looked up once per batch; a warning when lights are dropped | S, F13, R8, R12's warning |
| 9 | #77, **done**: **Pooling**: a pool with versioned handles, and the guide's section | F8, F30 |
| 10 | #78, **done**: **Input and pause**: `world.paused`; Flutter controls feed axes and actions; keys reach Flutter's shortcuts; `Signal.stream`, `on` takes a `Listenable` | F2, F3, F4, F5, F32, B |
| 11 | #79, **done**: **Look, physics and motion**: a per-object tint and `Material.copy`; gravity wakes bodies, `wakeUp` before the world does nothing, errors name the real reason; moving platforms; `Follow(space:)`; B's renames and small fixes; `FynkelGame.of`, `FynkelView` split | F12, F14, F15, F18, F20, F22, B |
| 12 | #80 and #81, **done**: 12b (the C base, the duplicated loops, the release profile) and 12a (`extend.dart` version 2): **Seams**: `extend.dart` version 2; one base for the two C interfaces; the duplicated loops; a release profile | R10, R11, R17, R18 |
| 13 | #82: **Tests and CI**: the alloc-test flake (the counting moved to an isolate of its own; what it then found fixed: lists that gave their storage back, a double per physics step); a soak test, decoder fuzzing (the gltf crate's panics made errors), Miri (all clean); tumble run in CI | R20, R21 |
| 14 | **Docs**: every public member documented and the lint on; one error type for one mistake; plumbing out of the public API; the guide's recipes and §20 | C, F10, F16, F17, F19, F23, F24, F26, F27, F28, F31, F34, F35 |
| 15 | **Measured again**: the step 5 checks after 7 and 8; a Note10+ profile with Dart symbols; the Mac's GPU memory; Android performance hints | S |

**Left for stage 7**, as the plans say: themes and colours shared with the
game (F6), a widget at a world point (F33), the app's pause (R10's part),
one GPU device per process (R13). F11 was answered by step 4. **With
animation or the look**: R12's passes and lights, R14's texture memory,
R15's crates.
