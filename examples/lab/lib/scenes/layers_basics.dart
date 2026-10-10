// Step 8: layers and debug drawing, the basics. A cube on layer 1, one on
// layer 0, and a ray through both, drawn every frame.

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

/// The blue cube's layer. Name layers with constants like this.
const blueLayer = 1;

class LayersBasicsScene extends LabScene {
  @override
  String get title => 'Layers and debug: the basics';
  @override
  String get step => 'step 8';
  @override
  String get about =>
      'The blue cube is on layer 1, the orange one on layer 0. A ray (the yellow line) goes through both, and a '
      'circle marks what it hits. "Ray hits layer 1" off lets it go through the blue cube; "camera draws layer 1" '
      'off hides it. "Box" draws a box round it for two seconds.';

  late final GameObject blue, orange;
  var rayHitsBlue = true;
  final ray = Ray(Vec3(-4, 0.5, 0), Vec3.right);

  @override
  void build() {
    blue = add(shape('blue cube', Mesh.cube(), const Color(0xFF3A8EE0), at: Vec3(-1, 0.5, 0)))..layer = blueLayer;
    orange = add(shape('orange cube', Mesh.cube(), const Color(0xFFE0613A), at: Vec3(1.5, 0.5, 0)));
  }

  Camera get _lens => world.camera!.get<Camera>()!;

  @override
  void onUpdate() {
    super.onUpdate();
    final layers = rayHitsBlue ? LayerMask.all : LayerMask.except([blueLayer]);
    final hit = world.raycast(ray, maxDistance: 10, layers: layers);
    final end = hit?.point ?? ray.at(10);
    debug.line(ray.origin, end, color: const Color(0xFFFFE000));
    if (hit != null) debug.sphere(hit.point, 0.15, color: const Color(0xFFFFE000));
    readout.value = [
      'the ray hits: ${hit?.object.name ?? 'nothing'}',
      'it hits ${layers == LayerMask.all ? 'every layer' : 'all but layer $blueLayer'}',
      'the camera draws ${_lens.cullingMask == LayerMask.all ? 'every layer' : 'all but layer $blueLayer'}',
      'debug lines this frame: ${debug.count}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Toggle('ray hits layer 1', () => rayHitsBlue, (on) => rayHitsBlue = on),
    Toggle('camera draws layer 1', () => _lens.cullingMask.contains(blueLayer), (on) {
      _lens.cullingMask = on ? LayerMask.all : LayerMask.except([blueLayer]);
    }),
    Toggle('debug lines', () => debug.enabled, (on) => debug.enabled = on),
    Button('box', () {
      debug.box(blue.transform.position, Vec3(1.4, 1.4, 1.4), color: const Color(0xFF4CC26B), seconds: 2);
      log.add('a box round the blue cube for 2 s');
    }),
  ];
}
