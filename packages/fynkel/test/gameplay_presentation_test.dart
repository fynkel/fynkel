import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  late World world;
  late GameObject object;
  setUp(() {
    world = World.create();
    object = world.edit(() => world.spawn());
  });
  tearDown(() => world.dispose());

  test('all persistent presentation transform setters commit and abort without leaking a replay', () {
    final setters = <void Function()>[
      () => object.setPresentationTransform(const Pose(position: Vec3(4, 0, 0))),
      () => object.writePresentation(TransformWriter().setPosition(4, 0, 0)),
      () => object.setPresentationPosition(4, 0, 0),
      () => object.setPresentation(const Pose(position: Vec3(4, 0, 0))),
    ];
    for (final setter in setters) {
      world.edit(() => object.transform.position = Vec3.zero);
      expect(
        () => world.edit(() {
          setter();
          expect(object.transform.position.x, 4);
          throw StateError('abort');
        }),
        throwsStateError,
      );
      expect(object.transform.position.x, 0);
      world.frame(1 / 60);
      expect(object.transform.position.x, 0);
      world.edit(setter);
      expect(object.transform.position.x, 4);
      world.edit(() => object.transform.position = const Vec3(7, 0, 0));
      world.frame(1 / 60);
      expect(object.transform.position.x, 7, reason: 'no older presentation replay overwrites the edit');
    }
  });

  test('automatic update presentation writes and plain data share callback rollback', () {
    world.frame(
      0,
      update: () {
        object.setPresentationPosition(3, 0, 0);
        object.setPresentation(const Name(value: 'committed'));
      },
    );
    expect(object.transform.position.x, 3);
    expect(object.name, 'committed');
    expect(
      () => world.frame(
        0,
        update: () {
          object.setPresentationPosition(8, 0, 0);
          object.setPresentation(const Name(value: 'discarded'));
          throw StateError('abort');
        },
      ),
      throwsStateError,
    );
    expect(object.transform.position.x, 3);
    expect(object.name, 'committed');
  });

  test('staged dynamic ownership rejects presentation transforms stickily', () {
    expect(
      () => world.edit(() {
        object.set(const RigidBody(bodyType: BodyType.dynamic));
        object.name = 'discarded';
        try {
          object.setPresentationPosition(4, 0, 0);
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(object.get<RigidBody>(), isNull);
    expect(object.name, isNot('discarded'));
    expect(object.transform.position.x, 0);
  });
}
