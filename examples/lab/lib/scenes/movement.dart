// Stage 10, phase 2: shaders that move their vertices (assets/shaders/).
// A field of grass blades in the wind, each swaying on its own, all one
// draw; water with waves; jellies that wobble, more when poked; and the
// Knight's cape blowing out behind him whichever way he turns. Their
// shadows move with them.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// How many grass blades the field has.
const blades = 2000;

class MovementScene extends LabScene {
  @override
  String get title => 'Look: movement';
  @override
  String get step => 'stage 10';
  @override
  String get about =>
      "Each shader's vertex function moves its mesh: $blades grass blades swaying on their own (one draw), waves on "
      "water, jellies that wobble when poked, and the Knight's cape blown out behind him as he turns. Their shadows "
      'move with them. A material says how far its shader moves it (its reach), so nothing is culled too soon.';

  @override
  Vec3 get eye => Vec3(0, 3.2, 7.5);
  @override
  Vec3 get target => Vec3(0, 0.4, 0);

  final jellies = <GameObject>[];
  Material? grass, water, cape;
  GameObject? knight;
  var _wind = 0.25, _waves = 1.0, _turning = true, _pokes = 0, _pixels = false, _drift = false, _fps = '–';
  final _random = math.Random(5);

  @override
  void build() {
    find('sun')!.get<Light>()!.shadows = true;
    for (var i = 0; i < 3; i++) {
      jellies.add(
        add(
          shape(
            'jelly $i',
            Mesh.sphere(),
            const [Color(0xFFE04070), Color(0xFF40C070), Color(0xFFF0B030)][i],
            at: Vec3(1.2 + i * 0.9, 0.35, -1.6 + i * 0.2),
            size: Vec3(0.7, 0.7, 0.7),
          ),
        ),
      );
    }
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    final shaders = await (
      assets.shader('shaders/grass.wgsl'),
      assets.shader('shaders/water.wgsl'),
      assets.shader('shaders/jelly.wgsl'),
      assets.shader('shaders/cape.wgsl'),
    ).wait;
    final (grassShader, waterShader, jellyShader, capeShader) = shaders;

    // One material and one mesh for every blade: one draw.
    grass = Material(
      shader: grassShader,
      color: const Color(0xFF7DB04A),
      roughness: 0.8,
      doubleSided: true,
      values: {'wind': _wind},
      reach: _wind,
    );
    final blade = _blade();
    for (var i = 0; i < blades; i++) {
      final at = Vec3(-4.6 + _random.nextDouble() * 4.2, 0, -1.8 + _random.nextDouble() * 4.0);
      add(
        GameObject(
          name: 'blade $i',
          transform: Transform(
            position: at,
            rotation: Quat.axisAngle(Vec3.up, _random.nextDouble() * math.pi),
            scale: Vec3(1, 0.3 + _random.nextDouble() * 0.35, 1),
          ),
          components: [MeshRenderer(blade, grass!)],
        ),
      );
    }

    water = Material(
      shader: waterShader,
      roughness: 0.08,
      values: {'height': _waves, 'deep': const Color(0xFF0B3050), 'shallow': const Color(0xFF6FA8C8)},
      reach: 0.1,
    );
    add(
      GameObject(
        name: 'water',
        transform: Transform(position: Vec3(2.6, 0.2, 1.3)),
        components: [MeshRenderer(_grid(4, 2.6, 48, 32), water!)],
      ),
    );

    final jelly = Material(shader: jellyShader, roughness: 0.15, reach: 0.15);
    for (final (i, j) in jellies.indexed) {
      final renderer = j.get<MeshRenderer>()!;
      renderer.material = jelly;
      renderer.color = [const Color(0xFFE04070), const Color(0xFF40C070), const Color(0xFFF0B030)][i];
    }

    final k = add(await assets.instantiate('kaykit/knight.glb'))
      ..position = Vec3(-0.1, 0, -1.6)
      ..rotation = Quat.axisAngle(Vec3.up, 2.4);
    for (final extra in ['1H_Sword_Offhand', 'Badge_Shield', 'Rectangle_Shield', 'Spike_Shield', '2H_Sword']) {
      k.find(extra)?.active = false;
    }
    k.animator?.crossFade('Idle', seconds: 0);
    // The cape alone gets the shader: a copy of the Knight's material.
    final renderer = k.find('Knight_Cape')!.get<MeshRenderer>()!;
    cape = renderer.material!.copy(name: 'cape')
      ..shader = capeShader
      ..set('wind', _wind * 2)
      ..reach = 0.4;
    renderer.material = cape!;
    knight = k;
  }

  /// A blade of grass: 6 cm wide at its foot, 1 m tall, to a point, in
  /// three steps so it bends smoothly. v is 1 at the foot.
  static Mesh _blade() {
    final positions = <Vec3>[];
    final uvs = <Vec2>[];
    for (var i = 0; i <= 3; i++) {
      final y = i / 3;
      final half = 0.03 * (1 - y);
      positions
        ..add(Vec3(-half, y, 0))
        ..add(Vec3(half, y, 0));
      uvs
        ..add(Vec2(0, 1 - y))
        ..add(Vec2(1, 1 - y));
    }
    final indices = <int>[];
    for (var i = 0; i < 3; i++) {
      final a = i * 2;
      indices.addAll([a, a + 1, a + 3, a, a + 3, a + 2]);
    }
    return Mesh.custom(
      positions: positions,
      normals: List.filled(positions.length, Vec3(0, 0, 1)),
      uvs: uvs,
      indices: indices,
    );
  }

  /// A flat grid [width] × [depth] metres, centred, of [across] × [down]
  /// squares: enough vertices for waves.
  static Mesh _grid(double width, double depth, int across, int down) {
    final positions = <Vec3>[];
    final uvs = <Vec2>[];
    for (var j = 0; j <= down; j++) {
      for (var i = 0; i <= across; i++) {
        positions.add(Vec3((i / across - 0.5) * width, 0, (j / down - 0.5) * depth));
        uvs.add(Vec2(i / across, j / down));
      }
    }
    final indices = <int>[];
    for (var j = 0; j < down; j++) {
      for (var i = 0; i < across; i++) {
        final a = j * (across + 1) + i;
        final b = a + across + 1;
        indices.addAll([a, b, a + 1, a + 1, b, b + 1]);
      }
    }
    return Mesh.custom(
      positions: positions,
      normals: List.filled(positions.length, Vec3.up),
      uvs: uvs,
      indices: indices,
    );
  }

  /// Pokes one jelly, picked at random: its own wobble goes up and dies
  /// away, while the others' stay as they are.
  void poke() {
    final jelly = jellies[_random.nextInt(jellies.length)];
    final renderer = jelly.get<MeshRenderer>()!;
    if (renderer.material?.shader == null) return;
    _pokes++;
    jelly.tween(1.0, 0.0, seconds: 1.2, ease: Ease.outQuad, onUpdate: (v) => renderer.set('poke', v));
    log.add('${jelly.name} poked');
  }

  /// How hard the wind blows: how far the grass tips bend, in metres. The
  /// grass's reach follows it.
  double get wind => _wind;
  set wind(double value) {
    _wind = value;
    grass
      ?..set('wind', value)
      ..reach = value;
    cape?.set('wind', value * 2);
  }

  /// How high the waves are, 0 to 2.
  double get waves => _waves;
  set waves(double value) {
    _waves = value;
    water?.set('height', value);
  }

  /// The pixel look: the garden drawn 180 pixels high, with hard edges.
  /// The camera is put on the nearest whole pixel, so the still parts
  /// don't shimmer as it drifts.
  bool get pixelLook => _pixels;
  set pixelLook(bool on) {
    _pixels = on;
    find('camera')!.get<Camera>()!.pixels = on ? 180 : null;
  }

  /// Whether the camera drifts slowly from side to side.
  bool get drift => _drift;
  set drift(bool on) {
    _drift = on;
    if (!on) find('camera')!.position = eye;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_drift) {
      find('camera')!.position = eye + Vec3(math.sin(time.sinceStart * 0.3) * 1.5, 0, 0);
    }
    if (_turning && knight != null) {
      knight!.rotation = Quat.axisAngle(Vec3.up, 2.4 + time.sinceStart * 0.4);
    }
    readout.value =
        '$blades blades   wind ${_wind.toStringAsFixed(2)} m   waves ${_waves.toStringAsFixed(2)}   '
        'pokes $_pokes${_pixels ? '   180 px high' : ''}   $_fps fps';
  }

  @override
  List<Control> get controls => [
    Button('poke a jelly', poke),
    Toggle('the Knight turns', () => _turning, (on) => _turning = on),
    Slider('wind', () => wind, (value) => wind = value, max: 0.5),
    Slider('waves', () => waves, (value) => waves = value, max: 2),
    Toggle('pixel look', () => pixelLook, (on) => pixelLook = on),
    Toggle('the camera drifts', () => drift, (on) => drift = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('movement', s));
}
