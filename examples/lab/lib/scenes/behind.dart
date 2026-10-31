// Stage 10, phase 3: shaders that read what's behind them (assets/shaders/).
// A pond in a stone basin: foam where the water meets the stones, the
// bank and the Knight's boots, the bed bent by the ripples and lost in the
// murk the deeper it is. Beside it a glass ball that bends what's behind
// it, and a force field that glows where it cuts through the floor.

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

class BehindScene extends LabScene {
  @override
  String get title => "Look: what's behind";
  @override
  String get step => 'stage 10';
  @override
  String get about =>
      "These shaders read the scene behind their surfaces: how far behind it is (depth_behind) and its colour "
      "(scene_color). The pond foams where it's shallow and bends the bed under its ripples; the glass ball bends "
      'what you see through it; the force field glows where it cuts through the floor. A frame with none of '
      'them in view keeps neither.';

  @override
  Vec3 get eye => Vec3(-0.4, 3.4, 5.8);
  @override
  Vec3 get target => Vec3(0, 0.2, 0);

  Material? pond, glass, bubble;
  GameObject? knight;
  var _waves = 1.0, _foam = 0.12, _bend = 0.03, _extras = true, _fps = '–';
  final _extraObjects = <GameObject>[];

  /// The water's level, and the basin's inside, 4 × 3 m.
  static const level = 0.42;
  static const width = 4.0, depth = 3.0;

  @override
  void build() {
    find('sun')!.get<Light>()!.shadows = true;
    // The basin's walls, a little higher than the water.
    const stone = Color(0xFF8A8478);
    for (final (at, size) in [
      (Vec3(0, 0.3, -depth / 2 - 0.15), Vec3(width + 0.6, 0.6, 0.3)),
      (Vec3(0, 0.3, depth / 2 + 0.15), Vec3(width + 0.6, 0.6, 0.3)),
      (Vec3(-width / 2 - 0.15, 0.3, 0), Vec3(0.3, 0.6, depth)),
      (Vec3(width / 2 + 0.15, 0.3, 0), Vec3(0.3, 0.6, depth)),
    ]) {
      add(shape('wall', Mesh.cube(), stone, at: at, size: size));
    }
    // A sandy bank rising out of the water on the right.
    add(
      shape('bank', Mesh.cube(), const Color(0xFFC8B088), at: Vec3(1.3, 0.25, 0), size: Vec3(1.8, 0.2, depth))
        ..rotation = Quat.axisAngle(Vec3.forward, -0.32),
    );
    // Stones on the bed, some breaking the surface.
    for (final (x, z, size, color) in [
      (-1.2, -0.6, 0.7, const Color(0xFFB09070)),
      (-0.3, 0.7, 0.5, const Color(0xFF708090)),
      (0.2, -0.8, 0.9, const Color(0xFF90A070)),
      (-1.5, 0.8, 0.35, const Color(0xFFD0C8B8)),
    ]) {
      add(shape('stone', Mesh.sphere(), color, at: Vec3(x, 0.12, z), size: Vec3(size, size, size)));
    }
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    final (pondShader, glassShader, bubbleShader) = await (
      assets.shader('shaders/pond.wgsl'),
      assets.shader('shaders/glass.wgsl'),
      assets.shader('shaders/bubble.wgsl'),
    ).wait;

    pond = Material(
      shader: pondShader,
      roughness: 0.06,
      values: {'height': _waves, 'deep': const Color(0xFF0A3A48), 'foam': _foam, 'bend': _bend},
      reach: 0.05,
    );
    add(
      GameObject(
        name: 'pond',
        transform: Transform(position: Vec3(0, level, 0)),
        components: [MeshRenderer(_grid(width, depth, 64, 48), pond!)],
      ),
    );

    glass = Material(shader: glassShader, values: {'bend': 0.06, 'tint': const Color(0xFFDDEEFF)});
    _extraObjects.add(
      add(
        GameObject(
          name: 'glass ball',
          transform: Transform(position: Vec3(-3.2, 0.6, 1.0), scale: Vec3(1.1, 1.1, 1.1)),
          components: [MeshRenderer(Mesh.sphere(), glass!)],
        ),
      ),
    );

    bubble = Material(
      shader: bubbleShader,
      blending: Blending.additive,
      doubleSided: true,
      values: {'color': const Color(0xFF40C0FF), 'touch': 0.12},
    );
    _extraObjects.add(
      add(
        GameObject(
          name: 'force field',
          transform: Transform(position: Vec3(3.4, 0.2, 1.2), scale: Vec3(1.6, 1.6, 1.6)),
          components: [MeshRenderer(Mesh.sphere(), bubble!)],
        ),
      ),
    );

    // Standing in the shallows, where the bank goes under.
    final k = add(await assets.instantiate('kaykit/knight.glb'))
      ..position = Vec3(1.6, 0.42, -0.8)
      ..rotation = Quat.axisAngle(Vec3.up, -0.4);
    for (final extra in ['1H_Sword_Offhand', 'Badge_Shield', 'Rectangle_Shield', 'Spike_Shield', '2H_Sword']) {
      k.find(extra)?.active = false;
    }
    k.animator?.crossFade('Idle', seconds: 0);
    knight = k;
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

  /// How high the waves are, 0 to 2.
  double get waves => _waves;
  set waves(double value) {
    _waves = value;
    pond?.set('height', value);
  }

  /// How deep the water is where the foam ends, in metres.
  double get foam => _foam;
  set foam(double value) {
    _foam = value;
    pond?.set('foam', value);
  }

  /// How far the ripples bend the bed, as a share of the screen.
  double get bend => _bend;
  set bend(double value) {
    _bend = value;
    pond?.set('bend', value);
  }

  /// Whether the glass ball and the force field are there.
  bool get extras => _extras;
  set extras(bool on) {
    _extras = on;
    for (final o in _extraObjects) {
      o.active = on;
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value =
        'waves ${_waves.toStringAsFixed(2)}   foam ${(_foam * 100).toStringAsFixed(0)} cm   '
        'bend ${_bend.toStringAsFixed(3)}   $_fps fps';
  }

  @override
  List<Control> get controls => [
    Toggle('glass ball and force field', () => extras, (on) => extras = on),
    Slider('waves', () => waves, (value) => waves = value, max: 2),
    Slider('foam', () => foam, (value) => foam = value, max: 0.3),
    Slider('bend', () => bend, (value) => bend = value, max: 0.1),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('behind', s));
}
