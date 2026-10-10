// The game: the stage 2 scene (docs/stage-2.md), a character in a garden
// you can go around, tap and zoom. Kenney's Nature Kit models
// (assets/CREDITS.md).

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

class Garden extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    final character = Character();
    add(OrbitCamera(character));
    add(
      GameObject(
        transform: Transform(forward: Vec3(-0.4, -1, -0.3)),
        components: [Light.sun(intensity: 2.5)],
      ),
    );
    add(GameObject(components: [Light.ambient(color: Color.rgb(0.75, 0.85, 1), intensity: 0.5)]));
    add(Ground());
    for (final (i, asset) in _props.indexed) {
      add(Props(asset, seed: i));
    }
    add(character);
  }

  static const _props = [
    'tree_oak',
    'tree_pineRoundA',
    'tree_default',
    'rock_largeA',
    'rock_smallC',
    'plant_bush',
    'flower_redA',
    'flower_yellowA',
    'mushroom_red',
    'stump_round',
    'log',
    'fence_simple',
  ];
}

/// Goes around the character's middle: drag one finger to turn, scroll,
/// pinch (or two fingers on a trackpad) to zoom. An [Orbit] rig does it.
class OrbitCamera extends GameObject {
  OrbitCamera(GameObject character)
    : super(
        components: [
          Camera(fovY: 50, clearColor: Color.rgb(0.55, 0.75, 0.95)),
          Orbit(
            character,
            offset: target,
            yaw: 0.5,
            pitch: 0.35,
            distance: 9,
            minPitch: 0.05,
            maxPitch: 1.3,
            minDistance: 4,
            maxDistance: 12,
            dragSpeed: 0.008,
          ),
        ],
      );

  /// The character's middle, where it looks.
  static const target = Vec3(0, 1.3, 0);
}

class Ground extends GameObject {
  Ground() : super(transform: Transform(scale: Vec3(30, 1, 30)));

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final grass = await assets.texture('grass.png');
    final material = Material(baseColorTexture: grass, uvScale: Vec2(15, 15), roughness: 1, name: 'grass');
    addComponent(MeshRenderer(Mesh.plane(), material));
  }
}

/// A few of one model, the same places every run: trees in a ring beyond
/// where the camera goes, the rest near the character.
class Props extends GameObject {
  Props(this.asset, {required this.seed});

  /// The model's name in assets/props.
  final String asset;
  final int seed;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final random = math.Random(seed), tree = asset.startsWith('tree');
    for (var i = 0; i < (tree ? 6 : 3); i++) {
      final angle = random.nextDouble() * 2 * math.pi;
      final radius = tree ? 13 + random.nextDouble() * 3 : 2.5 + random.nextDouble() * 3.5;
      final turn = Quat.axisAngle(Vec3.up, random.nextDouble() * 2 * math.pi);
      add(await assets.model('props/$asset.glb')).transform = Transform(
        position: Vec3(math.cos(angle) * radius, 0, math.sin(angle) * radius),
        rotation: turn,
        scale: Vec3(3, 3, 3),
      );
    }
  }
}

/// Turns its head a quarter turn each time it's tapped.
class Character extends GameObject {
  Character() : super(transform: Transform(rotation: Quat.axisAngle(Vec3.up, 0.4)));

  late final GameObject _head;
  double _target = 0, _turned = 0;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    _head = add(await assets.model('character.glb')).find('head')!;
  }

  @override
  bool onClick(Finger finger) {
    _target += math.pi / 2;
    return true;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final step = (_target - _turned).clamp(-6 * time.delta, 6 * time.delta);
    if (step == 0) return;
    _head.rotate(Vec3.up, step);
    _turned += step;
  }
}
