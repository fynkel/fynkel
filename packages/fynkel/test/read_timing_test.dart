// Which reads show this tick's writes and which the world as the last tick
// left it (docs/foundation.md 1.10, docs/unity-parity.md § On purpose).
// Unity applies everything at once; here writes are commands the next tick
// applies (ADR-0008), and a few reads come from the tick's table:
//
// - read your writes: `transform.*`, `localTransform`, `get<T>`, `activeSelf`,
//   `parent`/`children`, `name`, `behaviour<T>()`;
// - the last tick: `worldTransform`, `activeInHierarchy`, `find`/`findAll`/
//   `findWithTag` (a spawn or a rename this tick is found from the next);
//   `worldTransform` is null until the tick after its first read.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  test('reads that show this tick and reads of the last tick (foundation 1.10)', () {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    final parent = w.spawn(name: 'Parent');
    final child = w.spawn(name: 'Child', parent: parent);
    w.frame(1 / 60);
    // worldTransform is the last tick's: asked from the world on the
    // first read, from the snapshot after (foundation 4.4).
    expect(child.worldTransform, isNotNull);
    w.frame(1 / 60);
    expect(child.worldTransform, isNotNull);

    // A spawn: usable at once, found by name from the next tick.
    final coin = w.spawn(name: 'Coin');
    expect(coin.name, 'Coin');
    expect(w.find('Coin'), isNull);

    // A move: transform reads it now, worldTransform after the tick.
    parent.transform.position = const Vec3(5, 0, 0);
    expect(child.transform.position, const Vec3(5, 0, 0));
    expect(child.worldTransform!.position, Vec3.zero);

    // SetActive: activeSelf now, activeInHierarchy after the tick.
    parent.setActive(false);
    expect(parent.activeSelf, isFalse);
    expect(child.activeInHierarchy, isTrue);

    w.frame(1 / 60);
    expect(w.find('Coin'), same(coin));
    expect(child.worldTransform!.position, const Vec3(5, 0, 0));
    expect(child.activeInHierarchy, isFalse);
  });
}
