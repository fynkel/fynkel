import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

void main() {
  late Directory temp;
  late String cache;
  late NativeLibrary lib;
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  setUpAll(() {
    temp = Directory.systemTemp.createTempSync('fynkel-input-bindings-');
    cache = '${temp.path}/cache';
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    lib.dispose();
    temp.deleteSync(recursive: true);
  });
  World fresh({bool loaded = true}) {
    final world = World.create(library: lib);
    addTearDown(world.dispose);
    world.onLoadDiagnostics = (_) {};
    if (loaded) expect(world.loadProject(fixture, cacheDir: cache).ok, isTrue);
    return world;
  }

  void press(World world, String key) =>
      world.frame(0, input: [const InputEvent.reset(), InputEvent.down(InputDevice.keyboard, key)]);

  test('rebinds publish on commit while reads keep their current phase sample', () {
    final world = fresh();
    world.frame(
      0,
      input: const [InputEvent.down(InputDevice.keyboard, 'Space')],
      update: () {
        expect(world.input.state('jump')!.held, isTrue);
        final report = world.bindActions({
          'jump': ['keyboard:J'],
        });
        expect(report.ok, isTrue);
        expect(report.inputActions.singleWhere((a) => a.name == 'jump').bindings, ['keyboard:J']);
        expect(
          world.input.state('jump')!.held,
          isTrue,
          reason: 'rebinding does not refresh this phase sample',
        );
      },
    );
    expect(world.input.state('jump')!.held, isTrue);
    expect(world.tick, 0);
    press(world, 'Space');
    expect(world.input.state('jump')!.held, isFalse);
    press(world, 'J');
    expect(world.input.state('jump')!.held, isTrue);
  });

  test('aborted rebinds do not leak into action compilation or later scene overlays', () {
    final world = fresh();
    final object = world.edit(() => world.spawn(name: 'original'));
    expect(
      () => world.edit(() {
        object.name = 'discarded';
        world.bindActions({
          'jump': ['keyboard:J'],
        });
        throw StateError('abort bindings');
      }),
      throwsStateError,
    );
    expect(object.name, 'original');
    press(world, 'Space');
    expect(world.input.state('jump')!.held, isTrue);
    final report = world.loadScene('c0000000-0000-4000-8000-000000000034');
    final action = report.inputActions.singleWhere((a) => a.name == 'jump');
    expect(action.rebound, isFalse);
    expect(action.bindings, contains('keyboard:Space'));
    expect(
      () => world.frame(
        0,
        update: () {
          world.bindActions({
            'jump': ['keyboard:J'],
          });
          throw StateError('abort callback');
        },
      ),
      throwsStateError,
    );
    press(world, 'J');
    expect(world.input.state('jump')!.held, isFalse);
  });

  test('successive staged maps are copied immediately and last write wins across scene loads', () {
    final world = fresh();
    final keys = ['keyboard:J'];
    final rebinds = {'jump': keys};
    world.edit(() {
      final first = world.bindActions({
        'jump': ['keyboard:K'],
      });
      world.bindActions(rebinds);
      keys[0] = 'keyboard:L';
      rebinds.clear();
      expect(first.inputActions.singleWhere((a) => a.name == 'jump').bindings, ['keyboard:K']);
    });
    press(world, 'J');
    expect(world.input.state('jump')!.held, isTrue);
    final report = world.loadScene('c0000000-0000-4000-8000-000000000034');
    expect(report.inputActions.singleWhere((a) => a.name == 'jump').bindings, ['keyboard:J']);
    world.bindActions(const {});
    press(world, 'J');
    expect(world.input.state('jump')!.held, isFalse);
    press(world, 'Space');
    expect(world.input.state('jump')!.held, isTrue);
  });

  test('bad binding syntax is a report entry while other bindings still commit', () {
    final world = fresh();
    final report = world.edit(
      () => world.bindActions({
        'jump': ['unsupported:bad', 'keyboard:J'],
        'missing-action': ['keyboard:K'],
      }),
    );
    expect(report.ok, isTrue);
    expect(report.bindingErrors, hasLength(1));
    expect(report.bindingErrors.single.action, 'jump');
    expect(report.bindingErrors.single.binding, 'unsupported:bad');
    expect(report.bindingErrors.single.message, isNotEmpty);
    press(world, 'J');
    expect(world.input.state('jump')!.held, isTrue);
    expect(world.input.state('missing-action'), isNull);
  });

  test('missing project is sticky and earlier object and mixer writes roll back', () {
    final world = fresh(loaded: false);
    final object = world.edit(() => world.spawn(name: 'original'));
    expect(
      () => world.edit(() {
        object.name = 'discarded';
        world.audio.setBus(AudioBus.master, 0.25);
        try {
          world.bindActions({
            'jump': ['keyboard:J'],
          });
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(object.name, 'original');
    expect(world.audio.busVolume(AudioBus.master), 1);
    expect(() => world.bindActions(const {}), throwsStateError);
  });

  test('ordinary rebinding adopts queued objects without stepping simulation', () {
    final world = fresh();
    final object = world.spawn(name: 'queued');
    world.bindActions({
      'jump': ['keyboard:J'],
    });
    expect(object.isPending, isFalse);
    expect(world.find('queued'), same(object));
    expect(world.tick, 0);
  });
}
