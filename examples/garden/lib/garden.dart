// The game: the stage 2 scene (docs/stage-2.md), a character in a garden
// you can go around and zoom; it walks where you tap (stage 9). Kenney's Nature Kit models
// (assets/CREDITS.md).

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

class Garden extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    final character = Character();
    add(OrbitCamera(character));
    add(GameObject(components: [Light.sun(direction: Vec3(-0.4, -1, -0.3), intensity: 2.5)]));
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
          Camera(fovY: 50.deg, clearColor: Color.rgb(0.55, 0.75, 0.95)),
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

/// Tapped, it sends the character there.
class Ground extends GameObject {
  Ground() : super(transform: Transform(scale: Vec3(30, 1, 30)));

  @override
  bool onClick(Finger finger) {
    final hit = finger.over;
    if (hit == null) return false;
    world.first<Character>()?.walkTo(hit.point);
    return true;
  }

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
      add(await assets.instantiate('props/$asset.glb')).transform = Transform(
        position: Vec3(math.cos(angle) * radius, 0, math.sin(angle) * radius),
        rotation: turn,
        scale: Vec3(3, 3, 3),
      );
    }
  }
}

/// Kenney's character, with its own clips: it idles, walks where the
/// ground is tapped, and picks something up when it's tapped itself. Its
/// head turns to the camera after the clip, in onLateUpdate.
class Character extends GameObject {
  Character() : super(transform: Transform(rotation: Quat.axisAngle(Vec3.up, 0.4)));

  /// Metres a second: the walk clip's pace, so its feet don't slide.
  static const speed = 1.6;

  /// How far the head turns to the camera at most, either way.
  static const lookLimit = 1.0;

  late final GameObject _head;
  late final Animator _animator;
  Vec3? _goal;
  bool _pickingUp = false;
  double _look = 0;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.instantiate('character.glb'));
    _head = model.find('head')!;
    _animator = model.animator!
      ..play('idle')
      ..onEnd = (_) => _pickingUp = false;
  }

  /// Walks to [point] on the ground, unless it's busy picking up.
  void walkTo(Vec3 point) {
    if (!_pickingUp) _goal = Vec3(point.x, 0, point.z);
  }

  @override
  bool onClick(Finger finger) {
    _goal = null;
    _pickingUp = true;
    _animator.crossFade('pick-up', seconds: 0.15, loop: false);
    return true;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_goal case final goal?) {
      final to = goal - position;
      final distance = to.length;
      if (distance < 0.05) {
        _goal = null;
      } else {
        position += to * (math.min(distance, speed * time.delta) / distance);
        // The model faces +Z; lookRotation turns -Z to the way given.
        final facing = Quat.lookRotation(-to);
        rotation = Quat.slerp(rotation, facing, math.min(1, 10 * time.delta));
      }
    }
    if (!_pickingUp) _animator.crossFade(_goal == null ? 'idle' : 'walk', seconds: 0.2);
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    // The camera's way from the character, round its up: the head turns
    // that much, within its limit, eased, on top of the clip's pose.
    final camera = inverseTransformPoint(world.first<OrbitCamera>()!.position);
    final want = (math.atan2(camera.x, camera.z)).clamp(-lookLimit, lookLimit);
    _look += (want - _look) * math.min(1, 6 * time.delta);
    _head.localRotation = Quat.axisAngle(Vec3.up, _look) * _head.localRotation;
  }
}
