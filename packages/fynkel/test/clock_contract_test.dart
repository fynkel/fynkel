import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

class ClockHost extends Behaviour {
  int updates = 0;
  final fixedElapsed = <double>[];
  @override
  void fixedUpdate(FixedUpdateContext context) => fixedElapsed.add(Time.elapsed);
  @override
  void update(UpdateContext context) => updates++;
}

class StartWait extends Behaviour {
  double? started, ended;
  @override
  void start() => startCoroutine(wait());
  Iterable<Object?> wait() sync* {
    started = Time.elapsed;
    yield const WaitForSeconds(0.1);
    ended = Time.elapsed;
  }
}

void main() {
  late NativeLibrary library;
  setUpAll(() => library = NativeLibrary.open());
  tearDownAll(() => library.dispose());
  World fresh({double fixedDt = 0.01}) {
    final w = World.create(library: library, fixedDt: fixedDt, maxStepsPerFrame: 4);
    w.throwOnDiagnostics = false;
    addTearDown(w.dispose);
    return w;
  }

  test('catch-up drops simulation time while waits, invokes and tweens agree with gameplay elapsed', () {
    final w = fresh();
    final host = w.spawn().addBehaviour(ClockHost());
    w.frame(0.01);
    final start = w.context.time.elapsed;
    double? waited, invoked;
    double value = 0;
    Iterable<Object?> wait() sync* {
      yield const WaitForSeconds(0.1);
      waited = Time.elapsed;
    }

    host.startCoroutine(wait());
    host.invoke(() => invoked = Time.elapsed, 0.1);
    w.tween(0.0, 1.0, 1, (v) => value = v, ease: Ease.linear);
    w.frame(0); // Start scheduled work before measuring elapsed duration.
    host.fixedElapsed.clear();
    final plan = w.frame(2);
    expect(plan.steps, 4);
    expect(plan.gameplayDelta, 0.25);
    expect(plan.realDelta, 2);
    expect(plan.clampedSeconds, 1.75);
    expect(plan.droppedSeconds, greaterThan(0.19));
    expect(w.context.time.elapsed - start, closeTo(0.25, 1e-12));
    expect(host.fixedElapsed, everyElement(closeTo(start + 0.25, 1e-12)));
    expect(waited, closeTo(start + 0.25, 1e-12));
    expect(invoked, waited);
    expect(value, closeTo(0.25, 1e-12));
    expect(w.context.time.simulationElapsed, closeTo(0.05, 1e-6));
    expect(w.context.time.realElapsed, closeTo(2.01, 1e-12));
  });

  test('a callback-scheduled wait starts next frame and does not consume that frame delta', () {
    final w = fresh();
    final host = w.spawn().addBehaviour(StartWait());
    w.frame(0.1);
    expect(host.started, isNull);
    expect(host.ended, isNull);
    w.frame(0.05);
    expect(host.started, closeTo(0.15, 1e-12));
    expect(host.ended, isNull);
    w.frame(0.05);
    expect(host.ended, isNull);
    w.frame(0.05);
    expect(host.ended, closeTo(0.25, 1e-12));
  });

  test('paused frames advance real time; editor Step advances only simulation waits and tweens', () {
    final w = fresh();
    final host = w.spawn().addBehaviour(ClockHost());
    w.frame(0.01);
    final game = w.context.time.elapsed, real = w.context.time.realElapsed;
    final sim = w.context.time.simulationElapsed;
    bool gameplayDone = false, realDone = false, simulationDone = false;
    Iterable<Object?> wait(TimeClock clock, void Function() done) sync* {
      yield WaitForSeconds(0.02, clock: clock);
      done();
    }

    host.startCoroutine(wait(TimeClock.gameplay, () => gameplayDone = true));
    host.startCoroutine(wait(TimeClock.real, () => realDone = true));
    host.startCoroutine(wait(TimeClock.simulation, () => simulationDone = true));
    double gameplayTween = 0, simulationTween = 0, realTween = 0;
    w.tween(0.0, 1.0, 0.02, (v) => gameplayTween = v, ease: Ease.linear);
    w.tween(0.0, 1.0, 0.02, (v) => simulationTween = v, ease: Ease.linear, clock: TimeClock.simulation);
    w.tween(0.0, 1.0, 0.02, (v) => realTween = v, ease: Ease.linear, clock: TimeClock.real);
    w.frame(0);
    w.timeScale = 0;
    final paused = w.frame(0.1);
    expect(paused.steps, 0);
    expect(w.context.time.elapsed, game);
    expect(w.context.time.realElapsed, closeTo(real + 0.1, 1e-12));
    expect(realDone, isTrue);
    expect(realTween, 1);
    expect(gameplayDone, isFalse);
    expect(simulationDone, isFalse);
    final updates = host.updates, frame = w.context.time.frame;
    w.stepFrame();
    expect(simulationTween, 0, reason: 'simulation-clock work starts at the first simulation phase');
    w.stepFrame();
    expect(simulationTween, closeTo(0.5, 1e-6));
    w.stepFrame();
    expect(simulationDone, isTrue);
    expect(simulationTween, closeTo(1, 1e-6));
    expect(w.context.time.simulationElapsed, closeTo(sim + 0.03, 1e-6));
    expect(w.context.time.elapsed, game);
    expect(w.context.time.realElapsed, closeTo(real + 0.1, 1e-12));
    expect(w.context.time.frame, frame);
    expect(host.updates, updates);
    expect(gameplayDone, isFalse);
    expect(gameplayTween, 0);
    w.timeScale = 1;
    w.frame(0.02);
    expect(gameplayDone, isTrue);
    expect(gameplayTween, closeTo(1, 1e-6));
  });

  test('scale is sampled at begin; zero-tick and invalid frames use the same timeline', () {
    final w = fresh(fixedDt: 0.1);
    w.timeScale = 0.5;
    final plan = w.frame(0.02);
    expect(plan.steps, 0);
    expect(w.context.time.elapsed, 0.01);
    expect(w.context.time.simulationElapsed, 0);
    expect(w.context.time.realElapsed, 0.02);
    for (final dt in [-1.0, double.nan, double.infinity]) {
      final invalid = w.frame(dt);
      expect(invalid.realDelta, 0);
      expect(invalid.gameplayDelta, 0);
      expect(w.context.time.unscaledDelta, 0);
    }
    expect(w.context.time.elapsed, 0.01);
    w.timeScale = 2;
    final p = w.frame(0.1, fixedUpdate: (_) => w.timeScale = 0);
    expect(p.gameplayDelta, 0.2);
    expect(w.context.time.delta, 0.2);
    expect(w.frame(0.1).steps, 0);
    expect(w.context.time.delta, 0);
    expect(() => w.timeScale = double.nan, throwsArgumentError);
    expect(() => w.timeScale = double.infinity, throwsArgumentError);
  });

  test('Step consumes pending input edges once without advancing host clocks', () {
    final w = fresh();
    w.timeScale = 0;
    w.frame(0.1, input: const [InputEvent.down(InputDevice.keyboard, 'F')]);
    final pressed = <bool>[];
    for (var i = 0; i < 2; i++) {
      w.stepFrame(fixedUpdate: (_) => pressed.add(w.context.run(() => Input.getKeyDown('F'))));
    }
    expect(pressed, [true, false]);
    expect(w.context.time.elapsed, 0);
    expect(w.context.time.realElapsed, 0.1);
  });

  test('scene replacement preserves world timelines', () {
    final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
    final cache = Directory.systemTemp.createTempSync('fynkel-clock-scene-');
    addTearDown(() => cache.deleteSync(recursive: true));
    cookProject(fixture, cacheDir: cache.path);
    final w = fresh();
    w.onLoadDiagnostics = (_) {};
    w.frame(0.1);
    final game = w.context.time.elapsed, real = w.context.time.realElapsed;
    w.loadProject(fixture, cacheDir: cache.path);
    w.frame(0);
    expect(w.context.time.elapsed, game);
    expect(w.context.time.realElapsed, real);
  });

  test('test contexts drive gameplay frames independently of simulation ticks', () {
    final time = TestTime(fixedDelta: 0.01, timeScale: 0.5);
    time.advanceFrame(0.1);
    time.advanceTick();
    expect(time.elapsed, 0.05);
    expect(time.realElapsed, 0.1);
    expect(time.simulationElapsed, 0.01);
    time.timeScale = 0;
    time.advanceFrame(1);
    time.advanceTick();
    expect(time.elapsed, 0.05);
    expect(time.realElapsed, 1.1);
    expect(time.simulationElapsed, 0.02);
  });
}
