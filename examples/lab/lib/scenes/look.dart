// Stage 8: the look scene, with all of it together. A courtyard under the
// sky image by day: a sun with shadows, fog, metal and plastic, glass and
// a pool, a ring of glowing blocks, crates darker where they meet. By
// night the sun goes, the sky goes dark, and lamps round the pool and a
// spotlight on its post light it, the bulbs glowing. The quality switch
// is world.quality, or null for the device's own pick.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// `--dart-define=LAB_NIGHT=true` starts it at night, for measuring.
const _startNight = bool.fromEnvironment('LAB_NIGHT');

class LookScene extends LabScene {
  @override
  String get title => 'Look: everything';
  @override
  String get step => 'stage 8';
  @override
  String get about =>
      "All of stage 8's look at once: the sky, the sun's shadows and fog by day; lamps, a spotlight's shadows and "
      'glowing bulbs by night; glass, water, glow and darker corners at any time. Pick a quality level, or let the '
      'device pick; the readout says which is drawn and how fast.';

  @override
  Vec3 get eye => Vec3(0, 5, 13);
  @override
  Vec3 get target => Vec3(0, 1, 0);

  late final GameObject _eye = find('camera')!;
  late final Light _sun, _ambient, _spot;
  final _lamps = <Light>[];
  final _bulbs = <Material>[];
  final _ring = <GameObject>[];
  SkyImage? _image;
  var _night = false, _fog = true, _turning = true, _angle = 0.0, _spin = 0.0, _fps = '–';

  @override
  Future<void> onLoad() async {
    super.onLoad();
    _image = await assets.skyImage('sky.hdr');
    night = _night;
  }

  @override
  void build() {
    world.quality = Quality.values.where((q) => q.name == _startQuality).firstOrNull;
    _sun = find('sun')!.get<Light>()!..shadows = true;
    find('sun')!.rotation = Quat.lookRotation(Vec3(-0.5, -0.75, -0.45));
    _ambient = find('ambient')!.get<Light>()!;
    find('floor')!.active = false;

    final stone = const Color(0xFFC9C2B4), paving = const Color(0xFF9A948A), wood = const Color(0xFF9A7448);
    add(shape('paving', Mesh.plane(), paving, size: Vec3(60, 1, 60)));
    // A wall behind, and crates against it and on each other.
    add(shape('wall', Mesh.cube(), stone, at: Vec3(0, 1.5, -5), size: Vec3(18, 3, 0.5)));
    add(shape('crate 0', Mesh.cube(), wood, at: Vec3(-5.5, 0.5, -4.2), size: Vec3.all(1)));
    add(shape('crate 1', Mesh.cube(), wood, at: Vec3(-4.4, 0.4, -4.3), size: Vec3.all(0.8)));
    add(shape('crate 2', Mesh.cube(), wood, at: Vec3(-5.2, 1.3, -4.3), size: Vec3.all(0.6)));
    // Pillars down both sides.
    for (var i = 0; i < 4; i++) {
      for (final x in [-7.0, 7.0]) {
        add(shape('pillar', Mesh.cylinder(), stone, at: Vec3(x, 1.75, -3 + i * 3), size: Vec3(0.4, 1.75, 0.4)));
      }
    }
    // Metal at the back, plastic in front, from smooth to rough.
    for (var i = 0; i < 5; i++) {
      final roughness = 0.1 + i * 0.2;
      final x = (i - 2) * 1.3;
      add(
        GameObject(
          name: 'metal $i',
          transform: Transform(position: Vec3(x, 0.5, -2.6), scale: Vec3.all(1)),
          components: [
            MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFE8C070), metallic: 1, roughness: roughness)),
          ],
        ),
      );
      add(shape('plastic $i', Mesh.sphere(), const Color(0xFF4CA6E0), at: Vec3(x, 0.35, 4.4), size: Vec3.all(0.7)))
              .get<MeshRenderer>()!
              .material!
              .roughness =
          roughness;
    }

    // A pool, its water see-through, and two panes of tinted glass.
    add(shape('pool rim', Mesh.cube(), stone, at: Vec3(0, 0.1, 1), size: Vec3(6, 0.2, 3.6)));
    add(shape('pool floor', Mesh.cube(), const Color(0xFF2E6A78), at: Vec3(0, 0.15, 1), size: Vec3(5.4, 0.2, 3)));
    add(
      GameObject(
        name: 'water',
        transform: Transform(position: Vec3(0, 0.45, 1), scale: Vec3(5.4, 1, 3)),
        components: [
          MeshRenderer(Mesh.plane(), Material(color: const Color(0xFF1F6F8B), opacity: 0.55, roughness: 0.05)),
        ],
      ),
    );
    for (final (i, (x, tint)) in [(-4.2, const Color(0xFF9FD8C0)), (4.2, const Color(0xFFE0B0C8))].indexed) {
      add(
        GameObject(
          name: 'glass $i',
          transform: Transform(position: Vec3(x, 1.1, 1.5), scale: Vec3(1.6, 2, 0.05)),
          components: [
            MeshRenderer(Mesh.cube(), Material(color: tint, opacity: 0.35, roughness: 0.1, doubleSided: true)),
          ],
        ),
      );
    }

    // Lamps at the pool's corners: a post, and a bulb that glows and
    // lights round it, at night.
    const warm = Color(0xFFFFC46B);
    for (final (i, (x, z)) in [(-3.4, -1.2), (3.4, -1.2), (-3.4, 3.2), (3.4, 3.2)].indexed) {
      add(shape('lamp post $i', Mesh.cube(), const Color(0xFF30333A), at: Vec3(x, 0.9, z), size: Vec3(0.1, 1.8, 0.1)));
      final bulb = Material(color: Color.black, emissive: warm);
      _bulbs.add(bulb);
      final lamp = add(
        GameObject(
          name: 'bulb $i',
          transform: Transform(position: Vec3(x, 1.95, z), scale: Vec3.all(0.25)),
          components: [
            Light.point(color: warm, range: 6),
            MeshRenderer(Mesh.sphere(), bulb),
          ],
        ),
      );
      _lamps.add(lamp.get<Light>()!);
    }
    // A spotlight high on a post by the wall, shining down on the crates.
    add(shape('spot post', Mesh.cylinder(), const Color(0xFF30333A), at: Vec3(-2.5, 2, -4.2), size: Vec3(0.1, 2, 0.1)));
    final spot = add(
      GameObject(
        name: 'spotlight',
        transform: Transform(position: Vec3(-2.6, 4, -3.9), rotation: Quat.lookRotation(Vec3(-0.6, -1, -0.1))),
        components: [Light.spot(color: const Color(0xFFFFE2B0), range: 10, angle: 60.deg, shadows: true)],
      ),
    );
    _spot = spot.get<Light>()!;

    // A ring of glowing blocks, turning over the pool.
    final glow = Material(color: Color.black, emissive: const Color(0xFFFF4FA0), emissiveIntensity: 4);
    for (var i = 0; i < 10; i++) {
      _ring.add(
        add(
          GameObject(
            name: 'block $i',
            transform: Transform(scale: Vec3.all(0.18)),
            components: [MeshRenderer(Mesh.cube(), glow)],
          ),
        ),
      );
    }
    night = _startNight;
    fog = true;
  }

  /// Night: the sun gone, the sky dark, the lamps and spotlight on. By day
  /// the sky is the image, once loaded.
  bool get night => _night;
  set night(bool on) {
    _night = on;
    _sun.intensity = on ? 0 : 2.4;
    _ambient.intensity = on ? 0.04 : 0.3;
    for (final lamp in _lamps) {
      lamp.intensity = on ? 3 : 0;
    }
    for (final bulb in _bulbs) {
      bulb.emissiveIntensity = on ? 6 : 0.5;
    }
    _spot.intensity = on ? 60 : 0;
    final image = _image;
    world.sky = on
        ? Sky.gradient(
            zenith: const Color(0xFF04060E),
            horizon: const Color(0xFF1C2238),
            ground: const Color(0xFF07080A),
            lighting: 0.3,
          )
        : image != null
        ? Sky.image(image)
        : Sky.gradient(
            zenith: const Color(0xFF3F7CC8),
            horizon: const Color(0xFFD3E4F0),
            ground: const Color(0xFF6E6A60),
          );
  }

  /// Fog over the courtyard's far side.
  bool get fog => _fog;
  set fog(bool on) {
    _fog = on;
    world.fog = on ? const Fog(start: 14, end: 70) : null;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_turning) _angle += 0.15 * time.unscaledDelta;
    final at = Vec3(13 * math.sin(_angle), 5, 13 * math.cos(_angle));
    _eye
      ..position = at
      ..rotation = Quat.lookRotation(target - at);
    _spin += 0.6 * time.delta;
    for (final (i, block) in _ring.indexed) {
      final a = _spin + i / _ring.length * 2 * math.pi;
      block
        ..position = Vec3(1.2 * math.cos(a), 2.2 + 0.15 * math.sin(2 * a), 1 + 1.2 * math.sin(a))
        ..rotation = Quat.axisAngle(Vec3.up, a);
    }
    final picked = world.quality;
    final drawn = picked ?? Quality.device;
    readout.value =
        'drawn at ${drawn.name}${picked == null ? ' (the device picks)' : ''}   ${_night ? 'night' : 'day'}   '
        '$_fps fps';
  }

  @override
  List<Control> get controls => [
    for (final quality in Quality.values) Button(quality.name, () => world.quality = quality),
    Button("device's", () => world.quality = null),
    Toggle('night', () => night, (on) => night = on),
    Toggle('fog', () => fog, (on) => fog = on),
    Toggle('turn', () => _turning, (on) => _turning = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('look', s));
}
