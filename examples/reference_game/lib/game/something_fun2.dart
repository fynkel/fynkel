import 'package:fynkel/fynkel.dart';

/// What SomethingFun2 does (the Inspector shows this comment).
@FynkelBehaviour(id: 'game.SomethingFun2')
class SomethingFun2 extends Behaviour {
  @Expose(min: 0, step: 0.1, tooltip: 'Units per second')
  double speed = 5.0;

  @Expose(min: 0, step: 0.1, tooltip: 'Units per second')
  double speed2 = 5.0;

  @Expose(min: 0, step: 0.1, tooltip: 'Units per second')
  double speed3 = 5.0;

  @override
  void start() {}

  @override
  void update(UpdateContext context) {}

  @override
  void fixedUpdate(FixedUpdateContext context) {
    transform.position =
        transform.position + Vec3(1, 0, 0) * (speed * context.dt);
  }
}
