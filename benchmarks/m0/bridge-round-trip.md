# M0 backlog #10/#11 — C ABI v0 + ffigen, batched command/snapshot round trip

- Header: `crates/fynkel_native/include/fynkel.h` (hand-written, the contract).
- Rust: `crates/fynkel_native` (cdylib) — validation, staging, generational
  handles, subscriptions, snapshot/event/diagnostic buffers, `catch_unwind`
  panic boundary with world poisoning (ADR-0014).
- Dart: `packages/fynkel` — `dart run ffigen --config ffigen.yaml` generates
  `lib/src/native/fynkel_bindings.g.dart`; `native_world.dart` holds the v0
  hand-written codecs (to be generated from Rust schemas, ADR-0012).
- Tests: `cargo test -p fynkel_native`, `dart test` in `packages/fynkel`
  (uses Flutter's Dart 3.13.2: `~/Documents/flutter/flutter/bin/dart`; the
  Homebrew `dart` 3.8 on PATH is too old for the package).

## What the round trip covers

| Contract item (blueprint §5.3/§5.5) | v0 | Verified by |
| --- | --- | --- |
| byte order / scalar widths / alignment | LE, fixed-size records, 4-byte aligned, 16-byte header | both test suites decode the same bytes |
| protocol version + schema fingerprint | `fynkel_abi_version()`, `fynkel_schema_fingerprint()` checked at `FynkelNative.open` | Dart throws on mismatch |
| buffer ownership | inputs borrowed for the call; outputs world-owned until next mutating call | documented in header; Dart views via `asTypedList` (no copy) |
| count/length validation | header count × record size must equal body; nothing applied on failure | `protocol errors apply nothing` (Rust + Dart) |
| two command lanes | simulation → next `step()`; presentation → immediate | round-trip tests |
| temp id → handle mapping | `FYNKEL_EVENT_SPAWNED {temp, entity}` | round-trip tests |
| stale handles fail safely | generation check; slot reuse bumps generation; handle 0 never valid | Rust test (caught a real bug: first handle was 0) |
| status + diagnostics | `COMMANDS_REJECTED` + records `{command_index, code, entity, component, lane, message}` | both suites check index/code/message |
| panic boundary | `catch_unwind` → `PANIC`, world poisoned, later calls `POISONED` | Rust test |
| tick number | `FynkelStepResult.tick` | round-trip tests |

## Bandwidth (M3 Max, Dart JIT under `dart test`, release Rust)

1 000 entities subscribed; every tick submits 1 000 `SetTransform`
commands and reads all 1 000 snapshot records (600 ticks):

| Stage | per tick |
| --- | --- |
| Dart encode + submit (FFI) | 124 µs |
| `fynkel_world_step` (apply 1 000 commands, rebuild snapshot) | 22 µs |
| Dart read of 1 000 records (no allocation) | 6 µs |
| bytes in / out | 62.5 KiB / 46.9 KiB (3.7 MB/s + 2.8 MB/s at 60 Hz) |

Encoding dominates and is mostly the throwaway `Transform`/`List<double>`
objects in the v0 codec; the generated codecs write straight into the
`ByteData`. The native side is ~0.15 ms for 2 000 records per tick — far
under the tick budget; no per-property FFI anywhere.

## Carried forward

- Native-assets `hook/build.dart` (ADR-0015) replaces `DynamicLibrary.open`
  + `FYNKEL_NATIVE_LIB`; ffigen then switches to `ffi-native`.
- Generated codecs + fingerprint from Rust schemas (ADR-0012).
- Request IDs for explicit queries and resource requests (header families
  not yet present: query, resource/asset).
