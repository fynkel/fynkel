import 'dart:convert';
import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

import 'controller_test.dart' as fixture;

const alternate = AssetRef('c0000000-0000-4000-8000-000000000053');
const changedKind = AssetRef('c0000000-0000-4000-8000-000000000054');

void main() {
  late Directory temp;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    temp = Directory.systemTemp.createTempSync('fynkel-controller-authority-');
    final project = Directory('${temp.path}/project');
    fixture.copyTree(Directory('../../crates/fynkel_document/fixtures/project').absolute, project);
    File('${project.path}/assets/rigs/rig.controller.json').writeAsStringSync(fixture.controllerJson);
    for (final (id, kind, value) in [(alternate.asset, 'float', 7), (changedKind.asset, 'int', 12)]) {
      final data = jsonDecode(fixture.controllerJson) as Map<String, dynamic>;
      data['id'] = id;
      data['parameters'][0]['type'] = kind;
      data['parameters'][0]['default'] = value;
      if (kind == 'int') data['layers'][0]['states'][0].remove('speedParameter');
      File('${project.path}/assets/rigs/$kind.controller.json').writeAsStringSync(jsonEncode(data));
    }
    cache = '${temp.path}/cache';
    cookProject(project.path, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    lib.dispose();
    temp.deleteSync(recursive: true);
  });

  (World, GameObject) fresh({bool tick = true}) {
    final world = World.create(library: lib);
    addTearDown(world.dispose);
    world.onLoadDiagnostics = (_) {};
    expect(world.loadScene(fixture.rigScene, cacheDir: cache).ok, isTrue);
    final rig = world.find('Rig')!;
    // The legacy submission boundary prepares asset dependencies. Ensure all
    // selected controllers are registered without advancing simulation.
    rig.set(rig.get<Animator>()!.copyWith(controller: fixture.controller));
    for (final controller in [alternate, changedKind]) {
      final preload = world.spawn()..set(Animator(controller: controller));
      world.edit(preload.destroy);
    }
    if (tick) world.frame(1 / 60);
    return (world, rig);
  }

  test('a newly selected controller is prepared for staged reads and discarded on abort', () {
    final world = World.create(library: lib);
    addTearDown(world.dispose);
    world.onLoadDiagnostics = (_) {};
    world.loadScene(fixture.rigScene, cacheDir: cache);
    late GameObject rejected;
    expect(
      () => world.edit(() {
        rejected = world.spawn(
          name: 'discarded-controller',
        )..set(const Animator(controller: alternate, clip: AssetRef('c0000000-0000-4000-8000-0000000000ff')));
        expect(rejected.animation.getFloat('speed'), 7);
        rejected.animation.setFloat('speed', 9);
        expect(rejected.animation.getFloat('speed'), 9);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(rejected.isAlive, isFalse);
    expect(world.find('discarded-controller'), isNull);
    final committed = world.edit(() {
      final go = world.spawn()..set(const Animator(controller: alternate));
      expect(go.animation.getFloat('speed'), 7);
      go.animation.setFloat('speed', 8);
      return go;
    });
    expect(committed.animation.getFloat('speed'), 8);
    world.frame(1 / 60);
    expect(committed.animation.getFloat('speed'), 8);
  });

  test('ordinary and explicit calls share committed logical values before any animation tick', () {
    final (world, rig) = fresh();
    final tick = world.tick;
    final animation = rig.animation;
    world.edit(() => animation.setFloat('speed', 3));
    expect(animation.getFloat('speed'), 3);
    expect(animation.parameters.singleWhere((p) => p.name == 'speed').value, 1);
    animation.setFloat('speed', 4);
    world.edit(() => expect(animation.getFloat('speed'), 4));
    animation.setInteger('weapon', 8);
    expect(animation.getInteger('weapon'), 8);
    expect(world.tick, tick, reason: 'opening parameter transactions does not advance simulation');
    world.frame(1 / 60);
    expect(animation.parameters.singleWhere((p) => p.name == 'speed').value, 4);
  });

  test('controller replacement reads new defaults and schema even for previously read names', () {
    final (world, rig) = fresh();
    final animation = rig.animation;
    animation.setFloat('speed', 5);
    world.frame(1 / 60);
    expect(animation.getFloat('speed'), 5);
    world.edit(() => rig.set(rig.get<Animator>()!.copyWith(controller: alternate)));
    expect(animation.getFloat('speed'), 7);
    animation.setFloat('speed', 9);
    expect(animation.getFloat('speed'), 9);
    world.edit(() => rig.set(rig.get<Animator>()!.copyWith(controller: changedKind)));
    expect(animation.getInteger('speed'), 12);
    expect(() => animation.getFloat('speed'), throwsArgumentError);
    expect(() => animation.setFloat('speed', 2), throwsStateError);
    animation.setInteger('speed', 13);
    expect(animation.getInteger('speed'), 13);
  });

  test('abort and rejected writes never leak to subsequent ordinary getters', () {
    final (world, rig) = fresh();
    final animation = rig.animation;
    animation.setFloat('speed', 2);
    expect(
      () => world.edit(() {
        animation.setFloat('speed', 8);
        expect(animation.getFloat('speed'), 8);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(animation.getFloat('speed'), 2);
    expect(() => animation.setFloat('speed', double.nan), throwsStateError);
    expect(() => animation.setInteger('weapon', 1 << 40), throwsArgumentError);
    expect(animation.getFloat('speed'), 2);
    expect(animation.getInteger('weapon'), 0);
    expect(
      () => world.edit(() {
        animation.setFloat('speed', 10);
        try {
          animation.getInteger('speed');
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(animation.getFloat('speed'), 2);
  });

  test('registered defaults are available before first tick and ordinary reads adopt pending sources', () {
    final (world, rig) = fresh(tick: false);
    expect(rig.animation.parameters, isEmpty);
    expect(rig.animation.getFloat('speed'), 1);
    expect(rig.animation.getBool('grounded'), isTrue);
    final pending = world.spawn()..set(const Animator(controller: fixture.controller));
    expect(pending.isPending, isFalse);
    expect(pending.animation.getFloat('speed'), 1);
    expect(pending.isPending, isFalse, reason: 'the read enters the same adoption boundary as World.edit');
    expect(world.tick, 0);
    pending.animation.setFloat('speed', 6);
    expect(pending.animation.getFloat('speed'), 6);
    expect(pending.animation.parameters, isEmpty);
  });

  test('removing and replacing Animator cannot preserve a stale controller schema', () {
    final (world, rig) = fresh();
    expect(rig.animation.getFloat('speed'), 1);
    world.edit(() => rig.remove<Animator>());
    expect(() => rig.animation.getFloat('speed'), throwsStateError);
    world.edit(() => rig.set(const Animator(controller: changedKind)));
    expect(rig.animation.getInteger('speed'), 12);
    expect(() => rig.animation.getFloat('speed'), throwsArgumentError);
  });
  test('committed Animator removal resets same-controller values and cancels old requests', () {
    final (world, rig) = fresh();
    rig.animation.setFloat('speed', 5);
    world.frame(1 / 60);
    rig.animation.setFloat('speed', 6);
    world.edit(() => rig.remove<Animator>());
    world.edit(() => rig.set(const Animator(controller: fixture.controller)));
    expect(rig.animation.getFloat('speed'), 1);
    final diagnostics = <FynkelDiagnostic>[];
    world.throwOnDiagnostics = false;
    world.onDiagnostics = diagnostics.addAll;
    world.frame(1 / 60);
    expect(rig.animation.getFloat('speed'), 1);
    expect(diagnostics.single.code, DiagnosticCode.missingComponent);
    world.frame(1 / 60);
    expect(diagnostics, hasLength(1));
  });
}
