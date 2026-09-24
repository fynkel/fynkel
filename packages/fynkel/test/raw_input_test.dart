// Raw keys and mouse buttons (docs/unity-parity.md § Tier 2, ADR-0024):
// `Input.getKey` / `getKeyDown` / `getKeyUp` and the mouse buttons with no
// action declared, the edges reaching one tick as an action's do (§5.1)
// and the frame in `update`, a focus reset, and the once-per-name warning
// for an action the project does not declare — over the real native
// library and the fixture project.

import 'dart:convert';
import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

/// Reads F (declared by no action) and the left mouse button every callback.
class KeyReader extends Behaviour {
  final List<String> log = [];

  String _f() => '${Input.getKey('F') ? 'H' : '-'}${Input.getKeyDown('F') ? 'D' : '-'}${Input.getKeyUp('F') ? 'U' : '-'}'
      '${Input.getMouseButtonDown(0) ? ' click' : ''}';

  @override
  void fixedUpdate(FixedUpdateContext context) => log.add('fixed ${_f()}');
  @override
  void update(UpdateContext context) => log.add('update ${_f()}');
}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-raw-input-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  (World, KeyReader) loaded() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 4);
    addTearDown(w.dispose);
    w.onLoadDiagnostics = (_) {};
    w.loadProject(fixture, cacheDir: cache);
    final r = w.attach(w.allEntities().first).addBehaviour(KeyReader());
    w.frame(1 / 60);
    r.log.clear();
    return (w, r);
  }

  test('a key no action binds: held, down once across catch-up ticks, the frame in update', () {
    final (w, r) = loaded();
    w.frame(0.05, input: const [InputEvent.down(InputDevice.keyboard, 'F'), InputEvent.down(InputDevice.mouse, 'left')]);
    expect(r.log, ['fixed HD- click', 'fixed H--', 'fixed H--', 'update HD- click']);
    r.log.clear();

    w.frame(1 / 60);
    expect(r.log, ['fixed H--', 'update H--'], reason: 'held, no edge');
    r.log.clear();

    // Too short to tick: the release waits for the next tick; update sees it now.
    w.frame(0.004, input: const [InputEvent.up(InputDevice.keyboard, 'F')]);
    expect(r.log, ['update --U']);
    r.log.clear();
    w.frame(1 / 60);
    expect(r.log, ['fixed --U', 'update ---']);
    r.log.clear();

    // A focus loss releases what was held.
    w.frame(1 / 60, input: const [InputEvent.down(InputDevice.keyboard, 'F')]);
    r.log.clear();
    w.frame(1 / 60, input: const [InputEvent.reset()]);
    expect(r.log, ['fixed --U', 'update --U']);
    expect(w.input.getMouseButton(0), isFalse);
    expect(w.input.anyKey, isFalse);
  });

  test('a key name the grammar does not know is an error, not a silent false', () {
    final (w, _) = loaded();
    expect(() => w.input.getKey('space'), throwsArgumentError);
    expect(() => w.input.getMouseButton(7), throwsArgumentError);
    expect(w.input.getKey('Space'), isFalse);
  });

  test('an undeclared action warns once per name in the Console, and still reads as nothing', () {
    final lines = <String>[];
    EditorChannel.debugUse(enabled: true, sink: lines.add);
    addTearDown(EditorChannel.debugUse);
    final (w, _) = loaded();
    expect(w.input.held('jupm'), isFalse);
    expect(w.input.pressed('jupm'), isFalse);
    expect(w.input.held('jump'), isFalse, reason: 'declared: no warning');
    final warnings = [
      for (final l in lines) jsonDecode(l) as Map<String, Object?>,
    ].where((r) => r['source'] == 'input').toList();
    expect(warnings, hasLength(1));
    expect(warnings.single['severity'], 'warning');
    expect(warnings.single['message'], contains("'jupm' is not declared"));
  });

  test('TestInput presses keys and mouse buttons by hand', () {
    final input = TestInput();
    final context = TestGameContext(input: input);
    input.pressKey('Space');
    input.pressKey('mouse:right');
    context.run(() {
      expect((Input.getKey('Space'), Input.getKeyDown('Space'), Input.getMouseButtonDown(1)), (true, true, true));
    });
    input.endTick();
    input.releaseKey('Space');
    context.run(() {
      expect((Input.getKey('Space'), Input.getKeyDown('Space'), Input.getKeyUp('Space')), (false, false, true));
      expect(Input.anyKey, isTrue, reason: 'the right button is still down');
    });
  });
}
