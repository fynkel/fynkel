// Stage 8, phase 3: shadows. A late-afternoon sun casting long shadows
// over a garden of pillars, trees, an arch and a picket fence, with a row
// of pillars running off 90 m so the far cascades show; and a lamp post's
// spotlight casting its own. The camera drifts round, so a shadow that
// shimmered (its edge crawling as the camera moves) would show.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// `--dart-define=LAB_SHADOWS=false` starts the scene with no shadows, for
/// measuring what they cost.
const _startShadows = bool.fromEnvironment('LAB_SHADOWS', defaultValue: true);

class ShadowsScene extends LabScene {
  @override
  String get title => 'Look: shadows';
  @override
  String get step => 'stage 8';
  @override
  String get about =>
      "The sun's shadows, sharp near the camera and still there far off, and a lamp post's spotlight casting its "
      'own. The camera drifts: shadow edges should stay put. Low draws one hard-edged map for the sun, medium two '
      'soft ones, high three softer ones.';

  @override
  Vec3 get eye => Vec3(0, 5, 12);
  @override
  Vec3 get target => Vec3(0, 0.5, 0);

  late final Light _sun, _torch;
  late final GameObject _camera;
  var _moving = true, _night = false, _time = 0.0, _fps = '–';

  @override
  void build() {
    world.quality = Quality.levels.where((q) => q.name == _startQuality).firstOrNull;
    _camera = find('camera')!;
    // A low sun, for long shadows, over a big lawn.
    final sun = find('sun')!..rotation = Quat.lookRotation(Vec3(-0.6, -0.7, -0.45));
    _sun = sun.get<Light>()!
      ..intensity = 2.2
      ..shadows = _startShadows;
    find('floor')!.active = false;
    add(shape('lawn', Mesh.plane(), const Color(0xFF6E8A5A), size: Vec3(240, 1, 240)));

    final stone = const Color(0xFFC9C2B4), wood = const Color(0xFF8A6A48), leaf = const Color(0xFF4F7A3A);
    // Pillars round the middle.
    for (var i = 0; i < 6; i++) {
      final angle = i * math.pi / 3;
      add(
        shape(
          'pillar $i',
          Mesh.cylinder(),
          stone,
          at: Vec3(4 * math.cos(angle), 1.5, 4 * math.sin(angle)),
          size: Vec3(0.5, 1.5, 0.5),
        ),
      );
    }
    // An arch.
    add(shape('arch left', Mesh.cube(), stone, at: Vec3(-1.2, 1.25, -1), size: Vec3(0.4, 2.5, 0.4)));
    add(shape('arch right', Mesh.cube(), stone, at: Vec3(1.2, 1.25, -1), size: Vec3(0.4, 2.5, 0.4)));
    add(shape('arch top', Mesh.cube(), stone, at: Vec3(0, 2.7, -1), size: Vec3(2.8, 0.4, 0.4)));
    // A picket fence: thin posts, for fine shadows.
    for (var i = 0; i < 24; i++) {
      add(shape('picket $i', Mesh.cube(), wood, at: Vec3(-3.5 + i * 0.3, 0.45, 2.5), size: Vec3(0.08, 0.9, 0.04)));
    }
    add(shape('rail', Mesh.cube(), wood, at: Vec3(0, 0.7, 2.53), size: Vec3(7.2, 0.08, 0.03)));
    // Trees.
    final random = math.Random(3);
    for (var i = 0; i < 10; i++) {
      final at = Vec3(random.nextDouble() * 30 - 15, 0, random.nextDouble() * -25 - 6);
      final tall = 2 + random.nextDouble() * 2;
      add(
        GameObject(
          name: 'tree $i',
          transform: Transform(position: at),
          children: [
            shape('trunk', Mesh.cylinder(), wood, at: Vec3(0, tall / 2, 0), size: Vec3(0.3, tall / 2, 0.3)),
            shape('crown', Mesh.sphere(), leaf, at: Vec3(0, tall + 0.6, 0), size: Vec3.one * 2.2),
          ],
        ),
      );
    }
    // An avenue of pillars, 90 m long: shadows still there far off.
    for (var z = -8.0; z > -100; z -= 6) {
      for (final x in [-6.0, 6.0]) {
        add(shape('avenue', Mesh.cube(), stone, at: Vec3(x, 2, z), size: Vec3(0.6, 4, 0.6)));
      }
    }
    // A ball and a box under the lamp post.
    add(shape('ball', Mesh.sphere(), const Color(0xFFD04A3A), at: Vec3(3, 0.5, 5.6)));
    add(shape('crate', Mesh.cube(), wood, at: Vec3(4.6, 0.4, 6.2), size: Vec3.one * 0.8));

    // The lamp post: a pole, and a spotlight on top shining down past it.
    add(shape('lamp post', Mesh.cylinder(), const Color(0xFF30343A), at: Vec3(4.4, 2, 4), size: Vec3(0.12, 2, 0.12)));
    final head = add(
      GameObject(
        name: 'torch',
        transform: Transform(position: Vec3(4.2, 4.1, 4.3), rotation: Quat.lookRotation(Vec3(-0.35, -1, 0.45))),
        components: [
          Light.spot(color: const Color(0xFFFFD9A0), intensity: 40, range: 12, angle: 70.deg, shadows: _startShadows),
        ],
      ),
    );
    _torch = head.get<Light>()!;
    night = false;
  }

  /// Night: the sun off, the sky dark, so the spotlight's shadows show.
  bool get night => _night;
  set night(bool on) {
    _night = on;
    _sun.intensity = on ? 0 : 2.2;
    find('ambient')!.get<Light>()!.intensity = on ? 0.05 : 0.5;
    _camera.get<Camera>()!.clearColor = on ? const Color(0xFF05060A) : const Color(0xFF8FB4D8);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_moving) _time += time.unscaledDelta;
    // A slow drift round the garden, never quite repeating.
    final at = eye + Vec3(5 * math.sin(_time * 0.21), 1.5 * math.sin(_time * 0.13), 3 * math.sin(_time * 0.17));
    final look = target + Vec3(2 * math.sin(_time * 0.11), 0, 0);
    _camera
      ..position = at
      ..rotation = Quat.lookRotation(look - at);

    final picked = world.quality;
    final drawn = picked ?? Quality.device;
    final which = [if (_sun.shadows) 'sun', if (_torch.shadows) 'spotlight'];
    readout.value =
        'drawn at ${drawn.name}${picked == null ? ' (the device picks)' : ''}   '
        'shadows: ${which.isEmpty ? 'none' : which.join(' and ')}   $_fps fps';
  }

  @override
  List<Control> get controls => [
    for (final quality in Quality.levels) Button(quality.name, () => world.quality = quality),
    Button("device's", () => world.quality = null),
    Toggle('sun shadows', () => _sun.shadows, (on) => _sun.shadows = on),
    Toggle('spotlight shadows', () => _torch.shadows, (on) => _torch.shadows = on),
    Toggle('night', () => night, (on) => night = on),
    Toggle('move', () => _moving, (on) => _moving = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('shadows', s));
}
