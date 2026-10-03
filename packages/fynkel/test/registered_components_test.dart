import 'package:fynkel/gameplay.dart';
import 'package:test/test.dart';

class Health {
  const Health(this.hp, [this.target]);
  final double hp;
  final GameObject? target;
}

final health = ComponentType<Health>(
  id: 'game.Health',
  fields: const [
    ComponentField('hp', ComponentFieldKind.number, 100.0),
    ComponentField('target', ComponentFieldKind.object, null),
  ],
  encode: (h) => [h.hp, h.target],
  decode: (v) => Health(v[0] as double, v[1] as GameObject?),
);

void main() {
  late World w;
  setUp(() {
    w = World.create();
    w.registerComponent(health);
  });
  tearDown(() => w.dispose());
  test('caught addData default decoding failure rejects the entire edit', () {
    final invalid = ComponentType<int>(
      id: 'game.InvalidDefaults',
      fields: const [
        ComponentField('value', ComponentFieldKind.integer, 'invalid'),
      ],
      encode: (value) => [value],
      decode: (fields) => fields.single as int,
    );
    final player = w.spawn(name: 'original');
    expect(
      () => w.edit(() {
        player.name = 'discard';
        try {
          player.addData(invalid);
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(player.name, 'original');
    expect(w.registeredComponentTypes.any((t) => t.id == invalid.id), isFalse);
  });
  test(
    'read-only bulk returns an owned snapshot with one native population call',
    () {
      final p = w.spawn()..addData(health);
      final native = w.native;
      final snapshot = w.edit(() {
        final calls = native.ffiCalls;
        final rows = w.queryComponents(health);
        expect(native.ffiCalls - calls, 1);
        return rows;
      });
      expect(snapshot.single.owner, same(p));
      p.writeData(health, const Health(5));
      expect(snapshot.single.value.hp, 100);
      expect(p.readData(health)!.hp, 5);
    },
  );
  test('integer text boolean and asset subresource round trip through bulk and clone', () {
    final data = ComponentType<(int, bool, String, AssetRef)>(
      id: 'game.Record',
      fields: const [
        ComponentField('count', ComponentFieldKind.integer, 0),
        ComponentField('enabled', ComponentFieldKind.boolean, false),
        ComponentField('label', ComponentFieldKind.text, ''),
        ComponentField('asset', ComponentFieldKind.asset, AssetRef.none),
      ],
      encode: (v) => [v.$1, v.$2, v.$3, v.$4],
      decode: (v) =>
          (v[0] as int, v[1] as bool, v[2] as String, v[3] as AssetRef),
    );
    w.registerComponent(data);
    const asset = AssetRef(
      'd0000000-0000-4000-8000-000000000099',
      sub: 'mesh/2',
    );
    final p = w.spawn()
      ..writeData(data, (9007199254740993, true, 'héllo 🎮', asset));
    w.updateComponents(data, (_, v) => (v.$1 + 1, !v.$2, v.$3, v.$4));
    final cloned = p.clone();
    expect(cloned.readData(data), (9007199254740994, false, 'héllo 🎮', asset));
  });
  test(
    'typed values and pending references share native transaction and clone',
    () {
      final p = w.edit(() {
        final p = w.spawn(name: 'player');
        p.writeData(health, Health(80, p));
        expect(p.readData(health)!.target, same(p));
        return p;
      });
      expect(p.readData(health)!.target, same(p));
      final clone = p.clone();
      expect(clone.readData(health)!.hp, 80);
      expect(clone.readData(health)!.target, same(clone));
      w.edit(() {
        p.destroy();
        expect(clone.readData(health)!.target, same(clone));
      });
    },
  );
  test(
    'caught invalid scalar rejects registered and builtin edits together',
    () {
      final p = w.spawn(name: 'player');
      p.addData(health);
      expect(
        () => w.edit(() {
          p.name = 'discard';
          try {
            p.writeData(health, const Health(double.nan));
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(p.name, 'player');
      expect(p.readData(health)!.hp, 100);
    },
  );
  test(
    'bulk updates and ordinary reads agree without per-entity native calls',
    () {
      final objects = w.edit(
        () => List.generate(200, (_) {
          final p = w.spawn();
          p.addData(health);
          return p;
        }),
      );
      final instrumentation = w.native;
      w.edit(() {
        final before = instrumentation.ffiCalls;
        w.updateComponents(health, (_, h) => Health(h.hp - 5));
        expect(
          instrumentation.ffiCalls - before,
          2,
          reason: 'one population read and one atomic apply',
        );
      });
      expect(objects.first.readData(health)!.hp, 95);
      expect(objects.last.readData(health)!.hp, 95);
      w.edit(() {
        objects.first.writeData(health, const Health(42));
        w.updateComponents(health, (_, h) => Health(h.hp - 1));
        expect(objects.first.readData(health)!.hp, 41);
      });
    },
  );
  test(
    'bulk reentrant edits invalidate the snapshot and roll back all writes',
    () {
      final p = w.spawn();
      p.addData(health);
      expect(
        () => w.updateComponents(health, (owner, h) {
          owner.writeData(health, const Health(7));
          return const Health(8);
        }),
        throwsStateError,
      );
      expect(p.readData(health)!.hp, 100);
    },
  );
  test('destroyed targets become null including staged records', () {
    final p = w.spawn(), target = w.spawn();
    w.edit(() {
      p.writeData(health, Health(10, target));
      target.destroy();
      expect(p.readData(health)!.target, isNull);
    });
    expect(p.readData(health)!.target, isNull);
  });
  test(
    'registration conflict and late registration fail without replacing data',
    () {
      final p = w.spawn();
      p.addData(health);
      final conflict = ComponentType<Health>(
        id: 'game.Health',
        version: 2,
        fields: health.fields,
        encode: health.encode,
        decode: health.decode,
      );
      expect(() => w.registerComponent(conflict), throwsStateError);
      expect(() => w.edit(() => w.registerComponent(health)), throwsStateError);
      expect(p.readData(health)!.hp, 100);
    },
  );
}
