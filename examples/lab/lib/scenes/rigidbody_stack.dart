// Stage 5 step 4: a stack of ten boxes, and a ball to throw at it.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class RigidbodyStackScene extends LabScene {
  @override
  String get title => 'Rigidbodies: a stack';
  @override
  String get step => 'stage 5 step 4';
  @override
  String get about =>
      'Ten boxes, each a BoxCollider and a Rigidbody, stand on each other; they fall asleep once still. Throw a '
      'ball at them. "Interpolate" draws each between its last two steps, smooth however fast the frames come; '
      '"between frames" runs the step on its own thread after the frame (the default) rather than in it.';

  final boxes = <GameObject>[];
  GameObject? thrown;
  var smooth = true;

  @override
  void build() {
    add(floorCollider());
    for (var i = 0; i < 10; i++) {
      final color = i.isEven ? const Color(0xFFE0A33A) : const Color(0xFFD0D4DC);
      boxes.add(add(shape('box $i', Mesh.cube(), color, components: [BoxCollider(), Rigidbody()])));
    }
    rebuild();
  }

  void rebuild() {
    for (final (i, box) in boxes.indexed) {
      box.transform = Transform(position: Vec3(0, 0.5 + i, 0));
      box.get<Rigidbody>()!
        ..linearVelocity = Vec3.zero
        ..angularVelocity = Vec3.zero;
    }
    thrown?.destroy();
    thrown = null;
  }

  @override
  void onStart() {
    super.onStart();
    setSmooth(smooth);
  }

  void setSmooth(bool on) {
    smooth = on;
    final mode = on ? RigidbodyInterpolation.interpolate : RigidbodyInterpolation.none;
    for (final box in boxes) {
      box.get<Rigidbody>()!.interpolation = mode;
    }
    thrown?.get<Rigidbody>()!.interpolation = mode;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final asleep = boxes.where((b) => b.get<Rigidbody>()!.isSleeping).length;
    readout.value = [
      'rigidbodies: ${physics.rigidbodyCount}, boxes asleep: $asleep',
      'the top box is ${boxes.last.position.y.toStringAsFixed(2)} m up',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('throw a ball', () {
      thrown?.destroy();
      final body = Rigidbody(
        mass: 3,
        interpolation: smooth ? RigidbodyInterpolation.interpolate : RigidbodyInterpolation.none,
      );
      thrown = add(
        shape(
          'ball',
          Mesh.sphere(),
          const Color(0xFF3A8EE0),
          at: Vec3(0, 4, 6),
          size: Vec3(0.8, 0.8, 0.8),
          components: [SphereCollider(), body],
        ),
      );
      body.linearVelocity = Vec3(0, 2, -14);
      log.add('thrown at 14 m/s');
    }),
    Button('build it again', () {
      rebuild();
      log.add('built again');
    }),
    Toggle('interpolate', () => smooth, setSmooth),
    Toggle('between frames', () => physics.stepBetweenFrames, (on) => physics.stepBetweenFrames = on),
  ];
}
