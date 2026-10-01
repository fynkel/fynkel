# F0 baseline: September 30, 2026

This is a partial baseline, **not the F0 exit gate**. Runtime behavior has not been changed. [The roadmap](foundation-roadmap.md) remains the task ledger; [the ownership inventory](design/foundation-ownership.md) maps findings to code and outstanding experiments.

## Reproduce

From the repository root:

```sh
python3 tools/foundation_baseline.py --list
python3 tools/foundation_baseline.py --only sdk,probe
python3 tools/foundation_baseline.py
```

The runner uses Flutter's bundled Dart, builds this checkout's CLI before package suites that need it, and executes commands sequentially. Pass `--flutter /path/to/flutter` if needed, `--timeout SECONDS` to bound each command, or `--output PATH` for a new output directory. It preserves per-command logs and incrementally writes revision, working-tree status, tracked-diff/untracked-file hashes, hardware, toolchain, commands, durations and outcomes to `report.json`. A failing suite does not conceal later suite results; interruption stops the capture. Resolution/build/test commands can update caches and some tests may create project sidecars. Review the working tree afterward.

The first capture predates automatic CLI building and untracked-file hashing in the runner: its package checks used the checkout's existing CLI. A clean-checkout full run with the revised runner is still required. Raw output is ignored under `target/foundation-baseline`; checked-in JSON preserves the useful summary. Working-tree hashes identify differences but do not archive source bytes; retain the commit and corresponding patch for exact reproduction.

## Proven observations

[Diagnostic output](../benchmarks/foundation/results/2026-09-30-87e3dec8-probe.json), captured through [the real SDK/native diagnostic](../packages/fynkel/tool/foundation_probe.dart):

| Contract | Observation | Destination |
| --- | --- | --- |
| World identity | Two simultaneous default worlds produce handle `100000100000000`; using that handle in the other world resolves to its object. Destroying and recreating a world produces the same handle again. | F1.1 |
| Clock accounting | A .25s frame with a four-step cap reports delta .25s, drops about .16667s, and advances public elapsed .08333s. A .1s coroutine wait has already resumed. | F1.2 |
| Transform composition | A rotated child under scale(2,1,1), with grandchild offset(1,0,0), reports Dart world position approximately (1.4142,1.4142,0), native (1.4142,.7071,0). Error is .7071m after a completed tick. | Historical defect; [F1.3 affine migration](design/foundation-transform-migration.md) now reports 7.882e-8m error. |

The identity observation above is historical: the [live identity migration](design/foundation-identity-migration.md) now rejects it and adds actual-world regressions. The [clock migration](design/foundation-clock-migration.md) also resolves the elapsed/wait disagreement. Transform observations remain open.

The diagnostic intentionally reports behavior rather than asserting that a defect stays present. Once ADR-0043 chooses time and identity contracts, implementation must add regressions covering the old defect and the new behavior. Bounds, phase visibility and loading failures still need executable reproductions.

## Validation and limits

Revision: `87e3dec8afa33c2c439c6fe848a7f0599cc512aa`, dirty working tree on `main`. Machine: Apple M3 Max, macOS26.5.1 arm64. Cargo1.93.0; Flutter3.47.2 with Dart3.13.2. [Capture metadata and outcomes](../benchmarks/foundation/results/2026-09-30-87e3dec8-baseline.json) include the exact starting status. The existing generated reference-game Dart asset change was preserved.

| Area | Result |
| --- | --- |
| SDK | Package resolution, analysis with fatal infos, tests and native diagnostic passed. |
| Generator and generator example | Resolution, analysis and tests passed. |
| Authoring package | Resolution, analysis and tests passed. |
| Flutter integration and editor API | Resolution, analysis and tests passed. |
| Editor | Resolution and analysis passed; test suite failed on reference-project validation. |
| Apple Flutter host | Resolution and analysis passed; native Cargo test/build interrupted before completion. |
| ECS/scene/core targeted tests | Multiple groups passed; overall run interrupted while a test process was idle. No complete-suite pass claimed. |
| Stripped runtime | Build interrupted before completion. |
| Native lint, codegen and release performance | Rust formatting and codegen checks passed; Clippy exceeded the 45-second budget. Current-revision release headless capture passed. |

The editor failure reports the reference hero prefab using formatVersion5 against current7 (FYN-D0016), plus two empty-string skeleton references where null/object is required (FYN-D0206). No prefab migration was performed in this batch. This belongs to the existing-failure baseline and F7 migration work; first establish a green fixture baseline deliberately rather than suppressing validation.

The first host compiler stopped producing output and was observed idle for roughly eight minutes; the targeted core test process was also idle. They were terminated. A subsequent stripped compilation showed another stall and was interrupted. These are incomplete native checks, not asserted engine failures or evidence of passing. The cause has not been established. An editor-test-generated backup metadata sidecar was removed; authored files were preserved.

The original raw artifacts are in `target/foundation-baseline/20260930T111654791896Z`. The follow-up uses a 45-second per-command budget to bound stalled commands; such a timeout does not establish that the code is incorrect. No stale release binary is substituted for a current-revision performance capture.

[Follow-up command results](../benchmarks/foundation/results/2026-09-30-87e3dec8-followup.json) record formatting, Clippy, codegen and release execution. [Headless performance data](../benchmarks/foundation/results/2026-09-30-87e3dec8-headless.json) captures 3,000 ticks of the procedural demo: one hub and 24 satellites, seed42. Native step p50/p95 is 4/10µs, extraction p50/p95 is 4/15µs, and total measured run time is 34.49ms. Peak counted native memory is 217,601 bytes. This small native-only workload has no Dart bridge, renderer or active physics population and cannot qualify a large game. Compilation time is excluded from these runtime metrics.

[Scale observations](../benchmarks/foundation/results/2026-09-30-87e3dec8-scale.json) repeat the dense procedural transform workload for 300 ticks at larger populations:

| Live entities | Step p95 | Extraction p95 | Peak counted native memory |
| --- | --- | --- | --- |
| 1,001 | .139ms | .255ms | 1,264,793 bytes |
| 10,001 | .879ms | 1.949ms | 14,750,553 bytes |

These exclude Dart, GPU rendering, physics bodies and streaming. Native validation was concurrently pending, so treat the timings as initial observations, not stable regression budgets. Static/sparse change workloads and phase-level allocation measurements remain outstanding.

A [sequential native retry](../benchmarks/foundation/results/2026-09-30-87e3dec8-native-retry.json) again passed Apple Flutter host resolution/analysis, then timed out after 180 seconds while compiling native physics. The subsequent stripped attempt was interrupted; workspace tests and the retry's lint suite were not reached. This repeated condition prevents a complete native baseline in this batch. Raw retry artifacts are in `target/foundation-baseline/20260930T113156860945Z`.

## Remaining work before the F0 gate

Complete the native/stripped/host checks and current-revision release/profile workload captures. Measure sparse and dense changes, bridge calls/copies/allocations, render extraction, physics synchronization and asset readiness separately. Establish missing bounds/read-visibility/loading reproductions. Then implement the F0.3 gameplay slice and use its results to resolve ADR-0043 through ADR-0045. The ownership inventory is a starting map, not a completed API escape-path audit. F0.1 and F0.2 remain Active; F0.3 and F0.4 have not started.
