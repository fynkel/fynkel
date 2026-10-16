// Stage 8, phase 4: the sky and what reflects it. Metal balls from a
// mirror to fully rough under a Poly Haven sky (assets/sky.hdr): the
// smooth ones show the clouds, the rough ones only the sky's colour. A
// gradient day and night, a plain colour, and fog that fades an avenue of
// pillars 100 m long into the horizon.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// `--dart-define=LAB_SKY=day` (or night, plain, none) starts the scene
/// with that sky, for measuring; the image otherwise.
const _startSky = String.fromEnvironment('LAB_SKY', defaultValue: 'image');

/// `--dart-define=LAB_FOG=false` starts the scene without fog.
const _startFog = bool.fromEnvironment('LAB_FOG', defaultValue: true);

/// The skies the page switches between.
const skies = ['image', 'day', 'night', 'plain', 'none'];

class SkyScene extends LabScene {
  @override
  String get title => 'Look: sky and fog';
  @override
  String get step => 'stage 8';
  @override
  String get about =>
      'Metal balls from a mirror to fully rough: the smooth ones show the sky, the rough ones only its colour. '
      'The sky is a Poly Haven image, a gradient day or night, or a plain colour; fog fades the avenue into it.';

  @override
  Vec3 get eye => Vec3(0, 2, 7);
  @override
  Vec3 get target => Vec3(0, 1, 0);

  late final GameObject _camera;
  late final Light _sun;
  SkyImage? _image;
  var _sky = 'day', _fog = _startFog, _turning = true, _angle = 0.0, _turn = 0.0, _fps = '–';

  @override
  void build() {
    world.quality = Quality.levels.where((q) => q.name == _startQuality).firstOrNull;
    _camera = find('camera')!;
    _sun = find('sun')!.get<Light>()!;
    // The sky lights the scene; no ambient light on top.
    find('ambient')!.get<Light>()!.intensity = 0;
    find('floor')!.active = false;
    add(shape('ground', Mesh.plane(), const Color(0xFF8A8478), size: Vec3(300, 1, 300)));

    // Metal: a mirror on the left to fully rough on the right; plastic in
    // front of it the same way.
    for (var i = 0; i < 5; i++) {
      final roughness = i / 4;
      final x = (i - 2) * 1.5;
      add(
        GameObject(
          name: 'metal $i',
          transform: Transform(position: Vec3(x, 1.2, -1), scale: Vec3.all(1.2)),
          components: [
            MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFF0F0F0), metallic: 1, roughness: roughness)),
          ],
        ),
      );
      add(
        GameObject(
          name: 'plastic $i',
          transform: Transform(position: Vec3(x, 0.45, 1), scale: Vec3.all(0.9)),
          components: [MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFE0613A), roughness: roughness))],
        ),
      );
    }
    // An avenue of pillars 100 m long, for the fog to fade.
    for (var z = -6.0; z > -110; z -= 8) {
      for (final x in [-5.0, 5.0]) {
        add(shape('avenue', Mesh.cube(), const Color(0xFFC9C2B4), at: Vec3(x, 2, z), size: Vec3(0.8, 4, 0.8)));
      }
    }
    fog = _fog;
    sky = _startSky == 'image' ? 'day' : _startSky;
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    _image = await assets.skyImage('sky.hdr');
    if (_startSky == 'image') sky = 'image';
  }

  /// Which of [skies] is shown. The image waits for its file.
  String get sky => _sky;
  set sky(String which) {
    final image = _image;
    if (which == 'image' && image == null) return;
    _sky = which;
    // The sun matches the sky: low and warm in the image, white by day,
    // the moon's pale blue at night.
    _sun.color = switch (which) {
      'image' => const Color(0xFFFFE8C8),
      'night' => Color.rgb(0.65, 0.75, 1),
      _ => Color.white,
    };
    _sun.intensity = switch (which) {
      'image' => 2.5,
      'night' => 0.4,
      _ => 2,
    };
    world.sky = switch (which) {
      'image' => Sky.image(image!, rotation: _turn),
      'day' => Sky.gradient(
        zenith: const Color(0xFF3F7CC8),
        horizon: const Color(0xFFD3E4F0),
        ground: const Color(0xFF6E6A60),
      ),
      'night' => Sky.gradient(
        zenith: const Color(0xFF04060E),
        horizon: const Color(0xFF1E2840),
        ground: const Color(0xFF07080A),
      ),
      'plain' => Sky.color(const Color(0xFF606870)),
      _ => null,
    };
  }

  /// Fog from 10 m to 90 m, in the sky's horizon.
  bool get fog => _fog;
  set fog(bool on) {
    _fog = on;
    world.fog = on ? const Fog(start: 10, end: 90) : null;
  }

  /// How far the image sky is turned, in radians.
  double get turn => _turn;
  set turn(double radians) {
    _turn = radians;
    if (_sky == 'image') sky = 'image';
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_turning) _angle += 0.15 * time.unscaledDelta;
    // Swinging from side to side in front of the balls, so the reflections
    // move over them.
    final at = Vec3(4 * math.sin(_angle), 2, 7);
    _camera
      ..position = at
      ..rotation = Quat.lookRotation(target - at);
    final picked = world.quality;
    final drawn = picked ?? Quality.device;
    readout.value =
        'drawn at ${drawn.name}${picked == null ? ' (the device picks)' : ''}   sky: $_sky'
        '${_fog ? ' with fog' : ''}   $_fps fps';
  }

  @override
  List<Control> get controls => [
    for (final quality in Quality.levels) Button(quality.name, () => world.quality = quality),
    Button("device's", () => world.quality = null),
    for (final which in skies) Button('sky: $which', () => sky = which),
    Toggle('fog', () => fog, (on) => fog = on),
    Slider('turn the image', () => turn, (radians) => turn = radians, max: 2 * math.pi),
    Toggle('swing', () => _turning, (on) => _turning = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('sky', s));
}
