# F7 unified gameplay workflow gate

Date: 2026-10-03. Result: **F7.1–F7.4 complete for the qualification scope below.**

Implementation is in the working tree on base `22229b4f1967f4d07a8f0ed968ba64d7342648d5`, alongside earlier uncommitted F5/F6 work. [ADR-0058](decisions/ADR-0058-unified-gameplay-workflow.md) records the decisions; [the gameplay guide](gameplay.md) explains the supported workflow. [Machine-readable evidence](../benchmarks/foundation/results/2026-10-03-22229b4f-f7-completion.json) records commands, results, source hashes, local log hashes and qualification boundaries.

## Implementation and acceptance

| Item | Implementation and evidence |
| --- | --- |
| F7.1 | Ordinary code imports `gameplay.dart`; host integrations opt into `advanced.dart`. These export the same underlying types and World, with no additional state authority. Typed references, behaviours, registered data and systems share the existing gameplay transaction contract. Maintained game sources and the starter compile with the ordinary import. The compatibility `fynkel.dart` umbrella is a forwarding surface with retirement assigned to the F8 public API compatibility gate. This is API organization, not a restriction on access to World.native. |
| F7.2 | `ObjectRecipe` validates factories that return fresh spec nodes and behaviour instances, using weak identity tracking. `World.buildAll` creates a forest in one transaction, allocating every root before resolving references. Single-tree build delegates to it. Permanent tests cover repeated creation, independent execution fields, internal references, invalid factories, cycles, rollback, cross-root forward references, clone, two prefab instances and scene write/load. No hidden parent or second construction implementation is used. |
| F7.3 | Built-in and registered Play component-field batches now commit through one native gameplay journal, with resource preparation and stale-revision rejection. Single-component writes use that same path. A real-editor regression first reproduced a rejected batch leaving health changed, then proved the fix. Native regressions cover both decoding and semantic failures in mixed batches. Existing editor suites qualify inspector/schema access, authoring undo/revisions, Apply to Scene, paused simulation, reload/restart, package access and recovery. Runtime dependency isolation and generated contracts pass. |
| F7.4 | Maintained game sources, the starter, package and registered extension examples use the ordinary API. The tracked `code_only_try` sample was migrated with the owning migration/generation tools: current documents and lock, Pose values, typed identities and object references, and atomic forest creation. Original migration backups remain. Asset discovery now excludes recognized document backups consistently in full and incremental scans; unknown files still diagnose normally. Existing document migration/unknown-data preservation coverage passes. Authoring ABI is 6; generated bindings and compatibility tests agree. Runtime ABI remains 5 and viewport ABI 14; this change does not require another document-format bump. |

## Qualification

Apple M3 Max, macOS arm64, Metal. Native qualification uses release builds and locked dependencies. Tests explicitly select the rebuilt repository CLI; native libraries were rebuilt before final Dart/Flutter qualification.

| Check | Result |
| --- | --- |
| Rust workspace, serial test threads | 1,172 passed; 0 failed; 12 ignored manual tests |
| Final affected Rust core/authoring/native suites after live-batch changes | 545 passed; 0 failed; 3 ignored manual tests |
| Workspace Clippy, all targets, warnings denied | Passed |
| Formatting, codegen consistency, runtime dependency isolation | Passed |
| SDK analysis and full tests | No issues; 613 passed |
| Editor analysis and full tests | No issues; 589 passed; one existing skip |
| Authoring package analysis and full tests | No issues; 16 passed |
| Flutter package analysis and full tests | No issues; 48 passed |
| Generator analysis and tests | No issues; 14 passed |
| Registered gameplay example analysis and tests | No issues; 3 passed |
| Native combat extension headless example | Passed; registered system changed player health |
| Maintained games: code_only / reference_game / showcase / code_only_try | Validation, codegen checks and analysis passed; 18 / 10 / 3 / 18 game tests passed (reference_game has one existing skip); all four headless runs completed 600 ticks |
| Fresh generated starter | Validation, codegen check and analysis passed; 6 tests passed; 600-tick headless run passed |

The full Rust workspace run preceded the final live-batch implementation; the final affected Rust suites and workspace Clippy cover that implementation. The four maintained game workflows were rerun against the final shared forest build. The SDK authoring regression compares runtime creation with authored scene/prefab loading, while editor tests exercise the authoring and Play boundaries. No manual interactive editor session or exported application is claimed by these automated checks.

## Defects and retry history

- Play could apply the first component edit before rejecting a later invalid edit. The new regression reproduced health changing from 70 to 25 despite refusal. Native batching now preserves the original value and the authoring revision on refusal.
- Sequential top-level builds could not resolve a camera reference to a later player root. The shared forest transaction resolves the complete graph before filling components. The migrated sample's live-built and loaded scenes now pass the same comparison.
- Official migration backups were being indexed as assets without sidecars. Both discovery routes now recognize document backups without deleting them; a permanent regression also checks that arbitrary unknown `.bak` files retain their diagnostic.
- The old tracked sample exposed legacy type, schema and name-reference errors. These were migrated explicitly and regenerated rather than bypassing validation.
- Test development exposed an incomplete PointLight fixture, a mismatched exception expectation and an initial change to the existing live rejection protocol. The fixture/expectation were corrected and the original gameplay rejection contract restored; the existing stale-reference regression passes.
- An editor hierarchy timing assertion exceeded its unchanged 2,000 ms limit during concurrent native builds. The final full editor run passed in isolation; the threshold was not relaxed. Earlier failure logs are retained where available.

## Deliberate boundaries

`ObjectRecipe` is a factory contract, not automatic cloning of arbitrary Dart state. Nodes and behaviours must be fresh; canonical const spec nodes cannot be reused across calls. Existing behaviour-free ObjectSpec rebinding semantics remain documented for compatibility. Reusable linked trees should use recipes.

Live component-data field batches are atomic. Structural commands and arbitrary Dart behaviour setters retain ADR-0051's explicit independent-command semantics: completed commands remain if a later independent command fails. Arbitrary Dart side effects are not represented as rollback-safe component data. Scene document undo is separate from live execution. This boundary is stated in the API and guide, not hidden behind a successful-batch claim.

The fresh starter was created with `--no-flutter`, then qualified as a complete Dart/game/assets project. Flutter platform-runner generation, physical iOS export and second-machine runtime qualification remain F8. The maintained sample scope is tracked repository content; ignored local experiments (`rg_gate`, `rg_hero`, `shader_try`) were not modified. Local qualification logs and the generated starter live under ignored `target/`; committed evidence contains their hashes and commands.

F8 remains the production gate: broader workloads and release budgets, persistence policy, physical-device/second-machine checks, and final public API/documentation retirement. F7 completion does not establish those results or guarantee absence of undiscovered defects.

## Follow-up implementation review (2026-10-03)

The review inspected the ordinary/advanced API exports, recipe freshness and
forest allocation/reference publication, code-authored scene/prefab tests, live
component decoding and transaction ownership, migration discovery, and ABI
compatibility guards. The original completion evidence was compared against
current source hashes; only the two editor files changed during this review.

One contract weakness was found and fixed: `PlayDocument` used a backend
capability flag to fall back to sequential field writes. Although the production
native backend supported atomic batches, another backend could silently weaken
the component-data transaction guarantee. All component-data field batches now
call the atomic backend operation. An unsupported backend refuses before any
write; it cannot fall back to independent writes. Test backends implement their
atomic contract explicitly. The existing structural/script independent-command
boundary is unchanged.

A permanent test proves that an unsupported backend preserves data and revision
and performs no writes. Reinstating the previous fallback condition made that
test fail (it returned success); restoring the fix passes. A second real-native
regression proves that multiple field edits compose and that an invalid
intermediate replacement cannot be hidden by a later valid replacement.

Final checks: editor analysis clean; **591 editor tests passed**, one existing
skip; **9 SDK recipe/authoring tests**, **10 native live-edit tests**, and **6
asset discovery tests** passed. Formatting and diff whitespace checks pass. No
other concrete F7 defect was found in the reviewed paths; this does not establish
that undiscovered defects are impossible. The recorded F7 boundaries and F8
obligations remain in force. [Review evidence](../benchmarks/foundation/results/2026-10-03-22229b4f-f7-review.json)
records source/log hashes and commands separately from the original completion
capture.
