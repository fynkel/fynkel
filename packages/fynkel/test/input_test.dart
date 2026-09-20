// The input contract (§5.1, docs/input.md) from the Dart side: the
// project's actions bind on load, a captured frame drives them, the edges
// reach exactly one tick (a press across three catch-up ticks), update
// sees the frame's raw edges, a recording replays into a world, and
// TestInput drives the same facade without a world.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

/// Reads the actions at every callback.
class Reader extends Behaviour {
  final List<String> log = [];

  @override
  bool get wantsAfterPhysics => true;

  String _jump() {
    final j = Input.state('jump')!;
    return '${j.held ? 'H' : '-'}${j.pressed ? 'P' : '-'}${j.released ? 'R' : '-'}';
  }

  @override
  void fixedUpdate(FixedUpdateContext context) => log.add('fixed@${context.tick} ${_jump()} move=${Input.action2D('move')}');
  @override
  void afterPhysics(AfterPhysicsContext context) => log.add('after@${context.tick} ${_jump()}');
  @override
  void update(UpdateContext context) => log.add('update ${_jump()}');
}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-input-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World loaded() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 4);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    w.loadProject(fixture, cacheDir: cache);
    return w;
  }

  test('the project\'s actions bind on load and read as nothing before', () {
    final w = World.create(library: lib);
    addTearDown(w.dispose);
    w.frame(1 / 60);
    expect(w.input.actions, isEmpty);
    expect(w.input.held('jump'), isFalse);
    expect(w.input.action2D('move'), Vec2.zero);
    expect(w.input.state('jump'), isNull);

    final l = World.create(library: lib);
    addTearDown(l.dispose);
    l.onLoadDiagnostics = (_) {};
    final report = l.loadProject(fixture, cacheDir: cache);
    expect([for (final a in report.inputActions) '${a.name}:${a.type}'], ['move:axis2d', 'jump:button', 'look:axis2d', 'pause:button']);
    expect(report.inputActions[0].bindings, ['keyboard:WASD', 'gamepad:leftStick', 'touch:overlay/stick']);
    expect(report.inputActions[0].overlayControls, ['stick']);
    l.frame(1 / 60);
    expect([for (final a in l.input.actions) '${a.name}:${a.kind.name}'], ['move:axis2d', 'jump:button', 'look:axis2d', 'pause:button']);
    expect(l.input.state('jump'), isNotNull);
  });

  test('a press is seen by exactly one of three catch-up ticks; update sees the frame', () {
    final w = loaded();
    final go = w.attach(w.allEntities().first);
    final r = go.addBehaviour(Reader());
    w.frame(1 / 60); // creates the behaviour
    r.log.clear();

    // 50 ms: three ticks. Space and W went down during the frame.
    final plan = w.frame(0.05, input: const [
      InputEvent.down(InputDevice.keyboard, 'Space'),
      InputEvent.down(InputDevice.keyboard, 'W'),
    ]);
    expect(plan.steps, 3);
    expect(r.log, [
      'fixed@1 HP- move=Vec2(0.0, 1.0)',
      'after@1 HP-',
      'fixed@2 H-- move=Vec2(0.0, 1.0)',
      'after@2 H--',
      'fixed@3 H-- move=Vec2(0.0, 1.0)',
      'after@3 H--',
      'update HP-',
    ]);
    r.log.clear();

    // Release: one tick sees released; update sees the frame's release.
    w.frame(1 / 60, input: const [
      InputEvent.up(InputDevice.keyboard, 'Space'),
      InputEvent.up(InputDevice.keyboard, 'W'),
      InputEvent.axis(InputDevice.gamepad, 'leftStick', -0.5, 0.25),
    ]);
    expect(r.log, ['fixed@4 --R move=Vec2(-0.5, 0.25)', 'after@4 --R', 'update --R']);
    r.log.clear();

    // A frame too short to tick carries its edge to the next tick.
    expect(w.frame(0.004, input: const [InputEvent.down(InputDevice.touch, 'overlay/jump')]).steps, 0);
    expect(r.log, ['update HP-']);
    r.log.clear();
    expect(w.frame(0.014).steps, 1);
    expect(r.log, ['fixed@5 HP- move=Vec2(-0.5, 0.25)', 'after@5 HP-', 'update H--']);
    r.log.clear();

    // Focus lost: everything up, with edges.
    w.frame(1 / 60, input: const [InputEvent.reset()]);
    expect(r.log, ['fixed@6 --R move=Vec2(0.0, 0.0)', 'after@6 --R', 'update --R']);
    expect(w.isPoisoned, isFalse);
  });

  test('a recording replays frame by frame (the fixture walk)', () {
    final w = loaded();
    final recording = loadInputRecording('$fixture/test/walk.input.json');
    expect(recording.frames, hasLength(10));
    expect(recording.eventCount, 9);
    var jumps = 0, moving = 0;
    final plans = recording.replay(w, fixedUpdate: (_) {
      if (w.input.pressed('jump')) jumps++;
      if (w.input.held('move')) moving++;
    });
    expect(plans, hasLength(10));
    expect(plans.fold(0, (n, p) => n + p.steps), 12);
    expect(jumps, 2, reason: 'Space across the 50 ms frame, then the overlay');
    expect(moving, 10);
    expect(w.input.held('move'), isFalse);
    // Round trip.
    expect(InputRecording.parse(recording.encode()).toJson(), recording.toJson());
    expect(() => InputRecording.parse('{"fynkel":"x","formatVersion":1,"frames":[]}'), throwsFormatException);
    expect(() => InputRecording.parse('{"fynkel":"input-recording","formatVersion":1,"frames":[{"dt":-1}]}'),
        throwsFormatException);
  });

  test('a recorder captures what the frames were given', () {
    final rec = InputRecorder();
    rec.frame(1 / 60, const [InputEvent.down(InputDevice.keyboard, 'W')]);
    rec.frame(1 / 60, const []);
    rec.frame(0.02, const [InputEvent.move(InputDevice.mouse, 'delta', 3, -2), InputEvent.reset()]);
    final json = rec.recording.toJson();
    expect(json['fynkel'], 'input-recording');
    expect((json['frames'] as List)[0], {
      'dt': 1 / 60,
      'events': [
        {'device': 'keyboard', 'control': 'W', 'phase': 'down', 'x': 1.0}
      ]
    });
    expect((json['frames'] as List)[1], {'dt': 1 / 60});
    expect(((json['frames'] as List)[2] as Map)['events'], [
      {'device': 'mouse', 'control': 'delta', 'phase': 'move', 'x': 3.0, 'y': -2.0},
      {'phase': 'reset'},
    ]);
    expect(InputRecording.fromJson(json).frames[2].events[1], const InputEvent.reset());
  });

  test('TestInput drives the facade without a world', () {
    final ctx = TestGameContext(input: TestInput(actions: const [('jump', ActionKind.button), ('move', ActionKind.axis2d)]));
    ctx.run(() {
      expect(Input.pressed('jump'), isFalse);
      ctx.input.press('jump');
      expect(Input.pressed('jump'), isTrue);
      expect(Input.held('jump'), isTrue);
      expect(Input.axis('jump'), 1);
      ctx.input.press('jump'); // still down: no second edge
      ctx.input.endTick();
      expect(Input.pressed('jump'), isFalse);
      expect(Input.held('jump'), isTrue);
      ctx.input.setAxis('move', 0, 1);
      expect(Input.action2D('move'), const Vec2(0, 1));
      expect(Input.held('move'), isTrue);
      expect(Input.pressed('move'), isFalse, reason: 'axis actions have no edges');
      ctx.input.reset();
      expect(Input.released('jump'), isTrue);
      expect(Input.held('move'), isFalse);
      expect(Input.action2D('move'), Vec2.zero);
      expect(Input.held('nope'), isFalse, reason: 'an undeclared action reads as nothing');
      expect(() => ctx.input.press('nope'), throwsStateError);
    });
    expect(() => Input.held('jump'), throwsStateError, reason: 'no current context');
  });
}
