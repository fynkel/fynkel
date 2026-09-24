# Fynkel Engine — documentation

| Document | Role |
| --- | --- |
| [`status.md`](status.md) | **Start here.** What exists, what is proven end to end, what is not, known compromises, how "green" is verified. Rewritten at every gate. |
| [`roadmap.md`](roadmap.md) | The agreed order from here to 1.0 and after (Phase 1 foundation fixes → M4 → extensibility → rendering → M5 → M6). |
| [`blueprint.md`](blueprint.md) | The development blueprint (v0.4). Architecture, contracts, milestones M0–M7, backlog. |
| [`decisions/`](decisions/) | Architecture Decision Records. **Binding**; they override the blueprint where they conflict. |
| [`design/dart-sdk.md`](design/dart-sdk.md) | Dart SDK and standard component spec. Living draft, revisited at M1. |
| [`design/editor-ux.md`](design/editor-ux.md) | Editor UX spec. Living draft, revisited at M4. |
| [`ideas.md`](ideas.md) | Post-1.0 ideas (plugins, roadmap). Non-binding. |
| [`m0-architecture-report.md`](m0-architecture-report.md) | M0 go/no-go: gate results, measured numbers, Path A selected, Path C fallback. |
| [`m1-gate-report.md`](m1-gate-report.md) | M1 go/no-go: gate results, Dart allocation per frame, leak tests, bandwidth. |
| [`m2-gate-report.md`](m2-gate-report.md) | M2 go/no-go: the seven data-layer criteria, corpus tallies, tooling timings, carry-over D1–D8. |
| [`m3-gate-report.md`](m3-gate-report.md) | M3 go/no-go: the six criteria with evidence, the clean-clone run, the stress and reference-game numbers, the decided deviations, carry-over E1–E9. |
| [`toolchain.md`](toolchain.md) | Frozen toolchain matrix (M0). |
| [`catalogue.md`](catalogue.md) | The native component catalogue (M3-10): `Rotator`, `Oscillate`, `Follow`, `LookAtCamera`, `LifetimeDespawn`, `Emitter` — semantics, where in the tick they run, prefab registration, per-system time, cost. |
| [`codegen.md`](codegen.md) | Schema codegen pipeline: Rust structs → JSON Schema, Dart and Rust codecs, fixtures (M1). |
| [`documents.md`](documents.md) | Authoring documents (M2): envelope, strict JSON rules, canonical form, diagnostics, `fynkel document`. |
| [`hosts.md`](hosts.md) | The hosts sharing the core (headless, standalone, Flutter) and how each drives `step()` (M1). |
| [`input.md`](input.md) | The input contract (M3-07): actions and the binding grammar, the §5.1 held/edge rules, recordings, the Flutter capture, gamepads, the touch overlay. |
| [`scenes.md`](scenes.md) | Scenes at runtime (M3-12): replace and additive loads, unload, the Dart `SceneManager`, `fynkel.TriggerZone` and named zone events, runtime prefabs, the user's `settings.json` and the `SettingsStore`. |
| [`hud.md`](hud.md) | HUD models and world labels (M3-13): `GameModel`/`ModelValue` written by behaviours and bound with `ModelBinding`/`ModelBuilder`, the screen pattern (menu / HUD / pause / settings / results), the bounded projection view (`World.projection`, `fynkel_world_project`), `ProjectionLayer`/`WorldLabel`, the occlusion policy, the 64-anchor bound and `FYN-R0012`. |
| [`animation.md`](animation.md) | Animation (M3-19): the one clip core (channels over named nodes, poses, blending), skins and the joint palette, `fynkel.Animator`/`fynkel.Skin`, the tick, the `Animate` command, `AnimatorState` and `ANIMATION_ENDED`, glTF and FBX import of skins and clips (M3-19b: a Mixamo download straight in, four downloads as one character through the rig document), the skinned render path, the Dart `animation` control; animation depth (M3-19c): blend spaces as `.blend.json` documents played like a clip and phase-synced by a parameter, layers as a second `Animator` on a descendant, clip markers as events, opt-in root motion, the 200-rig gate. |
| [`audio.md`](audio.md) | Audio (M3-11): `fynkel_audio` — the backend decision (`cpal` + Fynkel's own mixer, no allocation in the callback), the engine on the tick and the mixer on the device thread joined by rings, `fynkel.AudioSource`/`fynkel.AudioListener`, the three buses, the 32-voice limit with priorities, the `Audio`/`PlaySound`/`AudioBus` commands, `AudioState`/`AudioStats`, `PLAYBACK_ENDED`, the device and the host lifecycle, the Dart `audio` control and `world.audio`, import and the cache. |
| [`editor.md`](editor.md) | The editor (M4, in progress): the authoring session and its C ABI (`fynkel_authoring_*`, cargo feature `authoring` — a game's runtime has none of it), `package:fynkel_authoring`, transactions as the unit of every edit, what each M4 item added. |
| [`instrumentation.md`](instrumentation.md) | Instrumentation (M3-16): where every §15.4 number is measured — the frame's split (`World.frameTimes`, `FynkelFrameStats`), the engine's system timers, extract/prepare/encode, GPU pass timestamps (Metal), the counting allocator and resident set, asset residency and the GPU memory estimate, Flutter timings, load time, the Dart heap probe (C3: allocation and GC from the VM's GC timeline, AOT in the profile bundle), the subscription budget (C5, `FYN-R0013`) — where each shows up (headless JSON, the standalone `[1s]` line, `FynkelFrameStats.summary()` `[stats]` lines, the hosts' HUDs), and the M0/M3 captures with the 12 ms p95 total-work check. |
| [`physics.md`](physics.md) | The physics contract (M3-08/09): `RigidBody`/`Collider`, the project's gravity and layers, the step and transform ownership, body commands, contacts, queries, the character controller and the third-person camera, the headless report, cost. |
| [`../platform/apple/README.md`](../platform/apple/README.md) | Apple (macOS + iOS) platform integration contract (§8.4): IOSurface ring, synchronization, resize, lifecycle. |

## Decision index

| ADR | Title | Status |
| --- | --- | --- |
| 0001 | 1.0 scope and native-first policy | Accepted |
| 0002 | M0 scope and Flutter/wgpu viewport composition path | Accepted, measured; Path A selected |
| 0003 | `hecs` behind `fynkel_ecs` | Accepted |
| 0004 | Dart/Rust command-snapshot protocol and wire format | Accepted |
| 0005 | License, repository, contribution, trademark | **Open** — deferred to public readiness (b), when the repository goes public |
| 0006 | Source document format and asset identity | Accepted |
| 0007 | Engine as a library; hosts own the loop | Accepted |
| 0008 | Dart execution model (main isolate, sync step, timestep, events) | Accepted |
| 0009 | 1.0 platform intent: macOS + iOS | Accepted |
| 0010 | Reference game tiers | Accepted |
| 0011 | Scale rule: Dart for the few, native components for the many | Accepted |
| 0012 | Codegen and component schema source of truth | Accepted |
| 0013 | Dart math types | Accepted |
| 0014 | Error reporting across the ABI | Accepted |
| 0015 | Native runtime distribution | Accepted |
| 0016 | Editor architecture | Accepted (Play superseded by 0021; amended 2026-09-24: codegen for Create → Behaviour) |
| 0017 | AI and automation interface | Accepted |
| 0018 | Editor extension model: declarative Inspector hints, no game code in the editor | Accepted (with M4 item 4; the roadmap's Phase 3 reopens it for Dart editor extensions) |
| 0019 | Gizmos, handles and Scene tools | Accepted |
| 0020 | Tags and layers | Accepted |
| 0021 | Play in the editor | Accepted |

## Still open

- ADR-0005 license — deferred with the rest of the publishing paperwork to a "public readiness (b)" item for when the repository goes public (blueprint §19.2d item 17, done thin on 2026-09-22).
- Whether Windows is 1.1 or later.
- MCP server on top of the CLI (post-1.0, cheap).
- ~~Runtime-inspection channel between editor and the separate Play process (design in M4).~~ Resolved 2026-09-21 (ADR-0016 resolutions): JSON lines on the game's stdout under `FYNKEL_EDITOR_CHANNEL=stdio`; inspection itself is built after Editor V0.
- ~~The docking package for Editor V0 (spike in M4 item 2; ADR-0016 resolutions).~~ Resolved 2026-09-21: `docking` 1.16.2 passes all seven criteria (`apps/editor/test/spike/docking_spike_test.dart`); the shell is built on it.
