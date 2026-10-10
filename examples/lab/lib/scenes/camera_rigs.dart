// Step 7: cameras with rigs. A ball runs round a square; three virtual
// cameras watch it, each with its own rig, and taps on the floor find the
// spot under the finger.

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class CameraRigsScene extends LabScene {
  @override
  String get title => 'Cameras: rigs';
  @override
  String get step => 'step 7';
  @override
  String get about =>
      'The green ball runs round a square. Pick a camera: "chase" follows it (Follow and LookAt), "orbit" circles it '
      '(drag to turn, pinch or scroll to zoom), "corner" stands still and turns to watch it (LookAt). Damping makes '
      'the chase lag; look-ahead puts it where the ball is going. Tap the floor: a marker goes where the finger is '
      '(screenToPlane).';

  late final GameObject ball, marker;
  late final VirtualCamera chase, orbit, corner;
  late final Follow follow;

  @override
  void build() {
    ball = add(shape('ball', Mesh.sphere(), const Color(0xFF7BC86C), at: Vec3(-3, 0.4, -3), size: Vec3.one * 0.8));
    marker = add(shape('marker', Mesh.cube(), const Color(0xFFFFC020), at: Vec3(0, -1, 0), size: Vec3(0.3, 0.1, 0.3)));
    for (final x in [-4.5, 4.5]) {
      for (final z in [-4.5, 4.5]) {
        add(shape('post', Mesh.cube(), const Color(0xFF8A6A50), at: Vec3(x, 0.5, z), size: Vec3(0.3, 1, 0.3)));
      }
    }
    follow = Follow(ball, offset: Vec3(0, 3, 5));
    chase = _camera('chase', 1, [follow, LookAt(ball)]);
    orbit = _camera('orbit', 0, [Orbit(ball, distance: 7)]);
    corner = _camera('corner', 0, [LookAt(ball)], at: Vec3(6, 5, 6));
  }

  VirtualCamera _camera(String name, int priority, List<Component> rigs, {Vec3 at = Vec3.zero}) => add(
    GameObject(
      name: name,
      transform: Transform(position: at),
      components: [
        VirtualCamera(priority: priority),
        ...rigs,
      ],
    ),
  ).get<VirtualCamera>()!;

  @override
  void onStart() {
    super.onStart();
    ball.sequence([
      for (final corner in [Vec3(3, 0.4, -3), Vec3(3, 0.4, 3), Vec3(-3, 0.4, 3), Vec3(-3, 0.4, -3)])
        ball.moveTo(corner, speed: 2.5, ease: Ease.linear),
    ], loops: -1);
    on(input.onTap, (finger) {
      final spot = world.camera!.screenToPlane(finger.position, Plane.ground);
      if (spot == null) return;
      marker.position = spot + Vec3(0, 0.05, 0);
      log.add('tap: the floor at (${spot.x.toStringAsFixed(1)}, ${spot.z.toStringAsFixed(1)})');
    });
  }

  void _pick(VirtualCamera camera) {
    for (final c in [chase, orbit, corner]) {
      c.priority = c == camera ? 1 : 0;
    }
    log.add('${camera.gameObject.name} is live');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final live = world.cameras.live?.gameObject.name ?? '–';
    final onScreen = input.width > 0 ? world.camera!.worldToScreen(ball.position) : null;
    readout.value = [
      'live: $live${world.cameras.isBlending ? ' (blending)' : ''}',
      'the ball on screen: ${onScreen == null ? 'behind' : '(${onScreen.x.round()}, ${onScreen.y.round()})'}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('chase', () => _pick(chase)),
    Button('orbit', () => _pick(orbit)),
    Button('corner', () => _pick(corner)),
    Slider('chase damping', () => follow.damping, (v) => follow.damping = v, max: 2),
    Slider('chase look-ahead', () => follow.lookAhead, (v) => follow.lookAhead = v, max: 1),
    Toggle('cut', () => world.cameras.defaultBlend.isCut, (on) {
      world.cameras.defaultBlend = on ? Blend.cut : const Blend(1);
    }),
    Button('shake', () => world.cameras.shake(0.3)),
  ];
}
