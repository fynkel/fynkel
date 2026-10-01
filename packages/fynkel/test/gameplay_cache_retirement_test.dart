import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/native/bridge.dart' show NativeGameplayRef;
import 'package:test/test.dart';

void main() {
  test('native publication is visible through existing SDK handles without a tick', () {
    final world = World.create();
    addTearDown(world.dispose);
    final parent = world.spawn(name: 'parent');
    final child = world.spawn(name: 'before')..set(const PointLight(range: 2));
    expect(child.get<PointLight>()!.range, 2);
    expect(child.parent, isNull);
    expect(child.transform.position, Vec3.zero);
    final childRef = NativeGameplayRef.live(child.entity);
    final parentRef = NativeGameplayRef.live(parent.entity);
    world.native.editGameplay((edit) {
      edit.setName(childRef, 'after');
      edit.set(childRef, const PointLight(range: 7));
      edit.set(parentRef, const Pose(position: Vec3(3, 0, 0)));
      edit.setParent(childRef, parentRef, keepWorld: false);
      edit.setActive(parentRef, false);
    });
    expect(child.name, 'after');
    expect(world.find('before'), isNull);
    expect(world.find('after'), same(child));
    expect(child.get<PointLight>()!.range, 7);
    expect(child.parent, same(parent));
    expect(parent.children, [child]);
    expect(child.activeInHierarchy, isFalse);
    expect(child.transform.position, const Vec3(3, 0, 0));
    expect(world.tick, 0);
    world.native.editGameplay((edit) => edit.destroy(childRef));
    expect(child.isAlive, isFalse, reason: 'identity bookkeeping is not a liveness cache');
    expect(() => world.attach(childRef.live!), throwsStateError);
    expect(world.byEntity(child.entity), isNull);
  });

  test('disposal snapshots poses without decoding references or changing identity membership', () {
    final world = World.create();
    final target = world.spawn();
    final source = world.spawn()..set(Follow(target: target));
    target.setLocalPosition(1, 2, 3);
    source.setLocalPosition(4, 5, 6);
    world.dispose();
    expect(target.isAlive || source.isAlive, isFalse);
    expect(target.localTransform.position, const Vec3(1, 2, 3));
    expect(source.localTransform.position, const Vec3(4, 5, 6));
    world.dispose();
  });
}
