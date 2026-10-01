import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

ClipData pose(double height) =>
    (ClipBuilder(length: 1)..position('joint', {0: Vec3(0, height, 0), 1: Vec3(0, height, 0)})).build();

void main() {
  late Directory cache;
  late NativeLibrary library;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-runtime-clips-');
    cookProject(Directory('../../crates/fynkel_document/fixtures/project').absolute.path, cacheDir: cache.path);
    library = NativeLibrary.open();
  });
  tearDownAll(() {
    library.dispose();
    cache.deleteSync(recursive: true);
  });

  World fresh() {
    final world = World.create(library: library);
    addTearDown(world.dispose);
    world.onLoadDiagnostics = (_) {};
    world.loadScene('c0000000-0000-4000-8000-000000000034', cacheDir: cache.path);
    return world;
  }

  (GameObject, GameObject) rig(World world) => world.edit(() {
    final root = world.spawn(name: 'runtime rig')..set(const Animator(playing: false));
    return (root, world.spawn(name: 'joint', parent: root));
  });

  test('aborted clip creation rolls back liveness and entity data', () {
    final world = fresh();
    final root = world.spawn(name: 'before');
    late AnimationClipAsset discarded;
    expect(
      () => world.edit(() {
        discarded = world.createClip(pose(2));
        expect(discarded.isAlive, isTrue);
        root.name = 'discarded';
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(discarded.isAlive, isFalse);
    expect(root.name, 'before');
    expect(
      () => world.frame(
        0,
        update: () {
          discarded = world.createClip(pose(8));
          root.name = 'discarded callback';
          throw StateError('abort callback');
        },
      ),
      throwsStateError,
    );
    expect(root.name, 'before');
    world.frame(1 / 60);
    expect(discarded.isAlive, isFalse);
  });

  test('automatic callbacks create and play clips at four frame rates', () {
    for (final hz in [30, 60, 120, 240]) {
      final world = fresh();
      final (root, joint) = rig(world);
      late AnimationClipAsset clip;
      world.frame(
        0,
        update: () {
          clip = world.createClip(pose(3));
          root.animation.play(clip, looping: true);
        },
      );
      expect(clip.isAlive, isTrue);
      expect(joint.localTransform.position.y, 0);
      for (var i = 0; i < hz ~/ 10; i++) {
        world.frame(1 / hz);
      }
      expect(joint.localTransform.position.y, closeTo(3, 1e-5));
      expect(root.animation.state!.time, greaterThan(0.05), reason: 'play is not replayed every frame');
    }
  });

  test('replacement abort preserves keys and successful repeated writes use final keys', () {
    final world = fresh();
    final (root, joint) = rig(world);
    final clip = world.createClip(pose(2));
    expect(
      () => world.edit(() {
        world.setClip(clip, pose(7));
        throw StateError('abort');
      }),
      throwsStateError,
    );
    root.animation.play(clip);
    world.frame(1 / 60);
    expect(joint.localTransform.position.y, closeTo(2, 1e-5));
    world.edit(() {
      world.setClip(clip, pose(4));
      world.setClip(clip, pose(9));
      root.animation.play(clip);
    });
    world.frame(1 / 60);
    expect(joint.localTransform.position.y, closeTo(9, 1e-5));
  });

  test('removal is logical in the edit, rollback restores it, and playing clips survive removal', () {
    final world = fresh();
    final (root, joint) = rig(world);
    final clip = world.createClip(pose(5));
    root.animation.play(clip);
    world.frame(1 / 60);
    expect(
      () => world.edit(() {
        clip.destroy();
        expect(clip.isAlive, isFalse);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(clip.isAlive, isTrue);
    world.edit(clip.destroy);
    expect(clip.isAlive, isFalse);
    world.frame(1 / 60);
    expect(joint.localTransform.position.y, closeTo(5, 1e-5));
  });

  test('create then remove in one edit publishes no live clip', () {
    final world = fresh();
    late AnimationClipAsset clip;
    world.edit(() {
      clip = world.createClip(pose(2));
      clip.destroy();
      expect(clip.isAlive, isFalse);
    });
    expect(clip.isAlive, isFalse);
    final (root, _) = rig(world);
    root.animation.play(clip);
    expect(
      () => world.frame(1 / 60),
      throwsA(
        isA<FynkelError>().having(
          (error) => error.diagnostics.map((d) => d.code),
          "diagnostics",
          contains(DiagnosticCode.clipUnavailable),
        ),
      ),
    );
    expect(root.animation.state!.duration, 0);
    expect(root.animation.state!.time, 0);
  });

  test('malformed native clip request stickily aborts preceding creation and resource changes', () {
    final world = fresh();
    final native = world.native;
    const id = 'd0000000-0000-4000-8000-000000000099';
    expect(
      () => native.editGameplay((edit) {
        edit.spawn(name: 'must not survive');
        edit.setRuntimeClip(id, pose(2).toPayload());
        try {
          edit.setRuntimeClip(id, Uint8List.fromList([1, 2, 3]));
        } catch (_) {}
      }),
      throwsA(anything),
    );
    expect(world.find('must not survive'), isNull);
    final (root, _) = rig(world);
    root.animation.play(const AssetRef(id));
    expect(
      () => world.frame(1 / 60),
      throwsA(
        isA<FynkelError>().having(
          (error) => error.diagnostics.map((d) => d.code),
          "diagnostics",
          contains(DiagnosticCode.clipUnavailable),
        ),
      ),
    );
    expect(root.animation.state!.duration, 0);
    expect(root.animation.state!.time, 0);
  });
}
