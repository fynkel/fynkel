# Fynkel Engine — documentation

| Document | Role |
| --- | --- |
| [`status.md`](status.md) | **Start here.** What exists, what is proven end to end, what is not, known compromises, how "green" is verified. Rewritten at every gate. |
| [`foundation-roadmap.md`](foundation-roadmap.md) | **Current work.** The September 30 foundation program: ECS and unified gameplay state, nine milestones, task status, acceptance criteria, and evidence gates. |
| [`foundation-baseline.md`](foundation-baseline.md) | September 30 F0 baseline commands, reproduced defects, existing failures and verification limits. |
| [`design/foundation-ownership.md`](design/foundation-ownership.md) | Code-based state owners, mutation routes, frame order, dependencies and finding-to-experiment map. |
| [`design/foundation-prototype.md`](design/foundation-prototype.md) | Experimental shared object/system access, contract tests and Dart/native storage/crossing evidence; unresolved F0 gate work. |
| [`design/foundation-state-comparison.md`](design/foundation-state-comparison.md) | Mixed built-in/custom transaction contracts, adapter/shared-kernel comparison, frame/allocation/bridge captures and the ADR-0046 decision. |
| [`design/foundation-identity-migration.md`](design/foundation-identity-migration.md) | Live 128-bit identity migration, ABI changes, actual-world regressions, measurements and remaining authority/qualification work. |
| [`design/foundation-identity-authority.md`](design/foundation-identity-authority.md) | Process-lifetime host authority and independent-library/replacement qualification. |
| [`design/foundation-clock-migration.md`](design/foundation-clock-migration.md) | Canonical native clocks, timed gameplay, editor Step, host acceptance, ABI 3 and qualification evidence. |
| [`design/foundation-transform-migration.md`](design/foundation-transform-migration.md) | Exact affine operations, reparenting, interpolation, physics constraints and F1.3 qualification. |
| [`design/foundation-bounds-migration.md`](design/foundation-bounds-migration.md) | Conservative affine and animated bounds, camera/shadow regressions and update cost. |
| [`roadmap.md`](roadmap.md) | Product roadmap and prior completion history; the foundation program precedes the remaining feature work. |
| [`foundation.md`](foundation.md) | The September 27 audit, fix history, and regression evidence. New architecture work is tracked in `foundation-roadmap.md`. |
| [`blueprint.md`](blueprint.md) | The development blueprint (v0.4). Architecture, contracts, milestones M0–M7, backlog. |
| [`decisions/`](decisions/) | Architecture Decision Records. **Accepted** decisions override the blueprint where they conflict; proposed records are designs awaiting resolution. |
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
| [`shaders.md`](shaders.md) | Writing a shader (ADR-0031): one `.shader` file — the header (menu name, lighting, alpha modes, shadows, properties) and a WGSL `surface()` the engine lights — made in the editor or by hand, its materials, `vertex()`, changing values from Dart, errors, what is not there yet. |
| [`animator.md`](animator.md) | The Animator Controller (ADR-0032): making one, the Animator window (states, transitions, parameters, layers), setting parameters from Dart, watching it in Play, the two games' controllers. |
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
| 0011 | Scale rule: Dart for the few, native components for the many | Accepted; direction amended by 0042 |
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
| 0022 | Prefabs: one root, the instance is the root | Accepted |
| 0023 | [Built-in primitives and GameObject presets](decisions/ADR-0023-built-in-primitives-and-gameobject-presets.md) | Accepted |
| 0024 | [Raw keys beside actions](decisions/ADR-0024-raw-keys-beside-actions.md) | Accepted |
| 0025 | [Physics materials](decisions/ADR-0025-physics-materials.md) | Accepted |
| 0026 | [Building a game from code](decisions/ADR-0026-code-first-building.md) | Accepted |
| 0027 | [Data assets](decisions/ADR-0027-data-assets.md) | Accepted |
| 0028 | [Editor extensions](decisions/ADR-0028-editor-extensions.md) | Accepted |
| 0029 | [Package architecture](decisions/ADR-0029-package-architecture.md) | Accepted |
| 0030 | [Inspector previews](decisions/ADR-0030-inspector-previews.md) | Accepted |
| 0031 | [Shader assets](decisions/ADR-0031-shader-assets.md) | Accepted |
| 0032 | [Animator Controller](decisions/ADR-0032-animator-controller.md) | Accepted |
| 0033 | [Animation v2](decisions/ADR-0033-animation-v2.md) | Accepted |
| 0034 | [Compound colliders](decisions/ADR-0034-compound-colliders.md) | Accepted |
| 0035 | [Object references](decisions/ADR-0035-object-references.md) | Accepted |
| 0036 | [Object interface](decisions/ADR-0036-everything-is-an-object.md) | Accepted; state/access review in 0043 and 0044 |
| 0037 | [Objects surviving scene changes](decisions/ADR-0037-dont-destroy-on-load.md) | Accepted |
| 0038 | [Background scene loading](decisions/ADR-0038-load-scene-async.md) | Accepted; readiness amendment in 0047 |
| 0039 | [Persistent objects](decisions/ADR-0039-persistent-objects.md) | Accepted |
| 0040 | [Background cook](decisions/ADR-0040-background-cook.md) | Accepted |
| 0041 | [Runtime trusts the cook](decisions/ADR-0041-runtime-trusts-the-cook.md) | Accepted |
| 0042 | [ECS centered foundation program](decisions/ADR-0042-ecs-centered-foundation-program.md) | Accepted direction; amends 0011 policy |
| 0043 | [Unified world state and frame contract](decisions/ADR-0043-unified-world-state-and-frame-contract.md) | Accepted through 0046/0047; migration pending |
| 0044 | [Components systems and gameplay access](decisions/ADR-0044-components-systems-and-gameplay-access.md) | Proposed; F0 prototype required |
| 0045 | [Asset readiness and render resources](decisions/ADR-0045-asset-readiness-and-render-resources.md) | Readiness accepted through 0047; budgets/render design remain F5/F6 |
| 0046 | [Transactional world state replacement](decisions/ADR-0046-transactional-world-state-replacement.md) | Accepted strategy and pure-data contract; migration pending |
| 0047 | [Identity clocks frame order and readiness](decisions/ADR-0047-identity-clocks-frame-order-and-readiness.md) | Accepted contracts; migration pending |
| 0048 | [Exact affine transform contract](decisions/ADR-0048-affine-transform-contract.md) | Accepted; live migration |
| 0049 | [Conservative render bounds](decisions/ADR-0049-conservative-render-bounds.md) | Accepted; live migration |
| 0050 | [Process-lifetime identity authority](decisions/ADR-0050-process-lifetime-identity-authority.md) | Accepted; live migration |
| 0051 | [F2 boundaries and presentation policy](decisions/ADR-0051-f2-boundaries-and-presentation-policy.md) | Accepted |
| 0052 | [Registered component values](decisions/ADR-0052-registered-component-values.md) | Accepted |
| 0053 | [Registered gameplay and extensions](decisions/ADR-0053-registered-gameplay-and-extension-contract.md) | Accepted |
| 0054 | [ECS change tracking](decisions/ADR-0054-ecs-change-tracking-boundary.md) | Accepted |
| 0055 | [Incremental synchronization and publication](decisions/ADR-0055-incremental-synchronization-and-publication.md) | Accepted |

ADR-0042 defines the foundation direction. ADR-0046 accepts the replacement strategy and pure-data contract; ADR-0047 resolves identity, clocks, frame order and readiness. Proposed portions describe remaining work; shipping migration remains tracked in the foundation roadmap.

## Still open

- ADR-0005 license — deferred with the rest of the publishing paperwork to a "public readiness (b)" item for when the repository goes public (blueprint §19.2d item 17, done thin on 2026-09-22).
- Whether Windows is 1.1 or later.
- MCP server on top of the CLI (post-1.0, cheap).
- ~~Runtime-inspection channel between editor and the separate Play process (design in M4).~~ Resolved 2026-09-21 (ADR-0016 resolutions): JSON lines on the game's stdout under `FYNKEL_EDITOR_CHANNEL=stdio`; inspection itself is built after Editor V0.
- ~~The docking package for Editor V0 (spike in M4 item 2; ADR-0016 resolutions).~~ Resolved 2026-09-21: `docking` 1.16.2 passes all seven criteria (`apps/editor/test/spike/docking_spike_test.dart`); the shell is built on it.
