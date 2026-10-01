import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/native/bridge.dart';
import 'package:fynkel/generated/schema.g.dart' as gen;
import 'package:test/test.dart';

void main() {
  late NativeLibrary library;
  late NativeWorld world;
  setUp(() {
    library = NativeLibrary.open();
    world = library.createWorld();
  });
  tearDown(() {
    world.dispose();
    library.dispose();
  });

  // Unbuilt objects intentionally have no live or legacy temp handle. A
  // transaction adapter supplies identity; the scalar encoder must never ask
  // these objects for an EntityRef.
  final targetObject = const ObjectSpec('target').object;
  final values = <WireRecord>[
    Follow(target: targetObject, offset: const Vec3(2, 3, 4)),
    ThirdPersonCamera(
      target: targetObject,
      distance: 7,
      pitch: 0.5,
      maxPitch: 1.25,
      collisionRadius: 0.25,
      lookAction: 'camera look',
    ),
    Skin(skeleton: targetObject),
  ];
  for (final original in values) {
    final codec = gen.componentCodecs[gen.componentIds[original.runtimeType]]!;
    test('${codec.name}: generated reference mapping survives pending and live FFI', () {
      codec.withType(<T extends WireRecord>() {
        final committed = world.editGameplay((edit) {
          final target = edit.spawn(name: 'target');
          final owner = edit.spawn(name: 'owner');
          var encoded = 0;
          edit.set(
            owner,
            original,
            encodeReference: (object) {
              expect(object, same(targetObject));
              encoded++;
              return target;
            },
          );
          expect(encoded, 1);
          var decoded = 0;
          final value = edit.component<T>(
            owner,
            decodeReference: (reference) {
              expect(reference, target);
              decoded++;
              return targetObject;
            },
          )!;
          expect(decoded, 1);
          expect(value.value, original);
          expect(value.references, [target]);
          return (owner, target);
        });
        world.editGameplay((edit) {
          final owner = NativeGameplayRef.live(committed.resolve(committed.value.$1)!);
          final target = committed.resolve(committed.value.$2)!;
          expect(
            edit
                .component<T>(
                  owner,
                  decodeReference: (reference) {
                    expect(reference!.live, target);
                    return targetObject;
                  },
                )!
                .value,
            original,
          );
          // The raw transport can still inspect tags without constructing SDK
          // objects. Mapping null really clears, unlike copyWith(null).
          final scalar = edit.component<T>(owner)!.value;
          final cleared = codec.mapObjectReferences(original, (_) => null);
          expect(scalar, cleared);
          var nulls = 0;
          edit.set(
            owner,
            cleared,
            encodeReference: (object) {
              expect(object, isNull);
              nulls++;
              return null;
            },
          );
          expect(nulls, 1);
          expect(edit.component<T>(owner)!.references, [null]);
        });
      });
    });
  }

  test('reference-free records do not invoke a resolver and keep scalar data', () {
    world.editGameplay((edit) {
      final owner = edit.spawn();
      const value = Layer(layer: 7);
      edit.set(owner, value, encodeReference: (_) => throw StateError('unexpected reference'));
      expect(
        edit.component<Layer>(owner, decodeReference: (_) => throw StateError('unexpected reference'))!.value,
        value,
      );
    });
  });

  test('caught resolver failures poison the edit on encode and decode', () {
    for (final onRead in [false, true]) {
      expect(
        () => world.editGameplay((edit) {
          final owner = edit.spawn();
          try {
            if (onRead) {
              edit.set(owner, const Follow(), references: [owner]);
              edit.component<Follow>(owner, decodeReference: (_) => throw StateError('resolver'));
            } else {
              edit.set(
                owner,
                Follow(target: targetObject),
                encodeReference: (_) => throw StateError('resolver'),
              );
            }
          } catch (_) {}
        }),
        throwsStateError,
      );
      world.editGameplay((edit) => expect(edit.entities(), isEmpty));
    }
  });

  test('ambiguous representations and unconverted objects cannot publish', () {
    for (final ambiguous in [false, true]) {
      expect(
        () => world.editGameplay((edit) {
          final owner = edit.spawn();
          try {
            edit.set(
              owner,
              Follow(target: targetObject),
              references: [owner],
              encodeReference: ambiguous ? (_) => owner : null,
            );
          } catch (_) {}
        }),
        throwsStateError,
      );
      world.editGameplay((edit) => expect(edit.entities(), isEmpty));
    }
  });

  test('expired reference returned by a resolver aborts the new edit', () {
    final previous = world.editGameplay((edit) => edit.spawn());
    expect(
      () => world.editGameplay((edit) {
        edit.set(edit.spawn(), Follow(target: targetObject), encodeReference: (_) => previous.value);
      }),
      throwsStateError,
    );
    world.editGameplay((edit) => expect(edit.entities(), hasLength(1)));
  });
}
