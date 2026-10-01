# Host identity authority qualification (F1.1)

**Date:** 2026-09-30  
**Decision:** [ADR-0050](../decisions/ADR-0050-process-lifetime-identity-authority.md), implementing the authority requirement of ADR-0047.  
**Source:** `main` working tree against `87e3dec8afa33c2c439c6fe848a7f0599cc512aa`; not a released revision.

## Implementation

`fynkel_contracts::IdentityAuthority` owns a version tag and an atomic monotonic counter. The module-wide one-time selector points to that storage instead of holding its own resettable counter. `allocate_world_id` is still the single route used by ECS, core, runtime and authoring worlds.

Bootstrap retains the allocation for the process lifetime. No function pointer or callback refers back into the originating library, and unloading the library does not free the counter. The host keeps the opaque authority token independently of engine module handles. Later modules attach it before first use. Reattaching the same token succeeds; a different token, null pointer or incompatible authority version is rejected. Existing namespaces and the selected authority never change.

The C API adds `fynkel_identity_authority()` and `fynkel_identity_attach(authority)`. Runtime ABI remains 3 and the entity/wire fingerprint is unchanged. The authority's separate opaque ABI is versioned so future storage changes can be rejected explicitly. The C pointer safety contract requires a valid process-lifetime authority obtained from a compatible module; a random pointer is not validated memory.

SDK embedding hosts can retain `NativeLibrary.identityAuthority` outside replaceable engine state and pass it to `NativeLibrary.open(identityAuthority: token)`. Ordinary SDK/editor applications continue to share one native code asset and require no new game-facing API. Custom native module loaders call the two C entry points on their respective module handles. Calling a replacement module's getter before attaching would independently bootstrap it; the attempted switch then fails.

## Executable evidence

`crates/fynkel_native/tests/identity_libraries.rs` is an opt-in dynamic-loader integration test, run against a freshly built release native cdylib:

1. Copy the library to independent paths and load two engine images.
2. Bootstrap an authority in the first; attach the second to the same storage.
3. Create an actual runtime, world and entity in each. Their slot/generation values match, but world IDs differ. Foreign `contains` and destroy operations are rejected; the destination entity survives.
4. Bootstrap a separate image and verify its authority address differs. Reject attempts to swap either selected authority. This checks actual image independence instead of assuming it from filenames.
5. Destroy both original worlds/runtimes and close both library handles.
6. Load ten replacement images in sequence. Each attaches the retained authority, creates a world with a monotonically newer ID and rejects both original references, then destroys its objects and closes its library handle.

Loader handle closure is tested on macOS arm64. OS loaders may retain internal code mappings; correctness does not depend on those mappings or on the original module's function addresses. The host token and its heap storage are the only retained identity authority used by replacements. Linux/Windows/iOS packaging qualification remains in the platform release gates.

Contract unit tests allocate 8,000 IDs concurrently, verify sticky attachment and reject a bad authority ABI without selecting it. Existing exhaustion tests seed the counter near u64::MAX; actual ECS tests retire generation-exhausted slots. SDK coverage adds authority retention across runtime disposal and repeated attachment. The earlier 300-world/Play-style recreation, foreign-reference and wire/JSON tests remain in place.

## Cost and qualification

A correct host retains one 16-byte authority payload plus allocator overhead until process exit. World creation performs one one-time-cell read and one atomic counter update. No additional entity bytes, bridge traffic, per-tick allocation or authority callback is introduced. The loader test deliberately bootstraps a second isolated authority to verify rejection; that test-only allocation also lasts until process exit.

The `authority` suite in `tools/foundation_baseline.py` builds the native cdylib, runs the 15-command foundation qualification suite, then runs the explicit dynamic-loader test. It records toolchain, hardware, working-tree hashes and raw logs. The [archived capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-authority.json) records all **17 commands passing**: **832 Rust workspace tests** (8 ignored in the normal suite), the **explicit dynamic-loader test**, **354 SDK**, **16 authoring**, **39 Flutter**, and **91 focused editor tests**. Compilation, strict Clippy, formatting, code generation and analyses pass. The loader test ends at world ID 12 after two original worlds and ten replacements. Raw logs are in `target/foundation-baseline/identity-authority`. Original identity, clock and transform probes remain passing.

## Remaining program work

This closes the missing multi-module allocation mechanism. It does not implement F2's separately tagged transaction references, commit/abort semantics or live state migration. Hosts still must perform the explicit authority handoff before replacing a module. Broader editor/reference-example, platform packaging, AOT and performance release gates remain F0/F7/F8 work; this report is not a full-engine release certification.
