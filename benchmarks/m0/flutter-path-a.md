# M0 — Flutter macOS host, Path A (IOSurface ring → `Texture`)

Benchmark profile: `mac-m3max-2026` (see `docs/toolchain.md`). Compare against
`standalone-baseline.md`.

| Field | Value |
| --- | --- |
| Date | 2026-09-19 |
| Host | `hosts/flutter_macos` (Flutter 3.47.2 / Impeller) + `crates/fynkel_m0` (wgpu 30.0.1, Metal) |
| Scene | same cube as baseline (`crates/fynkel_m0_scene`) |
| Viewport | 1600×1200 physical (800×600 logical window, Retina) |
| Ring | 4 IOSurface-backed `MTLTexture`s, BGRA8, same `MTLDevice` as Impeller |
| Loop | Flutter `Ticker` → `fynkel_m0_render()` once per Flutter frame (host owns the loop, ADR-0007) |
| Present | `copyPixelBuffer` returns the newest GPU-complete buffer; **no CPU copy** |
| Overlays | stats text, Material `FilledButton`, `CircularProgressIndicator` on top of the `Texture` |
| Build | `flutter build macos --release --dart-define=FYNKEL_M0_DYLIB=…/target/release/libfynkel_m0.dylib` |
| Capture | run 10+ s, read `[m0]` lines from stdout (one per second) |

## Result — steady state (per 1 s window, 120 Hz display)

| Metric | async mode (mode 0, superseded by M0-07) | blocking mode |
| --- | --- | --- |
| Dart tick interval p50 / p95 / p99 (ms) | 8.33 / 8.34 / 8.34 | 8.33 / 8.34 / 8.34 |
| `render()` FFI call p50 / p95 / p99 (ms) | 0.06 / 0.13 / 0.17 | 0.80 / 0.90 / 0.98 |
| Rust CPU encode+submit p50 / p95 (ms) | 0.055 / 0.12 | 0.80 / 0.89 (includes GPU wait) |
| submit → GPU complete p50 / p95 (ms) | 8.32 / 8.38 (see note) | 0.75 / 0.84 |
| frames rendered / presented / new-frame presents | 120 / 120 / 120 | 120 / 120 / 120 |
| dropped (ring full) / stale presents | 0 / 0 | 0 / 0 |

Baseline (standalone) for the same metric: CPU encode+submit **0.056 / 0.137 ms** p50/p95,
presented interval **8.33 / 8.67 ms** p50/p95.

## Interpretation

- **Path A works under Impeller.** The wgpu-rendered cube shows inside a
  `Texture` widget with Flutter widgets composited on top, vsync-locked at
  120 Hz, one new frame per display refresh, zero drops, zero stale presents,
  zero pixel copies.
- **CPU overhead vs baseline is within noise** in async mode (0.12 ms vs
  0.137 ms p95). The provisional 1.5 ms p95 composition budget (ADR-0002) is
  not threatened by the interop itself; the remaining unknown is Impeller's own
  cost to sample the external texture, which is measured in M0-07 with
  Instruments (Metal System Trace) rather than from inside the process.
- **Real GPU time for the frame is ~0.75 ms** (blocking mode measures it
  directly by waiting).
- **Async-mode finding (latency):** `wgpu::Queue::on_submitted_work_done`
  callbacks only fire when the device is next polled/submitted, so in this
  mode the "frame ready" signal reaches Flutter ~8.3 ms later. Resolved in
  M0-07 (`latency-and-sync.md`): a Metal completion handler signals at
  ~1.15 ms, and a bounded consumer-side wait gives presentation lag 0.
- Startup: first window shows one stale present and a 16.7 ms p99 tick (the
  ring warming up). Not a concern.

## Open for M0-06/07

- Resize / DPI change / minimise / background / dispose (M0-06). The ring is
  currently created once at the first layout.
- End-to-end latency (input → photon) and Impeller composition cost via
  Instruments (M0-07). Fill the table in ADR-0002 after that.
