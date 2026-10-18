// Stage 10, phase 1: materials' own shaders (assets/shaders/). A pool of
// lava that flows, three enemies sharing one material that each flash on
// their own when hit, a statue that dissolves with a glowing edge (and its
// shadow with it), and a hologram.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

class ShadersScene extends LabScene {
  @override
  String get title => 'Look: shaders';
  @override
  String get step => 'stage 10';
  @override
  String get about =>
      "Each surface is a .wgsl file in the lab's assets: lava that flows, enemies that share a material and flash "
      'one at a time when hit, a statue that dissolves (its shadow too), and a hologram. Edit a file and hot reload.';

  @override
  Vec3 get eye => Vec3(0, 3.4, 8);
  @override
  Vec3 get target => Vec3(0, 0.8, 0);

  final enemies = <GameObject>[];
  Material? lava, dissolve;
  var _dissolving = true, _amount = 0.0, _flow = 0.25, _hits = 0, _fps = '–';
  final _random = math.Random(3);

  @override
  void build() {
    find('sun')!.get<Light>()!.shadows = true;
    for (var i = 0; i < 3; i++) {
      enemies.add(
        add(
          shape(
            'enemy $i',
            Mesh.cube(),
            const Color(0xFF5C7A3A),
            at: Vec3(-3.2 + i * 1.1, 0.4, 0.6),
            size: Vec3.all(0.8),
          ),
        ),
      );
    }
    add(shape('statue', Mesh.sphere(), const Color(0xFFD8C8A8), at: Vec3(1.6, 1, -0.4), size: Vec3.all(1.6)));
    add(
      GameObject(
        name: 'lava',
        transform: Transform(position: Vec3(0, 0.01, 2.2), scale: Vec3(5, 1, 1.6)),
        components: [MeshRenderer(Mesh.plane(), Material())],
      ),
    );
    add(shape('hologram', Mesh.sphere(), const Color(0xFF40C0FF), at: Vec3(3.6, 1.1, 1), size: Vec3.all(0.9)));
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    final shaders = await (
      assets.shader('shaders/lava.wgsl'),
      assets.shader('shaders/flash.wgsl'),
      assets.shader('shaders/dissolve.wgsl'),
      assets.shader('shaders/hologram.wgsl'),
    ).wait;
    final (lavaShader, flash, dissolveShader, hologram) = shaders;
    lava = Material(
      shader: lavaShader,
      values: {'hot': const Color(0xFFFF6A10), 'scale': 3.0, 'flow': _flow},
      uvScale: Vec2(2.5, 0.8),
    );
    find('lava')!.get<MeshRenderer>()!.material = lava!;
    // One material for every enemy: each flashes by its own number.
    final enemy = Material(shader: flash, color: const Color(0xFF5C7A3A), roughness: 0.6);
    for (final e in enemies) {
      e.get<MeshRenderer>()!.material = enemy;
    }
    dissolve = Material(
      shader: dissolveShader,
      color: const Color(0xFFD8C8A8),
      alphaCutoff: 0.5,
      values: {'edge': const Color(0xFFFF8A30), 'amount': _amount},
    );
    find('statue')!.get<MeshRenderer>()!.material = dissolve!;
    find('hologram')!.get<MeshRenderer>()!.material = Material(
      shader: hologram,
      color: const Color(0xFF40C0FF),
      blending: Blending.additive,
    );
  }

  /// Flashes one enemy, picked at random: its own number goes to 1 and
  /// back, while the others stay as they are.
  void hit() {
    final enemy = enemies[_random.nextInt(enemies.length)];
    final renderer = enemy.get<MeshRenderer>()!;
    if (renderer.material?.shader == null) return;
    _hits++;
    enemy.tween(1.0, 0.0, seconds: 0.35, onUpdate: (v) => renderer.set('flash', v));
    log.add('${enemy.name} hit');
  }

  /// How much of the statue is gone, 0 to 1.
  double get amount => _amount;
  set amount(double value) {
    _amount = value;
    dissolve?.set('amount', value);
  }

  /// How fast the lava flows.
  double get flow => _flow;
  set flow(double value) {
    _flow = value;
    lava?.set('flow', value);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_dissolving && dissolve != null) {
      // There and gone, and back, every eight seconds.
      amount = 0.5 - 0.55 * math.cos(time.sinceStart * 2 * math.pi / 8);
    }
    readout.value =
        'dissolved ${(_amount.clamp(0, 1) * 100).round()}%   lava flow ${_flow.toStringAsFixed(2)}   '
        'hits $_hits   $_fps fps';
  }

  @override
  List<Control> get controls => [
    Button('hit', hit),
    Toggle('dissolve by itself', () => _dissolving, (on) => _dissolving = on),
    Slider('dissolved', () => amount, (value) {
      _dissolving = false;
      amount = value;
    }),
    Slider('lava flow', () => flow, (value) => flow = value, max: 2),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('shaders', s));
}
