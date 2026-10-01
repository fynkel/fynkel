// The Unity-shaped transform (docs/unity-parity.md § Tier 2): world
// position and rotation under a parent, the local ones, translate / rotate
// / rotateAround / lookAt, the axes and Euler degrees — over the real
// native library.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

Matcher near(Vec3 v) => predicate<Vec3>((a) => (a - v).length < 1e-4, 'within 1e-4 of $v');

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World world() => World.create(library: lib)..throwOnDiagnostics = true;

  group('math', () {
    test('Euler degrees round-trip and turn as the Inspector does (X, then Y, then Z)', () {
      for (final e in const [Vec3(10, 20, 30), Vec3(350, 5, 270), Vec3(0, 90, 0), Vec3(45, 0, 180)]) {
        expect(
          Quat.euler(Quat.euler(e).eulerAngles).rotate(const Vec3(0.3, 0.5, 0.7)),
          near(Quat.euler(e).rotate(const Vec3(0.3, 0.5, 0.7))),
          reason: '$e',
        );
      }
      expect(Quat.euler(const Vec3(10, 20, 30)).eulerAngles, near(const Vec3(10, 20, 30)));
      expect(
        Quat.euler(const Vec3(-10, 0, 0)).eulerAngles,
        near(const Vec3(350, 0, 0)),
        reason: 'within [0, 360)',
      );
      expect(Quat.euler(const Vec3(0, 90, 0)).rotate(Vec3.forward), near(const Vec3(-1, 0, 0)));
      // The Inspector's order: qz * qy * qx.
      final byAxis =
          Quat.axisAngle(const Vec3(0, 0, 1), 0.5) *
          Quat.axisAngle(const Vec3(0, 1, 0), 0.25) *
          Quat.axisAngle(const Vec3(1, 0, 0), 0.125);
      final q = Quat.euler(Vec3(0.125, 0.25, 0.5) * (180 / 3.141592653589793));
      expect(q.rotate(Vec3.one), near(byAxis.rotate(Vec3.one)));
    });

    test('lookRotation points forward (−Z) along the direction, up kept', () {
      for (final d in const [Vec3(1, 0, 0), Vec3(0, 0, 1), Vec3(1, 2, -3), Vec3(0, 1, 0), Vec3(0, -1, 0)]) {
        final q = Quat.lookRotation(d);
        expect(q.rotate(Vec3.forward), near(d.normalized), reason: '$d');
        expect(q.length, closeTo(1, 1e-6));
      }
      expect(Quat.lookRotation(const Vec3(1, 0, 0)).rotate(Vec3.up), near(Vec3.up));
      expect(Quat.lookRotation(Vec3.zero), Quat.identity);
    });
  });

  test('position and rotation are the world ones under a turned, scaled parent', () {
    final w = world();
    addTearDown(w.dispose);
    final parent = w.spawn(
      transform: Pose(
        position: const Vec3(10, 0, 0),
        rotation: Quat.euler(const Vec3(0, 90, 0)),
        scale: const Vec3.all(2),
      ),
      name: 'parent',
    );
    final child = w.spawn(
      transform: const Pose(position: Vec3(0, 0, -1), rotation: Quat.identity, scale: Vec3.one),
      parent: parent,
      name: 'child',
    );
    // Pending: the spawn's parent counts already.
    expect(child.transform.position, near(const Vec3(8, 0, 0)));
    expect(child.transform.parent!.localPosition, const Vec3(10, 0, 0));
    w.frame(1 / 60);
    w.frame(1 / 60);
    final t = child.transform;
    expect(t.localPosition, const Vec3(0, 0, -1));
    expect(t.position, near(const Vec3(8, 0, 0)));
    expect(t.position, near(child.worldTransform!.position), reason: 'what the world computes');
    expect(t.lossyScale, near(const Vec3.all(2)));
    expect(t.eulerAngles, near(const Vec3(0, 90, 0)));
    expect(t.forward, near(const Vec3(-1, 0, 0)));

    // A world write lands at that world point; the local one follows.
    t.position = const Vec3(10, 4, 0);
    expect(t.position, near(const Vec3(10, 4, 0)), reason: 'read-your-writes');
    expect(t.localPosition, near(const Vec3(0, 2, 0)));
    t.rotation = Quat.identity;
    expect(t.localEulerAngles, near(const Vec3(0, 270, 0)));
    w.frame(1 / 60);
    w.frame(1 / 60);
    expect(child.worldTransform!.position, near(const Vec3(10, 4, 0)));
    expect(t.eulerAngles, near(Vec3.zero));

    // Moving the parent moves the child's world position this tick.
    parent.transform.position = const Vec3(0, 0, 0);
    expect(t.position, near(const Vec3(0, 4, 0)));

    // Reparented to nothing this tick: the local is the world.
    child.setParent(null);
    expect(t.parent, isNull);
    expect(t.position, t.localPosition);
  });

  test('affine hierarchy retains shear through staged edits and native ticks', () {
    final w = world();
    addTearDown(w.dispose);
    final root = w.spawn(
      transform: const Pose(position: Vec3(3, 2, 1), rotation: Quat.identity, scale: Vec3(2, 1, -3)),
    );
    final branch = w.spawn(
      parent: root,
      transform: Pose(position: Vec3.zero, rotation: Quat.euler(const Vec3(0, 0, 45)), scale: Vec3.one),
    );
    final leaf = w.spawn(
      parent: branch,
      transform: const Pose(position: Vec3(1, 0, 0), rotation: Quat.identity, scale: Vec3.one),
    );
    final expected =
        const Vec3(3, 2, 1) +
        const Vec3(2, 1, -3).scaled(Quat.euler(const Vec3(0, 0, 45)).rotate(Vec3.right));
    expect(leaf.transform.position, near(expected));
    const point = Vec3(2, -4, 3);
    expect(branch.transform.inverseTransformPoint(branch.transform.transformPoint(point)), near(point));
    expect(branch.transform.inverseTransformVector(branch.transform.transformVector(point)), near(point));
    expect(branch.transform.transformDirection(point).length, closeTo(point.length, 1e-5));
    expect(branch.transform.lossyScale.y, closeTo(1.58113883, 1e-5));
    w.frame(1 / 60);
    expect(leaf.worldTransform!.position, near(expected));
    root.transform.localScale = const Vec3(5, 2, 1);
    branch.transform.localPosition = const Vec3(2, 1, 0);
    final staged = leaf.transform.position;
    leaf.transform.position = const Vec3(9, -3, 2);
    expect(leaf.transform.position, near(const Vec3(9, -3, 2)));
    expect(staged, isNot(near(expected)));
    w.frame(1 / 60);
    expect(leaf.worldTransform!.position, near(const Vec3(9, -3, 2)));
  });

  test('keep-world reparents preserve geometry or reject before staging', () {
    final w = world();
    addTearDown(w.dispose);
    final root = w.spawn(
      transform: const Pose(position: Vec3(3, 0, 0), rotation: Quat.identity, scale: Vec3.all(2)),
    );
    final child = w.spawn(
      transform: Pose(
        position: Vec3(1, 2, 3),
        rotation: Quat.euler(const Vec3(10, 20, 30)),
        scale: Vec3(1, 2, -1),
      ),
    );
    final before = child.transform.transformPoint(Vec3.one);
    child.setParent(root, keepWorld: true);
    expect(child.transform.transformPoint(Vec3.one), near(before));
    w.frame(1 / 60);
    expect(child.transform.transformPoint(Vec3.one), near(before));
    root.transform.localScale = const Vec3(2, 1, 1);
    expect(() => child.setParent(null, keepWorld: true), throwsStateError);
    expect(child.parent, root);
    final singular = w.spawn(
      transform: const Pose(position: Vec3.zero, rotation: Quat.identity, scale: Vec3(0, 1, 1)),
    );
    expect(() => child.setParent(singular, keepWorld: true), throwsStateError);
    expect(() => singular.transform.inverseTransformPoint(Vec3.one), throwsStateError);
    expect(singular.transform.transformPoint(Vec3.one), const Vec3(0, 1, 1));
    expect(() => root.setParent(child), throwsStateError);
    expect(root.parent, isNull);
    // Rejected operations did not enqueue hierarchy edits.
    w.frame(1 / 60);
    expect(child.parent, root);
  });

  test('translate, rotate, rotateAround, lookAt and the axes', () {
    final w = world();
    addTearDown(w.dispose);
    final go = w.spawn(name: 'mover');
    w.frame(1 / 60);
    final t = go.transform;

    t.rotate(const Vec3(0, 90, 0));
    expect(t.forward, near(const Vec3(-1, 0, 0)));
    expect(t.right, near(const Vec3(0, 0, -1)));
    expect(t.up, near(Vec3.up));
    t.translate(Vec3.forward);
    expect(t.position, near(const Vec3(-1, 0, 0)), reason: 'self: along its own forward');
    t.translate(Vec3.forward, relativeTo: Space.world);
    expect(t.position, near(const Vec3(-1, 0, -1)));

    // Self turns about its own (turned) X; world about the world's X.
    t.rotate(const Vec3(90, 0, 0));
    expect(t.forward, near(Vec3.up));
    t.rotation = Quat.euler(const Vec3(0, 90, 0));
    t.rotate(const Vec3(90, 0, 0), relativeTo: Space.world);
    expect(t.forward, near(const Vec3(-1, 0, 0)), reason: 'forward lies on the world X axis');
    expect(t.up, near(const Vec3(0, 0, 1)));

    t.value = identityTransform;
    t.position = const Vec3(2, 0, 0);
    t.rotateAround(Vec3.zero, Vec3.up, 90);
    expect(t.position, near(const Vec3(0, 0, -2)));
    expect(t.eulerAngles, near(const Vec3(0, 90, 0)));

    t.lookAt(const Vec3(0, 0, 5));
    expect(t.forward, near(const Vec3(0, 0, 1)));
    expect(t.up, near(Vec3.up));
    t.forward = const Vec3(1, 0, 0);
    expect(t.forward, near(const Vec3(1, 0, 0)));

    expect(t.transformPoint(Vec3.forward), near(t.position + t.forward));
    expect(t.inverseTransformPoint(t.transformPoint(const Vec3(1, 2, 3))), near(const Vec3(1, 2, 3)));
    expect(t.inverseTransformDirection(t.transformDirection(const Vec3(1, 2, 3))), near(const Vec3(1, 2, 3)));

    w.frame(1 / 60);
    expect(go.worldTransform!.position, near(const Vec3(0, 0, -2)));
    expect(go.worldTransform!.rotation.rotate(Vec3.forward), near(const Vec3(1, 0, 0)));
  });
}
