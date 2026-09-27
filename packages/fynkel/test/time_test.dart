// Time as a Unity script sees it (docs/foundation.md Step 1): inside the
// fixed phase — fixedUpdate, afterPhysics and the host's fixedUpdate
// callback — `Time.delta` is the fixed step, as Unity's `Time.deltaTime`
// is `fixedDeltaTime` inside FixedUpdate; in update and lateUpdate it is
// the frame's. So `x += v * Time.delta` moves the same distance per second
// at 30, 60 or 120 frames a second, whichever callback it is in.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// Records `Time.delta` in every callback it gets.
class DeltaProbe extends Behaviour {
  final List<(String, double)> seen = [];

  @override
  bool get wantsAfterPhysics => true;

  @override
  void fixedUpdate(FixedUpdateContext context) => seen.add(('fixedUpdate', Time.delta));
  @override
  void afterPhysics(AfterPhysicsContext context) => seen.add(('afterPhysics', Time.delta));
  @override
  void update(UpdateContext context) => seen.add(('update', Time.delta));
  @override
  void lateUpdate(UpdateContext context) => seen.add(('lateUpdate', Time.delta));
}

/// Moves `v * Time.delta` along X each fixed tick, as a Unity port would.
class Mover extends Behaviour {
  double x = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) => x += 3 * Time.delta;
}

/// A pause menu as a Unity game writes one: `Time.timeScale = 0` on the
/// pause action, back to 1 on the next — read in `update`, which must keep
/// running while the game is paused.
class PauseMenu extends Behaviour {
  bool toggle = false;
  int updates = 0, fixed = 0;
  final List<(double, double)> deltas = []; // (Time.delta, Time.unscaledDelta)

  @override
  void fixedUpdate(FixedUpdateContext context) => fixed++;

  @override
  void update(UpdateContext context) {
    updates++;
    deltas.add((Time.delta, Time.unscaledDelta));
    if (toggle) {
      toggle = false;
      Time.timeScale = Time.timeScale == 0 ? 1 : 0;
    }
  }
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World world() => World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;

  test('Time.delta is the fixed step in the fixed phase and the frame dt in update', () {
    final w = world();
    addTearDown(w.dispose);
    final probe = w.spawn(name: 'probe').addBehaviour(DeltaProbe());
    w.frame(1 / 60); // the spawn resolves; the behaviour starts in update
    probe.seen.clear();

    // A 30 fps frame: two ticks, then one update.
    final host = <double>[];
    w.frame(1 / 30, fixedUpdate: (_) => host.add(w.context.time.delta));
    expect(probe.seen, [
      ('fixedUpdate', 1 / 60),
      ('afterPhysics', 1 / 60),
      ('fixedUpdate', 1 / 60),
      ('afterPhysics', 1 / 60),
      ('update', 1 / 30),
      ('lateUpdate', 1 / 30),
    ]);
    expect(host, [1 / 60, 1 / 60], reason: "the host's fixedUpdate callback is in the fixed phase too");

    // Between frames (a host reading it from a HUD) it is the frame's again.
    expect(w.context.run(() => Time.delta), 1 / 30);
  });

  test('a fixedUpdate move covers the same distance per second at 30, 60 and 120 fps', () {
    double distanceAfterOneSecond(int fps) {
      final w = world();
      addTearDown(w.dispose);
      final mover = w.spawn(name: 'mover').addBehaviour(Mover());
      w.frame(1 / 60); // resolve; no tick reaches the behaviour yet
      final start = mover.x;
      for (var i = 0; i < fps; i++) {
        w.frame(1 / fps);
      }
      return mover.x - start;
    }

    for (final fps in [30, 60, 120]) {
      expect(distanceAfterOneSecond(fps), closeTo(3, 3 / 60 + 1e-9), reason: '$fps fps');
    }
  });

  test('Time.timeScale slows the ticks and scales update; 0 pauses them and update still runs (foundation 1.3)', () {
    final w = world();
    addTearDown(w.dispose);
    final menu = w.spawn(name: 'menu').addBehaviour(PauseMenu());
    final mover = w.spawn(name: 'mover').addBehaviour(Mover());
    w.frame(1 / 60);
    expect(w.context.run(() => Time.timeScale), 1);

    // Half speed: 60 frames of a second make 30 ticks, and 1.5 m.
    w.context.run(() => Time.timeScale = 0.5);
    var ticks = 0;
    final start = mover.x;
    menu.deltas.clear();
    for (var i = 0; i < 60; i++) {
      ticks += w.frame(1 / 60).steps;
    }
    expect(ticks, closeTo(30, 1));
    expect(mover.x - start, closeTo(1.5, 3 / 60 + 1e-9));
    expect(menu.deltas.last.$1, closeTo(1 / 120, 1e-9), reason: 'Time.delta in update is scaled');
    expect(menu.deltas.last.$2, closeTo(1 / 60, 1e-9), reason: 'Time.unscaledDelta is the frame');

    // The pause action: timeScale 0 — no tick, update runs with delta 0.
    w.context.run(() => Time.timeScale = 1);
    menu.toggle = true;
    w.frame(1 / 60);
    final fixed = menu.fixed, updates = menu.updates, tick = w.tick;
    for (var i = 0; i < 30; i++) {
      w.frame(1 / 60);
    }
    expect(w.tick, tick, reason: 'no tick while paused');
    expect(menu.fixed, fixed);
    expect(menu.updates, updates + 30, reason: 'update runs while paused');
    expect(menu.deltas.last, (0.0, 1 / 60));

    // …so the same action resumes.
    menu.toggle = true;
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(w.tick, greaterThan(tick));

    expect(() => w.context.run(() => Time.timeScale = -1), throwsArgumentError);
  });
}

