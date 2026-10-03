# ADR-0058: One gameplay and authoring workflow

Status: Accepted

Date: 2026-10-02

F7 extends the F2–F6 state, registration, ownership and resource contracts.

## Decisions

1. `gameplay.dart` is the ordinary import; `advanced.dart` exposes native/transport
   types for hosts and generated integrations. `fynkel.dart` remains a forwarding
   compatibility umbrella with retirement at the F8 public API compatibility
   gate. **F8 amendment (2026-10-03):** the umbrella is removed; generated code
   uses gameplay.dart and advanced.dart includes the ordinary surface. There is one World implementation and one mutation authority.
2. Reusable construction uses factories, exposed as `ObjectRecipe.create`.
   Each call creates new spec nodes and behaviour instances. Weak identity
   tracking rejects factory reuse without retaining every previously built game
   object. Internal references are assembled inside each invocation. External
   live references are explicit and remain subject to the existing validation.
   The resulting ObjectSpec uses the existing transactional build and authoring
   writer. `World.buildAll` allocates all roots before resolving cross-root
   references; single-tree build delegates to this same transaction path. No implicit cloning of arbitrary Dart execution state is attempted.
3. Live built-in/registered component-field batches are atomic through one
   gameplay journal, with preparation before publication and stale-revision
   rejection. Single-component writes use the same implementation. Backends that
   cannot perform an atomic component batch must refuse it before writing; there
   is no sequential fallback. Structural
   commands and arbitrary Dart behaviour setters keep ADR-0051's explicit
   independent-command boundary; neither scene undo nor Dart side effects are
   advertised as live transaction rollback.
4. Scene/prefab writing, loading, cloning, inspectors, Play, Apply to Scene,
   paused Step and reload/restart retain their existing canonical owners.
   Qualification must exercise these paths, including custom registered data,
   rollback, revisions and editor dependency isolation. No duplicate editor-only
   gameplay representation is introduced.
5. Existing project migrations preserve identities, unknown authored data and
   backup originals. Recognized document `.bak` files are not assets, in both
   full discovery and incremental updates. Arbitrary unknown source files still
   produce the normal diagnostic.
6. Authoring ABI 6 adds atomic live component batches. Runtime ABI 5 and
   built-in document versions are unchanged.
   Older examples still require the existing F1–F5 document/schema migration,
   regenerated bindings, lock updates and recooking. Migration is validated
   rather than bypassed with a stale lock or name-based reference fallback.

## Consequences

Game authors can mix local behaviours and bulk systems without learning the
transport. Factories provide clear execution ownership and work equally with
live builds and authored documents. Legacy ObjectSpec rebinding semantics remain
explicit for compatibility; reusable trees should use fresh factory instances.
The starter and maintained gameplay examples demonstrate the ordinary import;
host entry points opt into advanced types where required.

See [the gameplay guide](../gameplay.md) and the [F7 qualification report](../foundation-f7-gate-report.md).
