// Stage 8, phase 6: bloom and ambient occlusion. Lamps whose bulbs glow,
// a ring of glowing blocks turning, metal balls the sun shines on, and
// crates stacked on the ground and against a wall, darker where they meet.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// `--dart-define=LAB_BLOOM=false` and `LAB_AO=false` start the scene
/// without them, for measuring what they cost.
const _startBloom = bool.fromEnvironment('LAB_BLOOM', defaultValue: true);
const _startAo = bool.fromEnvironment('LAB_AO', defaultValue: true);

class GlowScene extends LabScene {
  @override
  String get title => 'Look: glow and contact';
  @override
  String get step => 'stage 8';
  @override
  String get about =>
      'Bulbs and a ring of blocks glow brighter than white, and so does the sun on the metal balls (bloom, at medium '
      'and high). Crates darken where they meet the ground, the wall and each other (ambient occlusion, at high).';

  @override
  Vec3 get eye => Vec3(0, 3.2, 8.5);
  @override
  Vec3 get target => Vec3(0, 1, 0);

  late final Camera _camera;
  final _ring = <GameObject>[];
  var _turning = true, _angle = 0.0, _fps = '–';

  @override
  void build() {
    world.quality = Quality.values.where((q) => q.name == _startQuality).firstOrNull;
    _camera = find('camera')!.get<Camera>()!;
    _camera
      ..bloom = _startBloom ? const Bloom() : null
      ..ambientOcclusion = _startAo ? const AmbientOcclusion() : null;
    world.sky = Sky.gradient(
      zenith: const Color(0xFF1B2440),
      horizon: const Color(0xFF6A5A78),
      ground: const Color(0xFF2A2630),
      lighting: 0.5,
    );
    find('sun')!.get<Light>()!.intensity = 1.2;

    // A wall behind, and crates: on the ground, against the wall, on each
    // other.
    add(shape('wall', Mesh.cube(), const Color(0xFFB8B0A0), at: Vec3(0, 1.5, -2.5), size: Vec3(12, 3, 0.4)));
    final crate = const Color(0xFFC89B62);
    add(shape('crate 0', Mesh.cube(), crate, at: Vec3(-3, 0.5, -1.7), size: Vec3.all(1)));
    add(shape('crate 1', Mesh.cube(), crate, at: Vec3(-1.9, 0.4, -1.8), size: Vec3.all(0.8)));
    add(shape('crate 2', Mesh.cube(), crate, at: Vec3(-2.6, 1.3, -1.8), size: Vec3.all(0.6)));
    add(shape('crate 3', Mesh.cube(), crate, at: Vec3(3, 0.35, 0.8), size: Vec3.all(0.7)));

    // Metal balls: the sun's highlight on them is brighter than white.
    for (var i = 0; i < 3; i++) {
      add(
        GameObject(
          name: 'ball $i',
          transform: Transform(position: Vec3(0.8 + i * 0.9, 0.35, 1.6 - i * 0.4), scale: Vec3.all(0.7)),
          components: [
            MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFE8D8B0), metallic: 1, roughness: 0.15)),
          ],
        ),
      );
    }

    // Lamps: a post, and a bulb that glows and lights round it.
    for (final (i, (x, colour)) in [(-4.2, const Color(0xFFFFC46B)), (4.2, const Color(0xFF7FD8FF))].indexed) {
      add(shape('post $i', Mesh.cube(), const Color(0xFF30333A), at: Vec3(x, 1, -1), size: Vec3(0.12, 2, 0.12)));
      add(
        GameObject(
          name: 'bulb $i',
          transform: Transform(position: Vec3(x, 2.15, -1), scale: Vec3.all(0.3)),
          components: [
            Light.point(color: colour, intensity: 2, range: 5),
            MeshRenderer(Mesh.sphere(), Material(color: Color.black, emissive: colour, emissiveIntensity: 6)),
          ],
        ),
      );
    }

    // A ring of glowing blocks, turning above the middle.
    final glow = Material(color: Color.black, emissive: const Color(0xFFFF4FA0), emissiveIntensity: 4);
    for (var i = 0; i < 10; i++) {
      _ring.add(
        add(
          GameObject(
            name: 'block $i',
            transform: Transform(scale: Vec3.all(0.16)),
            components: [MeshRenderer(Mesh.cube(), glow)],
          ),
        ),
      );
    }
  }

  /// Whether the camera has bloom, and how strong.
  bool get bloom => _camera.bloom != null;
  set bloom(bool on) => _camera.bloom = on ? const Bloom() : null;
  double get strength => _camera.bloom?.strength ?? 0;
  set strength(double value) => _camera.bloom = (_camera.bloom ?? const Bloom()).copyWith(strength: value);

  /// Whether it has ambient occlusion, and how dark.
  bool get ambientOcclusion => _camera.ambientOcclusion != null;
  set ambientOcclusion(bool on) => _camera.ambientOcclusion = on ? const AmbientOcclusion() : null;
  double get intensity => _camera.ambientOcclusion?.intensity ?? 0;
  set intensity(double value) =>
      _camera.ambientOcclusion = (_camera.ambientOcclusion ?? const AmbientOcclusion()).copyWith(intensity: value);

  @override
  void onUpdate() {
    super.onUpdate();
    if (_turning) _angle += 0.6 * time.unscaledDelta;
    for (final (i, block) in _ring.indexed) {
      final a = _angle + i / _ring.length * 2 * math.pi;
      block
        ..position = Vec3(1.1 * math.cos(a), 2.3 + 0.15 * math.sin(2 * a), 1.1 * math.sin(a))
        ..rotation = Quat.axisAngle(Vec3.up, a);
    }
    final picked = world.quality;
    final drawn = picked ?? Quality.device;
    String state(bool on, bool drawnHere) => !on ? 'off' : (drawnHere ? 'on' : 'on, not drawn at ${drawn.name}');
    readout.value =
        'drawn at ${drawn.name}${picked == null ? ' (the device picks)' : ''}   '
        'bloom ${state(bloom, drawn != Quality.low)}   '
        'ambient occlusion ${state(ambientOcclusion, drawn == Quality.high)}   $_fps fps';
  }

  @override
  List<Control> get controls => [
    for (final quality in Quality.values) Button(quality.name, () => world.quality = quality),
    Button("device's", () => world.quality = null),
    Toggle('bloom', () => bloom, (on) => bloom = on),
    Slider('bloom strength', () => strength, (value) => strength = value, max: 3),
    Toggle('ambient occlusion', () => ambientOcclusion, (on) => ambientOcclusion = on),
    Slider('occlusion intensity', () => intensity, (value) => intensity = value, max: 4),
    Toggle('turn the ring', () => _turning, (on) => _turning = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('glow', s));
}
