import 'package:fynkel/advanced.dart';
import 'package:fynkel/src/native/bridge.dart';
import 'package:test/test.dart';

void main() {
  late NativeLibrary library;
  late NativeWorld world;
  setUp(() {
    library = NativeLibrary.open();
    world = library.createWorld();
  });
  tearDown(() {
    if (!world.isDisposed) world.dispose();
    library.dispose();
  });

  test('audio bus bridge shares global mute and commits atomically with data', () {
    expect(
      () => world.editGameplay((edit) {
        edit.spawn(name: 'aborted bus owner');
        edit.setAudioBus(AudioBus.master, 0.25, muted: true);
        expect(edit.audioBus(AudioBus.master), (0.25, true));
        expect(edit.audioBus(AudioBus.music), (1.0, true));
        throw StateError('abort');
      }),
      throwsStateError,
    );
    world.editGameplay((edit) {
      expect(edit.entities(), isEmpty);
      expect(edit.audioBus(AudioBus.master), (1.0, false));
      edit.setAudioBus(AudioBus.master, 0.5, muted: true);
      edit.setAudioBus(AudioBus.music, 0.25);
      expect(edit.audioBus(AudioBus.master), (0.5, false));
    });
    world.editGameplay((edit) {
      expect(edit.audioBus(AudioBus.master), (0.5, false));
      expect(edit.audioBus(AudioBus.music), (0.25, false));
    });
    expect(
      () => world.editGameplay((edit) {
        edit.setAudioBus(AudioBus.sfx, 0.125, muted: true);
        try {
          edit.setAudioBus(AudioBus.master, double.nan);
        } catch (_) {}
      }),
      throwsStateError,
    );
    world.editGameplay((edit) => expect(edit.audioBus(AudioBus.sfx), (1.0, false)));
  });

  test('clone bridge returns scoped mappings and remaps every native reference family', () {
    final committed = world.editGameplay((edit) {
      final parent = edit.spawn(
        name: 'parent',
        transform: const Pose(position: Vec3(5, 0, 0)),
      );
      final source = edit.spawn(
        name: 'source',
        parent: parent,
        transform: const Pose(position: Vec3(2, 0, 0)),
      );
      final child = edit.spawn(name: 'child', parent: source);
      edit.set(source, const Follow(), references: [child]);
      edit.set(child, const ThirdPersonCamera(), references: [source]);
      edit.set(child, const Skin(), references: [child]);
      final clone = edit.cloneTree(source, name: '');
      expect(edit.name(clone.root), '');
      expect(edit.worldMatrix(clone.root)[12], 7);
      final copiedChild = clone.entities[child]!;
      expect(edit.component<Follow>(clone.root)!.references.single, copiedChild);
      expect(edit.component<ThirdPersonCamera>(copiedChild)!.references.single, clone.root);
      expect(edit.component<Skin>(copiedChild)!.references.single, copiedChild);
      expect(edit.parent(copiedChild), clone.root);
      return clone;
    });
    expect(world.contains(committed.resolve(committed.value.root)!), isTrue);
    expect(committed.value.entities.values.every((r) => committed.resolve(r) != null), isTrue);
    world.editGameplay((edit) {
      final source = NativeGameplayRef.live(committed.resolve(committed.value.root)!);
      final clone = edit.cloneTree(source, transform: const Pose(position: Vec3(11, 0, 0)));
      expect(edit.worldMatrix(clone.root)[12], 11);
    });
  });

  test('clone references expire on abort and failed foreign access poisons the session', () {
    late NativeGameplayClone aborted;
    expect(
      () => world.editGameplay((edit) {
        final source = edit.spawn(name: 'abort');
        aborted = edit.cloneTree(source);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(() => world.editGameplay((edit) => edit.cloneTree(aborted.root)), throwsStateError);
    final other = library.createWorld();
    addTearDown(other.dispose);
    final foreign = other.editGameplay((edit) => edit.spawn());
    expect(
      () => world.editGameplay((edit) {
        edit.spawn(name: 'must roll back');
        try {
          edit.cloneTree(NativeGameplayRef.live(foreign.resolve(foreign.value)!));
        } catch (_) {}
      }),
      throwsStateError,
    );
    world.editGameplay((edit) => expect(edit.entities(), isEmpty));
  });

  test('pending hierarchy and every reference family survive commit through FFI', () {
    final result = world.editGameplay((edit) {
      final target = edit.spawn(
        name: 'target',
        transform: const Pose(position: Vec3(4, 0, 0)),
      );
      final owner = edit.spawn(name: 'owner', parent: target);
      edit.set(owner, const Follow(), references: [target]);
      edit.set(owner, const ThirdPersonCamera(), references: [target]);
      edit.set(owner, const Skin(), references: [owner]);
      expect(edit.component<Follow>(owner)!.references.single, target);
      expect(edit.component<Skin>(owner)!.references.single, owner);
      expect(edit.parent(owner), target);
      expect(edit.children(target), [owner]);
      expect(edit.find('owner'), owner);
      expect(edit.worldMatrix(owner)[12], 4);
      expect(edit.entities(components: [Follow.componentId]), [owner]);
      edit.setName(owner, 'renamed');
      expect(edit.find('owner'), isNull);
      edit.setActive(target, false);
      expect(edit.active(owner), (true, false));
      return (target, owner);
    });
    final target = result.resolve(result.value.$1)!;
    final owner = result.resolve(result.value.$2)!;
    expect(world.contains(owner), isTrue);
    world.editGameplay((edit) {
      final ref = NativeGameplayRef.live(owner);
      expect(edit.name(ref), 'renamed');
      expect(edit.component<Follow>(ref)!.references.single!.live, target);
      expect(edit.component<Skin>(ref)!.references.single!.live, owner);
    });
  });

  test('caught native validation failure still rolls back the whole callback', () {
    expect(
      () => world.editGameplay((edit) {
        final e = edit.spawn(name: 'discarded');
        try {
          edit.set(e, const AudioSource(volume: -1));
        } catch (_) {}
      }),
      throwsStateError,
    );
    world.editGameplay((edit) {
      expect(edit.entities(), isEmpty);
    });
  });

  test('callback exception and final reference failure publish nothing', () {
    expect(
      () => world.editGameplay((edit) {
        edit.spawn();
        throw StateError('callback');
      }),
      throwsStateError,
    );
    expect(
      () => world.editGameplay((edit) {
        final a = edit.spawn(), b = edit.spawn();
        edit.set(a, const Follow(), references: [b]);
        edit.destroy(b);
      }),
      throwsStateError,
    );
    world.editGameplay((edit) {
      expect(edit.entities(), isEmpty);
    });
  });

  test('closed edits and pending tokens cannot be reused, including across worlds', () {
    late NativeGameplayEdit escaped;
    final a = world.editGameplay((edit) {
      escaped = edit;
      return edit.spawn();
    });
    expect(() => escaped.entities(), throwsStateError);
    expect(
      () => world.editGameplay((edit) {
        edit.name(a.value);
      }),
      throwsStateError,
    );
    final other = library.createWorld();
    addTearDown(other.dispose);
    expect(
      () => other.editGameplay((edit) {
        final owner = edit.spawn();
        edit.set(owner, const Follow(), references: [a.value]);
      }),
      throwsStateError,
    );
    expect(other.isPoisoned, isFalse);
  });

  test('legacy native mutation cannot interleave and ignored rejection aborts', () {
    expect(
      () => world.editGameplay((edit) {
        edit.spawn();
        expect(world.step().status, FynkelStatus.FYNKEL_STATUS_COMMANDS_REJECTED);
      }),
      throwsStateError,
    );
    expect(world.isPoisoned, isFalse);
    world.editGameplay((edit) {
      expect(edit.entities(), isEmpty);
    });
  });

  test('removal, cancellation and next-tick consumption use canonical storage', () {
    final r = world.editGameplay((edit) {
      final e = edit.spawn(), cancelled = edit.spawn();
      edit.set(e, const Rotator());
      edit.set(e, const Layer(layer: 3));
      edit.remove<Layer>(e);
      expect(edit.component<Layer>(e), isNull);
      edit.destroy(cancelled);
      return (e, cancelled);
    });
    expect(r.resolve(r.value.$2), isNull);
    final ref = NativeGameplayRef.live(r.resolve(r.value.$1)!);
    world.step();
    world.editGameplay((edit) {
      expect(edit.component<Pose>(ref)!.value.rotation, isNot(Quat.identity));
    });
  });

  test('nested and asynchronous callbacks abort and leave world usable', () async {
    expect(
      () => world.editGameplay((edit) {
        edit.spawn();
        try {
          world.editGameplay((_) {});
        } catch (_) {}
      }),
      throwsA(anything),
    );
    expect(
      () => world.editGameplay((edit) async {
        edit.spawn();
        await Future<void>.value();
        edit.spawn();
      }),
      throwsStateError,
    );
    await Future<void>.delayed(Duration.zero);
    world.editGameplay((edit) {
      expect(edit.entities(), isEmpty);
    });
  });
}
