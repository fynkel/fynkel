// Stage 8, phase 2: lamps. A dark room lit by coloured lamps that drift
// about, and a torch that follows the pointer. Every object is lit by the
// lamps brightest at it, up to 4 at low and medium and 8 at high, so the
// floor and walls are tiles: one big floor would get only the few lamps
// nearest its middle, and big tiles show their edges where neighbours'
// lamps differ. 32 lights (31 lamps and the torch) is the budget measured
// here.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// `--dart-define=LAB_LAMPS=0` starts the scene with that many coloured
/// lamps (31 unless given), for measuring.
const _startLamps = int.fromEnvironment('LAB_LAMPS', defaultValue: 31);

class LampsScene extends LabScene {
  @override
  String get title => 'Look: lamps';
  @override
  String get step => 'stage 8';
  @override
  String get about =>
      'A dark room, lit by coloured lamps and a torch that follows the pointer. Each object is lit by the lamps '
      'brightest at it: up to 4 at low and medium quality, 8 at high. Try 64 lights, and low.';

  @override
  Vec3 get eye => Vec3(0, 8, 11);
  @override
  Vec3 get target => Vec3(0, 0, 0.5);

  late final GameObject _torch;
  final _lamps = <GameObject>[];
  var _moving = true, _time = 0.0, _fps = '–';

  /// The room's half-width: its floor runs from -size to size.
  static const size = 8.0;

  @override
  void build() {
    world.quality = Quality.values.where((q) => q.name == _startQuality).firstOrNull;
    // Night: no sun, a little ambient light, a dark sky.
    find('sun')!.active = false;
    find('floor')!.active = false;
    find('ambient')!.get<Light>()!.intensity = 0.04;
    find('camera')!.get<Camera>()!.clearColor = const Color(0xFF05060A);

    final floor = Material(color: const Color(0xFF8A8F99), roughness: 0.7);
    final wall = Material(color: const Color(0xFFB8B0A0), roughness: 0.9);
    // Tiles 1 m across, and lamps reaching 3.5 m: neighbouring tiles then
    // mostly share their nearest lamps, so their edges don't show.
    const tile = 1.0;
    for (var x = -size + tile / 2; x < size; x += tile) {
      for (var z = -size + tile / 2; z < size; z += tile) {
        add(_box('floor', floor, Vec3(x, -0.05, z), Vec3(tile, 0.1, tile)));
      }
      // The back wall and the two sides, 3 m high.
      for (var y = tile / 2; y < 3; y += tile) {
        add(_box('wall', wall, Vec3(x, y, -size), Vec3(tile, tile, 0.2)));
        add(_box('wall', wall, Vec3(-size, y, x), Vec3(0.2, tile, tile)));
        add(_box('wall', wall, Vec3(size, y, x), Vec3(0.2, tile, tile)));
      }
    }
    // Things to light: metal and plastic, round and square.
    final random = math.Random(8);
    for (var i = 0; i < 14; i++) {
      final at = Vec3(random.nextDouble() * 12 - 6, 0, random.nextDouble() * 12 - 6);
      final metal = i.isEven;
      final material = Material(
        color: metal ? const Color(0xFFE8E0D0) : const Color(0xFFF2F2F2),
        metallic: metal ? 1 : 0,
        roughness: 0.15 + random.nextDouble() * 0.5,
      );
      final tall = 0.6 + random.nextDouble() * 1.4;
      add(
        i % 3 == 0
            ? GameObject(
                name: 'ball $i',
                transform: Transform(position: at + Vec3(0, 0.5, 0)),
                components: [MeshRenderer(Mesh.sphere(), material)],
              )
            : _box('block $i', material, at + Vec3(0, tall / 2, 0), Vec3(0.8, tall, 0.8)),
      );
    }

    _torch = add(
      GameObject(
        name: 'torch',
        transform: Transform(position: eye),
        components: [Light.spot(intensity: 250, range: 25, angle: 24.deg, softness: 0.4)],
      ),
    );
    lamps = _startLamps;
  }

  /// How many coloured lamps there are, 0 to 64.
  int get lamps => _lamps.length;
  set lamps(int count) {
    while (_lamps.length > count) {
      _lamps.removeLast().destroy();
    }
    while (_lamps.length < count) {
      final i = _lamps.length;
      // Hues round the colour wheel, so neighbours differ.
      final colour = _hue((i * 137.5) % 360);
      _lamps.add(
        add(
          GameObject(
            name: 'lamp $i',
            transform: Transform(position: _lampAt(i), scale: Vec3.one * 0.15),
            components: [
              Light.point(color: colour, intensity: 1.5, range: 3.5),
              // The bulb: glows in the lamp's colour, lit by nothing.
              MeshRenderer(Mesh.sphere(), Material(color: Color.black, emissive: colour, emissiveIntensity: 3)),
            ],
          ),
        ),
      );
    }
  }

  /// Where lamp [i] is at [_time]: on a slow loop of its own over the room.
  Vec3 _lampAt(int i) {
    final phase = i * 2.39996;
    final radius = 2 + (i % 5) * 1.2;
    final speed = 0.15 + (i % 3) * 0.05;
    final angle = phase + _time * speed * (i.isEven ? 1 : -1);
    return Vec3(radius * math.cos(angle), 0.6 + 0.5 * (1 + math.sin(_time * 0.7 + phase)), radius * math.sin(angle));
  }

  /// A bright colour [degrees] round the colour wheel, a little pale.
  static Color _hue(double degrees) {
    double channel(double n) {
      final k = (n + degrees / 60) % 6;
      return 1 - 0.75 * math.max(0, math.min(1, math.min(k, 4 - k)));
    }

    return Color.rgb(channel(5), channel(3), channel(1));
  }

  GameObject _box(String name, Material material, Vec3 at, Vec3 size) => GameObject(
    name: name,
    transform: Transform(position: at, scale: size),
    components: [MeshRenderer(Mesh.cube(), material)],
  );

  @override
  void onUpdate() {
    super.onUpdate();
    if (_moving) _time += time.unscaledDelta;
    for (var i = 0; i < _lamps.length; i++) {
      _lamps[i].position = _lampAt(i);
    }
    // The torch shines from beside the camera at where the pointer is on
    // the floor, or the room's middle without one.
    final pointer = input.mousePosition;
    var aim = Vec3.zero;
    if (pointer != null) {
      final ray = find('camera')!.get<Camera>()!.screenPointToRay(pointer);
      if (ray.direction.y < -1e-3) aim = ray.origin + ray.direction * (-ray.origin.y / ray.direction.y);
    }
    final from = eye + Vec3(1, -0.5, 0);
    _torch
      ..position = from
      ..rotation = Quat.lookRotation(aim - from);

    final picked = world.quality;
    final drawn = picked ?? Quality.device;
    readout.value =
        '${lamps + 1} lights   drawn at ${drawn.name}${picked == null ? ' (the device picks)' : ''}   $_fps fps';
  }

  @override
  List<Control> get controls => [
    for (final count in [7, 31, 63]) Button('${count + 1} lights', () => lamps = count),
    for (final quality in Quality.values) Button(quality.name, () => world.quality = quality),
    Button("device's", () => world.quality = null),
    Toggle('move', () => _moving, (on) => _moving = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('lamps', s));
}
