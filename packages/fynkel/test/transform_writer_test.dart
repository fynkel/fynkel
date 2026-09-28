// C1 (blueprint §19.2c item 1): allocation-free transform writes produce the
// same wire bytes and read-your-writes results as the value API.
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/wire/support.dart' show WireStrings;
import 'package:test/test.dart';

void main() {
  test('TransformWriter encodes exactly like Transform', () {
    final t = Pose(
      position: const Vec3(1, 2, 3),
      rotation: Quat.axisAngle(Vec3.up, 0.7) * Quat.axisAngle(Vec3.right, 0.2),
      scale: const Vec3(2, 2, 2),
    );
    final w = TransformWriter().identity().setRotationAxisAngle(0, 1, 0, 0.7).rotateAxisAngle(1, 0, 0, 0.2);
    w.setPosition(1, 2, 3).setUniformScale(2);
    final a = ByteData(40), b = ByteData(40);
    t.encode(a, 0, WireStrings());
    w.encode(b, 0);
    expect(b.buffer.asUint8List(), a.buffer.asUint8List());
    expect(TransformWriter().decode(a, 0).toTransform(), Pose.decode(a, 0, Uint8List(0)));
  });

  test('scalar writes keep unspecified parts and read back before the tick', () {
    final world = World.create()..throwOnDiagnostics = true;
    final go = world.spawn(
      name: 'a',
      transform: Pose(position: const Vec3(1, 1, 1), rotation: Quat.identity, scale: const Vec3.all(3)),
    );
    world.frame(1 / 60);
    expect(go.isPending, isFalse);

    go.setLocalPosition(5, 6, 7);
    expect(go.localTransform.position, const Vec3(5, 6, 7));
    expect(go.localTransform.scale, const Vec3.all(3), reason: 'scale seeded from the snapshot');

    go.setLocalScale(1, 2, 3);
    expect(go.localTransform.position, const Vec3(5, 6, 7), reason: 'position kept from the staged write');
    expect(go.localTransform.scale, const Vec3(1, 2, 3));

    world.frame(1 / 60);
    expect(go.localTransform.position, const Vec3(5, 6, 7));
    expect(go.localTransform.scale, const Vec3(1, 2, 3));
    expect(go.worldTransform!.position, const Vec3(5, 6, 7));

    final w = TransformWriter().setPosition(-1, 0, 0);
    go.writeLocal(w);
    w.setPosition(9, 9, 9); // the writer is reusable; the object keeps its own copy
    expect(go.localTransform.position, const Vec3(-1, 0, 0));
    world.frame(1 / 60);
    expect(go.worldTransform!.position, const Vec3(-1, 0, 0));
    world.dispose();
  });
}
