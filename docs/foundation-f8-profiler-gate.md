# F8 execution profiling and startup gate — 2026-10-04

No renderer optimization is retained in this batch. The profiler workflow now
separates attachment from the bounded workload. F8 remains open.

## Valid new baseline

One new eight-second Metal System Trace successfully attached to the exact PID
of the retained medium-quality frozen dense-5000 app. The analyzer matched 170 AO
fragment encoders, 169 main-scene fragment encoders and 164 blur compute encoders.
Their median active intervals were 5.858, 5.284 and 3.749 ms respectively. Channels
can overlap: these figures are not additive, unprofiled frame time or display FPS.
The result confirms that screen-space shading/filtering remains a substantial
GPU cost. It does not identify memory stalls or individual expensive instructions.

The intended AO-only/contact-only comparison could not be completed. Six other
attachment attempts returned `Cannot find process for provided pid`; all are
marked failed and excluded. The later attempts reproduce the error while the
app waits idle before loading the game. A diagnostic two-second delay also failed.
Thus the original launch/recording race is not established as the cause of these
failures. No automatic retry or fixed startup delay is shipped. The complete
successful trace stays in `target/f8-metal-effects-both-retry/metal.trace`; exported
tables are archived with the results.

## Retained diagnostic change

Previously the scene started before Instruments attached. Metal requests now
include `awaitProfiler`; the app opens its capture log and waits before loading
the game. The runner waits up to 30 seconds for the live recorder's ready message,
then transfers `capture-go-<captureId>.json`. The app validates and consumes that
signal and logs `profiler-ready`. Its idle wait is independently capped at 45
seconds. Recorder exit or readiness timeout fails the capture and cleanup stops
the exact app PID and recorder child. Normal captures do not wait for a signal.
The scene still has its original eight-second deadline; this does not extend
GPU load to accommodate attachment delays. Missing acknowledgement is rejected.

This is a startup/safety improvement, not a fix for Instruments process discovery.
The new gated workflow has not yet produced a successful Metal recording on this
phone; the successful baseline preceded it. Its positive app handshake was
verified independently on the phone: a matching signal released the scene,
statistics were recorded, the eight-second deadline completed and its exact PID
was stopped. That verification is explicitly labelled a manual handshake check,
not a profiler capture or performance result. Failed gated recordings verify that
the runner withholds the start signal when attachment fails.

19 Python checks pass, covering delayed readiness, recorder exit, timeout,
withholding release after failure, exact-PID cleanup and signal identity. Dart
analysis and the iOS release build pass. The final phone check used medium quality
and left the app stopped. No shader, sample-count or quality-default changes were
made. All earlier retained engine optimizations remain.

Next: resolve Instruments' PID discovery failure, then finish the isolated
AO/contact execution measurements. The current evidence does not justify another
arithmetic rewrite or a claim that the dense scene meets 60/120 FPS.

[Results](../benchmarks/foundation/results/2026-10-04-f8-profiler-gate.json)
and [raw exports, failed attempts and handshake verification](../benchmarks/foundation/results/2026-10-04-f8-profiler-gate-raw.tar.gz).

Follow-up: [USB reconnection and three successful gated recordings](foundation-f8-metal-effects.md)
resolve the attachment blocker for the recorded session and validate the positive
handshake end to end. The earlier failures above remain historical evidence.
