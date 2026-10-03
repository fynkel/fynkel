import 'package:fynkel/gameplay.dart';
import 'package:test/test.dart';

class _Counter extends Behaviour {
  int count = 0;
}

void main() {
  test('the same recipe builds independent trees and forward references in one edit', () {
    final world = World.create();
    addTearDown(world.dispose);
    final recipe = ObjectRecipe(() {
      final target = ObjectSpec('target');
      return ObjectSpec(
        'root',
        children: [
          ObjectSpec(
            'follower',
            components: [Follow(target: target.object)],
            behaviours: [_Counter()],
          ),
          target,
        ],
      );
    });
    final first = recipe.create();
    final second = recipe.create();
    world.edit(() {
      world.build(first);
      world.build(second);
    });
    expect(first.object, isNot(same(second.object)));
    expect(first.children.first.object.get<Follow>()!.target, same(first.children.last.object));
    expect(second.children.first.object.get<Follow>()!.target, same(second.children.last.object));
    final a = first.children.first.behaviours.single as _Counter;
    final b = second.children.first.behaviours.single as _Counter;
    a.count++;
    expect(b.count, 0);
    first.object.destroy();
    expect(second.object.isAlive, true);
  });

  test('forest references resolve before publication and invalid forests roll back', () {
    final world = World.create();
    addTearDown(world.dispose);
    final target = ObjectSpec('target');
    final follower = ObjectSpec('follower', components: [Follow(target: target.object)]);
    final roots = world.buildAll([follower, target]);
    expect(roots.map((r) => r.parent), everyElement(isNull));
    expect(roots.first.get<Follow>()!.target, same(roots.last));
    expect(() => world.buildAll([ObjectSpec('never'), ObjectSpec('bad', layer: 'unknown')]), throwsStateError);
    expect(world.find('never'), isNull);
    expect(world.buildAll([]), isEmpty);
  });

  test('factory reuse, shared behaviours and cyclic descriptions fail before publication', () {
    final shared = ObjectSpec('shared');
    final reused = ObjectRecipe(() => shared);
    reused.create();
    expect(reused.create, throwsStateError);
    expect(
      ObjectRecipe(() => shared).create,
      throwsStateError,
      reason: 'a new recipe wrapper cannot make an issued node fresh',
    );
    final world = World.create();
    addTearDown(world.dispose);
    final built = ObjectSpec('already built');
    world.build(built);
    expect(ObjectRecipe(() => built).create, throwsStateError);

    final script = _Counter();
    final scripts = ObjectRecipe(() => ObjectSpec('root', behaviours: [script]));
    scripts.create();
    expect(scripts.create, throwsStateError);
    final children = <ObjectSpec>[];
    final cycle = ObjectSpec('cycle', children: children);
    children.add(cycle);
    expect(ObjectRecipe(() => cycle).create, throwsStateError);
  });

  test('a failed build does not contaminate the next recipe instance', () {
    final world = World.create();
    addTearDown(world.dispose);
    final recipe = ObjectRecipe(() => ObjectSpec('root', behaviours: [_Counter()]));
    final canceled = recipe.create();
    expect(
      () => world.edit(() {
        world.build(canceled);
        throw StateError('cancel');
      }),
      throwsStateError,
    );
    expect(world.find('root'), isNull);
    final next = world.build(recipe.create());
    expect(next.isAlive, true);
  });
}
