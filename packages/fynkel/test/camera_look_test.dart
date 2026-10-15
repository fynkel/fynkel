import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

/// A square glowing [glow] times white, a fifth of a 64 × 64 view across,
/// on black, seen at [quality] by a camera with [bloom].
({World world, HeadlessView view, Camera camera}) glowing(double glow, Quality quality) {
  final world = World()..quality = quality;
  final camera = Camera(clearColor: Color.black);
  world.add(
    GameObject(
      transform: Transform(position: Vec3(0, 0, 5)),
      components: [camera],
    ),
  );
  world.add(
    GameObject(
      transform: Transform(rotation: Quat.axisAngle(Vec3.right, 90.deg), scale: Vec3.all(1.2)),
      components: [
        MeshRenderer(Mesh.plane(), Material(color: Color.black, emissive: Color.white, emissiveIntensity: glow)),
      ],
    ),
  );
  final view = HeadlessView(world, width: 64, height: 64);
  addTearDown(view.dispose);
  addTearDown(world.dispose);
  return (world: world, view: view, camera: camera);
}

/// The red of the pixel 3 to the left of the square, in the middle row.
int beside(HeadlessView view) => view.read(view.frame()).rgba[(32 * 64 + 23) * 4];

void main() {
  test('a camera has bloom and ambient occlusion unless told not to', () {
    final camera = Camera();
    expect(camera.bloom, const Bloom());
    expect(camera.bloom, const Bloom(strength: 1, threshold: 1));
    expect(camera.ambientOcclusion, const AmbientOcclusion(intensity: 1, radius: 0.5));
    expect(Camera(bloom: null, ambientOcclusion: null).bloom, isNull);
    expect(const Bloom(strength: 0.5).toString(), 'Bloom(strength: 0.5, threshold: 1.0)');
    expect(const AmbientOcclusion(radius: 2).toString(), 'AmbientOcclusion(intensity: 1.0, radius: 2.0)');
    expect(const Bloom(strength: 2).copyWith(threshold: 3), const Bloom(strength: 2, threshold: 3));
  });

  test('light brighter than white glows at medium and high, not at low', () {
    for (final quality in [Quality.medium, Quality.high]) {
      final (:view, :camera, world: _) = glowing(8, quality);
      expect(beside(view), greaterThan(20), reason: '$quality');
      camera.bloom = null;
      expect(beside(view), 0, reason: 'none without it ($quality)');
      camera.bloom = const Bloom(strength: 3);
      expect(beside(view), greaterThan(40), reason: 'more with more strength ($quality)');
    }
    final (:view, camera: _, world: _) = glowing(8, Quality.low);
    expect(beside(view), 0);
  });

  test('what the engine refuses throws and changes nothing', () {
    final (:view, :camera, world: _) = glowing(8, Quality.high);
    expect(() => camera.bloom = const Bloom(strength: -1), throwsArgumentError);
    expect(camera.bloom, const Bloom());
    expect(() => camera.ambientOcclusion = const AmbientOcclusion(radius: double.nan), throwsArgumentError);
    expect(camera.ambientOcclusion, const AmbientOcclusion());
    expect(beside(view), greaterThan(20), reason: 'still glowing');
  });
}
