// The game: the stage 2 scene (docs/stage-2.md), a character in a garden
// you can turn around, tap and zoom. Only package:fynkel and the view's
// FrameInput; main.dart is the Flutter side, and measures.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show FrameInput;

/// The app's own loading: the engine takes bytes and never touches files.
typedef BytesOf = Future<Uint8List> Function(String asset);

/// How long one asset took, in its three parts.
typedef LoadTime = ({String asset, Duration read, Duration load, Duration spawn});

/// Kenney's Nature Kit models (assets/CREDITS.md), each placed a few times.
const _props = [
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

class Garden {
  Garden(this._bytesOf) {
    camera = world.spawn(Camera(fovY: 50, clearColor: Color.rgb(0.55, 0.75, 0.95)), _cameraTransform());
    world.spawn(Light.sun(intensity: 2.5), Transform(forward: Vec3(-0.4, -1, -0.3)));
    world.spawn(Light.ambient(color: Color.rgb(0.75, 0.85, 1), intensity: 0.5));
  }

  final BytesOf _bytesOf;
  final world = World();
  late final GameObject camera;

  /// The character, and its model, while it's loaded.
  GameObject? character;
  Model? _characterModel;
  GameObject? _head;

  // The camera circles the character's middle: [_yaw] around it, [_pitch]
  // above the ground, [_distance] away.
  static const _target = Vec3(0, 1.3, 0);
  double _yaw = 0.5, _pitch = 0.35, _distance = 9;

  // Where the head is turning to, and how far it has got.
  double _headTarget = 0, _headTurned = 0;

  /// Everything: the ground, the props and the character, read and loaded
  /// all at once. What each took, the character's last.
  Future<List<LoadTime>> load() async {
    final ground = _timed('grass.png', (bytes) async {
      final grass = await Texture2D.load(bytes);
      return () => world.spawn(
        Transform(scale: Vec3(30, 1, 30)),
        MeshRenderer(
          Mesh.plane(),
          Material(baseColorTexture: grass, uvScale: Vec2(15, 15), roughness: 1, name: 'grass'),
        ),
      );
    });
    final props = [
      for (final (i, name) in _props.indexed)
        _timed('props/$name.glb', (bytes) async {
          final model = await Model.load(bytes);
          return () => _place(name, model, seed: i);
        }),
    ];
    final times = await Future.wait([ground, ...props]);
    return [...times, await loadCharacter()];
  }

  /// The character, loaded again after [unloadCharacter].
  Future<LoadTime> loadCharacter() => _timed('character.glb', (bytes) async {
    final model = _characterModel = await Model.load(bytes);
    return () {
      final c = character = world.spawnModel(model, Transform(rotation: Quat.axisAngle(Vec3.up, 0.4)));
      _head = c.find('head');
      _headTarget = _headTurned = 0;
    };
  });

  /// Takes the character out and frees all it brought: its objects, then
  /// its materials, meshes and textures, then the model. Memory should be
  /// back where it was before [loadCharacter].
  void unloadCharacter() {
    final model = _characterModel;
    if (model == null) return;
    character!.despawn();
    character = _head = _characterModel = null;
    for (final material in model.materials) {
      material.dispose();
    }
    for (final mesh in model.meshes) {
      mesh.dispose();
    }
    for (final texture in model.textures) {
      texture.dispose();
    }
    model.dispose();
  }

  /// Reads [asset], loads it with [load], and calls what that returns to
  /// put it in the world, timing each part.
  Future<LoadTime> _timed(String asset, Future<void Function()> Function(Uint8List bytes) load) async {
    final clock = Stopwatch()..start();
    final bytes = await _bytesOf(asset);
    final read = clock.elapsed;
    final spawn = await load(bytes);
    final loaded = clock.elapsed;
    spawn();
    return (asset: asset, read: read, load: loaded - read, spawn: clock.elapsed - loaded);
  }

  /// A few of [model] scattered around the character, the same places
  /// every run: trees in a ring beyond where the camera goes, the rest
  /// near the character, inside the ring the camera moves on.
  void _place(String name, Model model, {required int seed}) {
    final random = math.Random(seed);
    final tree = name.startsWith('tree');
    for (var i = 0; i < (tree ? 6 : 3); i++) {
      final angle = random.nextDouble() * 2 * math.pi;
      final radius = tree ? 13 + random.nextDouble() * 3 : 2.5 + random.nextDouble() * 3.5;
      world.spawnModel(
        model,
        Transform(
          position: Vec3(math.cos(angle) * radius, 0, math.sin(angle) * radius),
          rotation: Quat.axisAngle(Vec3.up, random.nextDouble() * 2 * math.pi),
          scale: Vec3(3, 3, 3),
        ),
      );
    }
  }

  void onFrame(double dt, FrameInput input) {
    // A tap on the character turns its head a quarter turn.
    if (input.tapped case final point?) {
      final hit = world.raycast(camera.rayThrough(point));
      if (character case final c? when hit != null && hit.object.isPartOf(c)) _headTarget += math.pi / 2;
    }
    if (_head case final head?) {
      final step = (_headTarget - _headTurned).clamp(-6 * dt, 6 * dt);
      if (step != 0) {
        head.rotate(Vec3.up, step);
        _headTurned += step;
      }
    }

    // Drag to go around, scroll (or two fingers on a trackpad) to zoom.
    _yaw -= input.drag.dx * 0.008;
    _pitch = (_pitch + input.drag.dy * 0.006).clamp(0.05, 1.3);
    _distance = (_distance * math.exp(input.scroll.dy * 0.002)).clamp(4.0, 12.0);
    if (input.drag.distance > 0 || input.scroll.distance > 0) camera.transform = _cameraTransform();
  }

  /// From the camera to the character's middle, what a tap there casts.
  Ray get centreRay {
    final from = _cameraTransform().position;
    return Ray(from, _target - from);
  }

  Transform _cameraTransform() {
    final flat = math.cos(_pitch) * _distance;
    final position = _target + Vec3(math.sin(_yaw) * flat, math.sin(_pitch) * _distance, math.cos(_yaw) * flat);
    return Transform(position: position, lookAt: _target);
  }
}
