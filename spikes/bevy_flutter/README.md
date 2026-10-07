# Spike: Bevy embedded in Flutter (2026-10-07)

**Question.** Can Bevy render inside a Flutter app, driven from Dart, well
enough to be the core of a code-first Flutter game framework?

Nothing here is product code. It is thrown away or rewritten after the
decision. It does not touch the Fynkel workspace.

## Design (the same path Fynkel uses, ADR-0002 Path A)

- Bevy 0.19.1 (wgpu 29, Metal), no window, no winit. Its camera renders
  into a `ManualTextureView` that is one slot of a ring of IOSurfaces.
- Swift wraps the IOSurfaces as `CVPixelBuffer`s behind a `FlutterTexture`;
  Flutter composites a `Texture` widget. No CPU copy per frame.
- A Rust worker thread owns the Bevy `App` and runs `app.update()` when Dart
  ticks it (one FFI call per frame, carrying all cube transforms). Dart's UI
  thread never waits for Bevy.
- A slot is shown only after its GPU work completed (Metal completion
  handler on a command buffer submitted behind the frame).

## Scene

Ground, sun with shadows, ambient light, **1 000 cubes whose transforms are
computed in Dart every frame** and sent in one call, **one skinned character
playing `walk`** (`character.glb`), and a **Flutter HUD** over the view (fps,
GPU/CPU ms, buttons).

## Pass / fail (fixed before any number exists)

| # | Bar | Platform |
|---|---|---|
| P1 | Bevy's picture shows in a Flutter `Texture`, HUD composited on top, no per-frame CPU readback | macOS, iPhone |
| P2 | The 1 000 Dart-driven cubes and the walking character render correctly | macOS, iPhone |
| P3 | Dart side of the per-frame call (filling + sending 1 000 transforms) < 0.5 ms | macOS |
| P4 | ≥ 115 fresh frames/s at a 1600×1200 window on the 120 Hz Mac | macOS |
| P5 | Window resize: no crash, correct picture after resize | macOS |
| P6 | Dispose and recreate the view 10×: no crash | macOS |
| P7 | iPhone 16 Pro Max: ≥ 60 fps native resolution, and ≥ 110 at 0.75 scale | iPhone |
| P8 | Release app size of the Bevy part, reported (no bar: decided after) | iPhone |

**Verdict:** P1–P2 failing means Bevy cannot be embedded this way → keep the
Fynkel runtime. P3–P7 failing by a wide margin means it embeds but costs too
much → investigate before deciding. All passing → Bevy is a viable core.

## Results (2026-10-07, MacBook Pro 120 Hz + iPhone 16 Pro Max, release)

Raw logs in `results/`; screenshot `results/mac_shot.png`.

| # | Result | Numbers |
|---|---|---|
| P1 | ✅ | IOSurface ring → `CVPixelBuffer` → `Texture`; Flutter HUD and button drawn over it; no readback |
| P2 | ✅ (Mac seen) | Cubes, shadows, walking character correct, colours right (sRGB). iPhone frames arrive; picture not yet looked at by a person |
| P3 | ✅ | Dart fill + `bf_frame`: p50 0.020 ms, p95 0.035 ms (25× under the bar) |
| P4 | ✅ | 1600×1200: fresh fps median 120.0 (min 118) over 10 s, in both present modes; Bevy update p50 2.7 ms, GPU p50 2.0 ms; 0 coalesced |
| P5 | ✅ | 6 view-size changes (800×600 … 1600×1200): native resize 0.33–0.45 ms Mac, 1.1–4.0 ms iPhone; frames resume each time. Driven by resizing the view in code; dragging the window edge by hand not done |
| P6 | ✅ | 10× destroy + create: 46–47 ms each (Mac), 45–53 ms (iPhone), no crash; RSS Mac +13 MB over 10 cycles (small leak to look at), iPhone flat 191–208 MB; one unexplained stall, see below |
| P7 | ✅ (with the frame queue) | Native 1320×2868: **120.0** fresh fps (min 120.0 over 10 s); 0.75: **120.0** (min 120.0). Without the queue (newest-frame only): native median 68, 0.75 median 106 |
| P8 | reported | Runner binary 115 MB = 57 MB code + 58 MB symbols (64 MB after `strip -x`); same as standalone Bevy (CmpBevy 114 MB). Flutter adds ≈ 13 MB. Whole .app 128 MB unstripped |

**Where the iPhone frames went, and the fix.** Bevy renders all 120
frames/s on the phone (update 2.6 ms, GPU 5.2–5.6 ms; the same with 100
cubes, so it is a fixed per-frame cost, likely the shadow cascades). In the
first runs only 68 (native) / 106 (0.75) reached the screen: update + GPU
≈ 8 ms puts each frame's completion right at the next vsync, so often two
finished between two rasters and the presenter, which always jumped to the
newest, threw one away. The ring now **queues** finished frames (oldest
first, at most 2 waiting, bit 8 asks Flutter for another raster — the
same idea as Fynkel's F8 frame queue). Result: 120.0 at both scales. Cost:
when a frame finishes late it is shown one vsync later instead of being
dropped (at most +2 frames of latency, ≈ 17 ms; not measured).
`--dart-define=SPIKE_QUEUE=false` restores the old mode.

**One unexplained stall (Mac, queue mode).** In one run the 4th recreate
showed no frames for 10 s: the Dart timer kept running, but Flutter stopped
ticking frames (the ticker fired 0 times a second), and the app then shut
down cleanly, so no deadlock was held. Three more full runs (30 recreates)
did not reproduce it (`results/mac_queue_stall.log`). Best guess: macOS
paused the window's display link (window hidden/occluded); not proven.
In those runs the create time also rose from 46 ms to 100–170 ms after the
3rd recreate, and RSS grows ≈ 3 MB over 10 recreates — both worth a look in
real code, neither a blocker.

**Cold start.** The very first create on the Mac took 5.7 s and the first
picture ~7 s (pipeline/shader compile with a cold Metal cache); every later
create was 46 ms. iPhone: 68 ms, first picture < 1 s.

**Verdict.** **Bevy embeds in Flutter and works well.** All bars pass
(P7 after the frame-queue fix): it draws through the same IOSurface path
Fynkel uses, driven from Dart with a 0.02 ms per-frame cost, at 120 fps
on the Mac and at native resolution on the iPhone 16 Pro Max, and it
survives resize and repeated recreate. Open: the one stall above, the
create-time/RSS growth over recreates, the cold first start on the Mac
(shader compile), and a person looking at the iPhone picture.

## Run it

```sh
cd spikes/bevy_flutter/native && cargo build --release
cd ../app && flutter build macos --release --dart-define=SPIKE_AUTO=true
build/macos/Build/Products/Release/bevy_flutter_app.app/Contents/MacOS/bevy_flutter_app
```

iPhone: `cargo build --release --target aarch64-apple-ios --lib` in
`native/`, then `flutter build ios --release --dart-define=SPIKE_AUTO=true`
(optionally `--dart-define=SPIKE_SCALE=0.75`, `SPIKE_CUBES=100`), install
and launch with `xcrun devicectl`, and fetch `tmp/spike.log` from the app
container with `xcrun devicectl device copy from`. Without `SPIKE_AUTO`
the app runs interactively with a Recreate button.
