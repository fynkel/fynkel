import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class _EditingScript extends Behaviour {
  _EditingScript(this.target, this.log);
  final GameObject target;
  final List<String> log;
  bool failFixed = false;
  void write(String phase) => world.edit(() {
    target.name = phase;
    log.add(phase);
    expect(world.find(phase), same(target));
    if (phase == 'fixed' && failFixed) throw StateError('abort callback edit');
  });
  @override
  void onCreate() => write('create');
  @override
  void onEnable() => write('enable');
  @override
  void start() => write('start');
  @override
  void fixedUpdate(FixedUpdateContext context) => write('fixed');
  @override
  bool get wantsAfterPhysics => true;
  @override
  void afterPhysics(AfterPhysicsContext context) {
    if (failFixed) expect(target.name, 'start');
    write('after');
  }

  @override
  void update(UpdateContext context) => write('update');
  @override
  void lateUpdate(UpdateContext context) => write('late');
  @override
  void onDisable() => write('disable');
  @override
  void onDestroy() => write('destroy');
}

void main() {
  late World w;
  setUp(() => w = World.create());
  tearDown(() => w.dispose());

  test('explicit callback edits preserve lifecycle and phase order', () {
    final log = <String>[];
    final target = w.edit(() => w.spawn(name: 'target'));
    final b = w.edit(() => w.spawn().addBehaviour(_EditingScript(target, log)));
    w.frame(1 / 60);
    expect(log, ['create', 'enable', 'start', 'fixed', 'after', 'update', 'late']);
    expect(target.name, 'late');
    w.edit(() => b.destroy());
    w.frame(0);
    expect(log.sublist(7), ['disable', 'destroy']);
    expect(target.name, 'destroy');
  });

  test('a failed behaviour edit rolls back and later callbacks still run', () {
    final log = <String>[];
    final errors = <BehaviourError>[];
    w.onBehaviourError = errors.add;
    final target = w.edit(() => w.spawn(name: 'target'));
    w.edit(() => w.spawn().addBehaviour(_EditingScript(target, log)..failFixed = true));
    w.frame(1 / 60);
    expect(errors.single.callback, 'fixedUpdate');
    expect(target.name, 'late');
    expect(w.find('fixed'), isNull);
    expect(log, containsAllInOrder(['fixed', 'after', 'update', 'late']));
  });

  test('host edits execute before physics and update effects wait for next tick', () {
    final body = w.edit(
      () => w.spawn()
        ..set(const RigidBody(bodyType: BodyType.dynamic, gravityScale: 0, linearDamping: 0))
        ..set(const Collider()),
    );
    w.frame(1 / 60);
    w.frame(
      1 / 60,
      fixedUpdate: (_) => w.edit(() {
        expect(body.bodyState()!.linearVelocity.x, 0);
        body.setLinearVelocity(const Vec3(3, 0, 0));
        expect(body.bodyState()!.linearVelocity.x, 0);
      }),
      update: () => w.edit(() {
        expect(body.bodyState()!.linearVelocity.x, 3);
        body.setLinearVelocity(const Vec3(5, 0, 0));
      }),
    );
    expect(body.bodyState()!.linearVelocity.x, 3);
    w.frame(1 / 60);
    expect(body.bodyState()!.linearVelocity.x, 5);
  });

  test('host rollback preserves later callbacks and pending legacy work', () {
    final target = w.edit(() => w.spawn(name: 'original'));
    var updated = false;
    expect(
      () => w.frame(
        1 / 60,
        fixedUpdate: (_) => w.edit(() {
          target.name = 'discard';
          throw StateError('abort');
        }),
        update: () => w.edit(() {
          expect(target.name, 'original');
          target.name = 'update';
          updated = true;
        }),
      ),
      throwsStateError,
    );
    expect(updated, isTrue);
    expect(target.name, 'update');
    var entered = false;
    w.frame(
      1 / 60,
      fixedUpdate: (_) {
        target.name = 'legacy';
        w.edit(() {
          expect(target.name, 'legacy');
          entered = true;
        });
      },
    );
    expect(entered, isTrue);
    expect(target.name, 'legacy');
  });

  test('zero and multiple tick frames keep explicit edits at their requested phase', () {
    for (final hz in [30, 60, 120, 240]) {
      final world = World.create();
      try {
        final go = world.edit(() => world.spawn()..set(const Layer()));
        var ticks = 0, frames = 0;
        for (var i = 0; i < hz; i++) {
          world.frame(
            1 / hz,
            fixedUpdate: (_) => world.edit(() {
              ticks++;
              go.set(Layer(layer: (go.get<Layer>()!.layer + 1) % 32));
            }),
            update: () => world.edit(() => frames++),
          );
        }
        expect(ticks, 60);
        expect(frames, hz);
        expect(go.get<Layer>()!.layer, 60 % 32);
      } finally {
        world.dispose();
      }
    }
  });
}
