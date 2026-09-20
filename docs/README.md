# Fynkel Engine — documentation

| Document | Role |
| --- | --- |
| [`status.md`](status.md) | **Start here.** What exists, what is proven end to end, what is not, known compromises, how "green" is verified. Rewritten at every gate. |
| [`blueprint.md`](blueprint.md) | The development blueprint (v0.4). Architecture, contracts, milestones M0–M7, backlog. |
| [`decisions/`](decisions/) | Architecture Decision Records. **Binding**; they override the blueprint where they conflict. |
| [`design/dart-sdk.md`](design/dart-sdk.md) | Dart SDK and standard component spec. Living draft, revisited at M1. |
| [`design/editor-ux.md`](design/editor-ux.md) | Editor UX spec. Living draft, revisited at M4. |
| [`ideas.md`](ideas.md) | Post-1.0 ideas (plugins, roadmap). Non-binding. |
| [`m0-architecture-report.md`](m0-architecture-report.md) | M0 go/no-go: gate results, measured numbers, Path A selected, Path C fallback. |
| [`m1-gate-report.md`](m1-gate-report.md) | M1 go/no-go: gate results, Dart allocation per frame, leak tests, bandwidth. |
| [`toolchain.md`](toolchain.md) | Frozen toolchain matrix (M0). |
| [`codegen.md`](codegen.md) | Schema codegen pipeline: Rust structs → JSON Schema, Dart and Rust codecs, fixtures (M1). |
| [`documents.md`](documents.md) | Authoring documents (M2): envelope, strict JSON rules, canonical form, diagnostics, `fynkel document`. |
| [`hosts.md`](hosts.md) | The hosts sharing the core (headless, standalone, Flutter) and how each drives `step()` (M1). |
| [`input.md`](input.md) | The input contract (M3-07): actions and the binding grammar, the §5.1 held/edge rules, recordings, the Flutter capture, gamepads, the touch overlay. |
| [`physics.md`](physics.md) | The physics contract (M3-08): `RigidBody`/`Collider`, the project's gravity and layers, the step and transform ownership, body commands, contacts, queries, the headless report, cost. |
| [`../platform/apple/README.md`](../platform/apple/README.md) | Apple (macOS + iOS) platform integration contract (§8.4): IOSurface ring, synchronization, resize, lifecycle. |

## Decision index

| ADR | Title | Status |
| --- | --- | --- |
| 0001 | 1.0 scope and native-first policy | Accepted |
| 0002 | M0 scope and Flutter/wgpu viewport composition path | Accepted, measured; Path A selected |
| 0003 | `hecs` behind `fynkel_ecs` | Accepted |
| 0004 | Dart/Rust command-snapshot protocol and wire format | Accepted |
| 0005 | License, repository, contribution, trademark | **Open** — before M3 public push |
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
| 0016 | Editor architecture | Accepted |
| 0017 | AI and automation interface | Accepted |

## Still open

- ADR-0005 license (decide before going public at M3).
- Whether Windows is 1.1 or later.
- MCP server on top of the CLI (post-1.0, cheap).
- Runtime-inspection channel between editor and the separate Play process (design in M4).
