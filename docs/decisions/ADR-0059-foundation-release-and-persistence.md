# ADR-0059: Foundation qualification and gameplay persistence

Status: Accepted
Date: 2026-10-03

## Context

F1–F7 establish one gameplay state authority. F8 must distinguish tested
contracts from product claims, and persisted gameplay identity from ephemeral
world handles. The user has one Mac and an available physical iPhone; a second
Mac is unavailable. ADR-0005 licensing remains an owner decision before public
publication.

## Decisions

- Qualification runs sequentially through `tools/foundation_release.py`, pinning
  the current release CLI, retaining failures, raw measurements and source/log
  hashes. A source change during capture invalidates that run. Regression
  comparisons require matching machine and suite, complete workload keys and
  actual samples. A timing ceiling is a regression alarm, not a 60fps promise.
- Persistence is **game-owned, versioned save data**. Games choose persistent
  object IDs and fields; loading creates new runtime identities and resolves
  references within one gameplay transaction. The registered combat sample
  demonstrates Health values, forward/self references, duplicate display names,
  validation, missing-reference refusal, and failed-restore rollback. File
  storage and replacement of the running session belong to the application.
- The sample save format is `fynkel.combat-save`, version 1. It saves the selected
  Health objects, their display names and health/invulnerability/target fields.
  It does not serialize every engine subsystem, transforms, callbacks,
  coroutines, physics solver state, animation playback or asset residency.
  Unsupported versions require an explicit game migration. Unknown save fields
  are refused rather than silently discarded. Authoring documents remain a
  separate format with their existing unknown-data preservation contract.
- General execution snapshots and deterministic rollback/networking are not a
  foundation-release promise. If added, they require a dedicated post-foundation
  persistence/networking milestone and ADR with supported-state and cross-build
  compatibility tests. Fixed ticks and input recordings alone do not prove it.
- The old `fynkel.dart` umbrella is retired now. `gameplay.dart` owns ordinary
  exports; `advanced.dart` includes them and adds native integration types.
  Generators, examples, templates and tests migrate together. Runtime and
  authoring ABIs do not change for a Dart import migration.
- macOS and iOS are the intended runtime platform scope. Browser portability
  compilation is not browser runtime support. Second-Mac execution remains
  unverified until hardware is available. Physical-device results must name the
  app/build, device and observed actions, separating human observations from
  automated checks. No local run is relabeled as a second-machine run.
- The final gate can declare Ready with named exceptions only when all required
  local correctness checks pass. Required correctness/dependency failures mean
  Not ready. Missing hardware evidence is an explicit release limitation, not
  a passing check. Nothing is published until ADR-0005 is resolved by the owner.

## Alternatives and consequences

A universal memory snapshot would imply restoring arbitrary Dart and native
execution state without a defined compatibility contract. We reject that promise.
Project-owned data saves are small and testable, and restore through the existing
transaction authority. Games must explicitly decide what their save contains.

Keeping the umbrella indefinitely would leave a migration unfinished. Removing
it before public release makes the API boundary explicit; existing projects must
change imports and regenerate bindings. This is an intentional source break,
not a runtime state or scene-format change.

A second Mac cannot be simulated by rerunning on this Mac. That acceptance item
stays open, with the consequence that broad macOS release qualification cannot
be claimed from this machine's evidence alone.
