// Step 7: cameras, the basics. Two virtual cameras on one cube, and the
// view blending between them.

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class CameraBasicsScene extends LabScene {
  @override
  String get title => 'Cameras: the basics';
  @override
  String get step => 'step 7';
  @override
  String get about =>
      'Two virtual cameras look at the cube: one in front, one from above. "Switch" gives the other the higher '
      'priority, and the view blends to it over a second (or jumps, with "cut"). "Orthographic" flattens the view. '
      '"Shake" shakes it.';

  late final VirtualCamera front, above;

  @override
  void build() {
    add(shape('cube', Mesh.cube(), const Color(0xFFE0613A), at: Vec3(0, 0.5, 0)));
    front = _virtual('front', Vec3(0, 2, 6), priority: 1);
    above = _virtual('above', Vec3(0, 8, 2), priority: 0);
  }

  VirtualCamera _virtual(String name, Vec3 at, {required int priority}) => add(
    GameObject(
      name: name,
      transform: Transform(position: at, rotation: Quat.lookRotation(Vec3(0, 0.5, 0) - at)),
      components: [VirtualCamera(priority: priority, orthographicSize: 3)],
    ),
  ).get<VirtualCamera>()!;

  void _switch() {
    final was = front.priority;
    front.priority = above.priority;
    above.priority = was;
    log.add('now ${front.priority > above.priority ? 'front' : 'above'} has the higher priority');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final live = world.cameras.live == front ? 'front' : 'above';
    final lens = world.camera!;
    readout.value = [
      'live: $live${world.cameras.isBlending ? ' (blending)' : ''}',
      'lens: ${lens.orthographic ? 'orthographic, orthographicSize ${lens.orthographicSize}' : 'perspective, fovY ${lens.fovY.inDegrees.round()}°'}',
      'blend: ${world.cameras.defaultBlend}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('switch', _switch),
    Toggle('cut', () => world.cameras.defaultBlend.isCut, (on) {
      world.cameras.defaultBlend = on ? Blend.cut : const Blend(1);
    }),
    Toggle('orthographic', () => world.camera!.orthographic, (on) {
      world.camera!.orthographic = on;
    }),
    Button('shake', () => world.cameras.shake(0.3)),
  ];
}
