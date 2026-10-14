// Stage 8: the look scene. Every phase of stage 8 is measured here, and it
// grows with them: shadows, lamps, a sky, glass, fog, glow. For now it has
// what there is (a sun, ambient light, metal and plastic, a glowing cube)
// and the quality switch: world.quality, or null for the device's own pick.
// Every level draws the same picture so far; above low, the scene goes
// through a half-float image and a last pass, which later effects use.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

class LookScene extends LabScene {
  @override
  String get title => 'Look: quality levels';
  @override
  String get step => 'stage 8';
  @override
  String get about =>
      'The scene stage 8 is measured in. Pick a quality level, or let the device pick: every level looks '
      'the same for now. Above low, the scene is drawn into a half-float image first, as bloom will need; the '
      'readout says which level is drawn and how fast.';

  late final GameObject _eye = find('camera')!;
  late final Quality device = Quality.device;
  var _turning = true, _angle = 0.0, _fps = '–';

  @override
  void build() {
    world.quality = Quality.values.where((q) => q.name == _startQuality).firstOrNull;
    // Metal along the back, plastic in front, from smooth to rough.
    for (var i = 0; i < 5; i++) {
      final roughness = 0.1 + i * 0.2;
      final x = (i - 2) * 1.4;
      add(
        GameObject(
          name: 'metal $i',
          transform: Transform(position: Vec3(x, 0.6, -1.2), scale: Vec3(1.1, 1.1, 1.1)),
          components: [
            MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFE8C070), metallic: 1, roughness: roughness)),
          ],
        ),
      );
      add(
        GameObject(
          name: 'plastic $i',
          transform: Transform(position: Vec3(x, 0.45, 0.6), scale: Vec3(0.9, 0.9, 0.9)),
          components: [MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFF4CA6E0), roughness: roughness))],
        ),
      );
    }
    // A ring of pillars, and a cube brighter than white: what bloom is for.
    const colours = [0xFFE0613A, 0xFFFFD040, 0xFF6CC070, 0xFFB070E0, 0xFFE070A0, 0xFF60D0D0];
    for (var i = 0; i < 12; i++) {
      final angle = i * 2 * math.pi / 12;
      add(
        shape(
          'pillar $i',
          Mesh.cube(),
          Color(colours[i % colours.length]),
          at: Vec3(7 * math.cos(angle), 1, 7 * math.sin(angle)),
          size: Vec3(0.6, 2, 0.6),
        ),
      );
    }
    add(
      GameObject(
        name: 'glow',
        transform: Transform(position: Vec3(0, 0.4, 2.4), scale: Vec3(0.6, 0.6, 0.6)),
        components: [
          MeshRenderer(
            Mesh.cube(),
            Material(color: Color.black, emissive: const Color(0xFFFF9040), emissiveIntensity: 4),
          ),
        ],
      ),
    );
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_turning) _angle += 0.2 * time.unscaledDelta;
    final at = Vec3(9 * math.sin(_angle), 4, 9 * math.cos(_angle));
    _eye
      ..position = at
      ..rotation = Quat.lookRotation(target - at);
    final picked = world.quality;
    final drawn = picked ?? device;
    readout.value = 'drawn at ${drawn.name}${picked == null ? ' (the device picks)' : ''}   $_fps fps';
  }

  @override
  List<Control> get controls => [
    for (final quality in Quality.values) Button(quality.name, () => world.quality = quality),
    Button("device's", () => world.quality = null),
    Toggle('turn', () => _turning, (on) => _turning = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('look', s));
}
