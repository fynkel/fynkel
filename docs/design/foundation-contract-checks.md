# Foundation identity, clock, frame and readiness checks

**Date:** 2026-09-30  
**Revision:** `87e3dec8afa33c2c439c6fe848a7f0599cc512aa` plus the uncommitted foundation working tree.  
**Decision:** [ADR-0047](../decisions/ADR-0047-identity-clocks-frame-order-and-readiness.md).  
**Capture:** [Machine, commands, source hashes and outcomes](../../benchmarks/foundation/results/2026-09-30-87e3dec8-contracts.json).

`crates/fynkel_core/examples/foundation_contracts.rs` is an isolated executable contract model. It does not replace shipping handles, frame dispatch or the asset store. Clock planning delegates to the actual native `FrameClock`; gameplay/real accounting is a candidate wrapper. The earlier [runtime probe](../foundation-baseline.md) remains the evidence for existing handle and clock defects. The [mixed transaction comparison](foundation-state-comparison.md) remains the evidence for supported built-in/custom rollback and read visibility.

Run the reproducible capture:

```sh
python3 tools/foundation_baseline.py --only foundation-contracts \
  --flutter /Users/oumari/Documents/flutter/flutter/bin/flutter --timeout 60
```

Both steps pass: 13 release tests and Clippy with warnings denied. Rust formatting also passes. The archive contains the current working-tree hashes; it is not a release certification.

| Checks | Observation | Still needed in production |
| --- | --- | --- |
| Live worlds, recreation, shared-authority runtimes, concurrent allocation, >255 worlds | Full references differ and cross-world lookup rejects them | Host authority plumbing, actual ABI/codegen/Dart readers and all raw lookup APIs |
| Artificial namespace exhaustion and generation maximum | Creation fails repeatedly at exhaustion; retired slots cannot alias stale references | Actual ECS free-list retirement, diagnostics and atomic spawn failure |
| 0.25-second hitch and 2-second clamp | Gameplay deadline/tween elapsed matches gameplay delta; completed simulation time remains shorter under the step cap | Actual coroutine, tween, invoke, animation and test-context migration; model uses deadline arithmetic rather than running those consumers |
| Zero tick, pause, manual Step, half speed, invalid/huge inputs | Gameplay advances independently of ticks, pause holds it, Step advances only simulation | Native/SDK shared clock transport, consumer parity and frame/Step lifecycle |
| Two catch-up ticks and zero-tick update | Fixed/simulation/after-physics repeat per tick; update/late/publish occur once; edits increment revision without creating ticks | Real dispatcher, input/event delivery, callback rejection/error continuation |
| Visual override expiry and immutable published tuple | Override affects only publication and clears on next frame begin | Actual hierarchy/matrices, bounds, interpolation reset, physics feedback, frame-buffer lifetime |
| Required decode/simulation failure and optional failure | Required failures block activation; optional failures allow declared fallback; terminal failure cannot be overwritten | Corrupt/missing cache tests through actual store/background worker and diagnostic preservation |
| Cancellation and frame boundary | Late completion/upload/activation rejected; activation allowed only at a boundary | Actual request generations, thread cancellation and scene replacement rollback |
| Device generations | GPU readiness cannot leak across devices; loss leaves CPU/simulation readiness intact | Real uploads, fence completion, render fallbacks and device recovery |
| Retention handoff, persistent consumer and frame pin | Transfer has no release gap; final consumer controls eligibility for eviction | Existing `Residency` integration, caches/budgets and actual GPU/audio lifetime |

The models deliberately omit resource I/O, GPU calls, inspector/lifecycle integration and the general registry. No performance or complete concurrency claim follows from these checks. The process-wide authority is represented by one shared object; independently instantiated authorities are outside the permitted exchange protocol. New raw runtime references are not persistent or cross-process identifiers.

The next implementation unit is F1.1: migrate generated handle records and identity allocation together, with actual-world regression tests. The 128-bit layout is selected here; an eight-bit allocator patch would leave an avoidable editor-session exhaustion limit. F1.2 follows with one canonical clock and actual consumer regressions. Remaining F0 integration/baseline qualification continues in parallel with scoped correctness work; milestone completion still requires its entire exit gate.
