// The game's three places — the Menu, the Level and the Results — and its
// code prefabs (a coin, a burst of sparks), as `ObjectSpec`s: the one
// description both ways of running the game use. `Game` in live mode
// hands them to `world.build`; tool/scenes.dart writes the same specs as
// scene and prefab documents (`package:fynkel/authoring.dart`) that the
// scenes mode loads. Nothing here is JSON: the shapes are the built-in
// primitives, the materials a [Look] made from code, the sounds the
// project's CC0 clips (assets/audio).

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../generated/assets.g.dart';
import 'coin_kind.dart';
import 'collectible.dart';
import 'gate.dart';
import 'player.dart';
import 'switch.dart';

/// The project's layers and tags (fynkel.project.json): the player walks
/// on `player`, the zones sit on `trigger` and listen to `player` only.
const layersAndTags = LayersAndTags(layers: ['default', 'props', 'player', 'trigger'], tags: ['player', 'pickup']);
final int playerMask = layersAndTags.mask(['player'])!;
final int defaultMask = layersAndTags.mask(['default'])!;

/// The game's materials, one [make] call each: `world.createMaterial` when
/// the game builds live, a written `.material.json` for the scenes.
class Look {
  Look(AssetRef Function(String name, PbrMaterial material) make)
      : floor = make('floor', const PbrMaterial(baseColor: Vec4(0.55, 0.56, 0.52, 1), roughness: 0.9)),
        wall = make('wall', const PbrMaterial(baseColor: Vec4(0.78, 0.74, 0.66, 1), roughness: 0.8)),
        pillar = make('pillar', const PbrMaterial(baseColor: Vec4(0.35, 0.36, 0.4, 1), metallic: 0.1, roughness: 0.6)),
        gold = make(
          'gold',
          const PbrMaterial(
            baseColor: Vec4(1, 0.78, 0.3, 1),
            metallic: 0.4,
            roughness: 0.35,
            emissive: Vec3(1, 0.7, 0.2),
            emissiveStrength: 0.35,
          ),
        ),
        player = make('player', const PbrMaterial(baseColor: Vec4(0.2, 0.45, 0.95, 1))),
        visor = make('visor', const PbrMaterial(baseColor: Vec4(0.05, 0.05, 0.08, 1), metallic: 0.9, roughness: 0.2)),
        hazard = make(
          'hazard',
          const PbrMaterial(baseColor: Vec4(0.6, 0.05, 0.05, 1), emissive: Vec3(1, 0.1, 0.05), emissiveStrength: 0.8),
        ),
        gate = make('gate', const PbrMaterial(baseColor: Vec4(0.25, 0.27, 0.32, 1), metallic: 0.9, roughness: 0.4)),
        pad = make(
          'switch',
          const PbrMaterial(baseColor: Vec4(0.05, 0.3, 0.35, 1), emissive: Vec3(0.1, 0.9, 1), emissiveStrength: 1.5),
        );

  final AssetRef floor, wall, pillar, gold, player, visor, hazard, gate, pad;
}

// ---- building blocks

Transform at(double x, double y, double z, {Vec3 scale = Vec3.one, Quat rotation = Quat.identity}) =>
    Transform(position: Vec3(x, y, z), rotation: rotation, scale: scale);

/// A static box: the cube primitive (its collider scales with it).
ObjectSpec box(String name, AssetRef material, Transform transform) => ObjectSpec.primitive(
      Primitive.cube,
      name: name,
      transform: transform,
      components: [MeshRenderer(mesh: Primitive.cube.mesh, material: material)],
    );

/// The sun and the sky (assets/textures/sky.hdr): every place has them.
List<ObjectSpec> lighting() => [
      ObjectSpec(
        'Sun',
        transform: Transform(
          position: const Vec3(0, 20, 0),
          rotation: Quat.axisAngle(Vec3.up, 35 * math.pi / 180) * Quat.axisAngle(Vec3.right, -52 * math.pi / 180),
        ),
        components: const [DirectionalLight(color: Vec3(1, 0.95, 0.85), intensity: 9000)],
      ),
      ObjectSpec(
        'Environment',
        components: [
          Environment(ambientColor: const Vec3(0.55, 0.65, 0.9), ambientIntensity: 600, sky: Assets.textures.sky, skyIntensity: 1500),
        ],
      ),
    ];

// ---- code prefabs

/// A coin of [kind]: a gold disc spinning in place, a `collect` zone that
/// listens to the player, and the [Collectible] that scores it (it takes
/// the kind's colour and size). [transform] places it. tool/scenes.dart
/// writes it as assets/prefabs/coin.prefab.json.
ObjectSpec coin(Look look, CoinKind kind, {String name = 'Coin', Transform transform = const Transform()}) => ObjectSpec(
      name,
      transform: transform,
      layer: 'trigger',
      tags: const ['pickup'],
      components: [
        const Rotator(speed: 2.5, space: MotionSpace.world),
        Collider(shape: ColliderShape.sphere, size: const Vec3(0.8, 0.8, 0.8), isSensor: true, mask: playerMask),
        const TriggerZone(event: 'collect', once: true),
      ],
      behaviours: [Collectible()..kind = kind],
      children: [
        ObjectSpec(
          'Disc',
          transform: Transform(
            position: const Vec3(0, 0.7, 0),
            rotation: Quat.axisAngle(Vec3.right, math.pi / 2),
            scale: const Vec3(0.7, 0.06, 0.7),
          ),
          components: [MeshRenderer(mesh: Primitive.cylinder.mesh, material: look.gold)],
        ),
      ],
    );

/// Six sparks that fly apart and go (`LifetimeDespawn`): what a coin
/// leaves. [Collectible] builds it where the coin was and kicks the pieces.
ObjectSpec burst(AssetRef material, Vec3 where) => ObjectSpec(
      'Burst',
      transform: Transform(position: where),
      children: [
        for (var i = 0; i < 6; i++)
          ObjectSpec(
            'Spark $i',
            layer: 'props',
            transform: Transform(
              position: Vec3(0.25 * math.cos(i * math.pi / 3), 0.6, 0.25 * math.sin(i * math.pi / 3)),
              scale: const Vec3(0.18, 0.18, 0.18),
            ),
            components: [
              MeshRenderer(mesh: Primitive.cube.mesh, material: material, castShadows: false),
              const RigidBody(mass: 0.05, angularDamping: 0.5),
              Collider(friction: 0.3, restitution: 0.4, mask: defaultMask),
              const LifetimeDespawn(seconds: 1.2),
            ],
          ),
      ],
    );

// ---- the places

/// Where the coins are, in the order the route takes them.
const coinSpots = [
  Vec3(0, 0, -3),
  Vec3(0, 0, -6.5),
  Vec3(4, 0, -9),
  Vec3(-12, 0, -9),
  Vec3(-15, 0, -9),
];

/// The level: a walled floor, five coins — the last a big one — a sweeper
/// to dodge, a gate that shuts the last two coins in and the switch that
/// opens it. [coinAt] makes a coin of a kind named `Coin n` at a spot: the
/// [coin] spec when building live, an instance of the written coin prefab
/// for the scenes.
List<ObjectSpec> level(
  Look look,
  CoinKinds kinds, {
  required ObjectSpec Function(String name, Vec3 at, CoinKind kind) coinAt,
}) => [
      ...lighting(),
      ObjectSpec(
        'Camera',
        transform: at(0, 4.5, 7),
        components: const [
          Camera(fovY: 0.95, far: 120),
          ThirdPersonCamera(target: 'Player', distance: 7, pitch: 0.45, minPitch: -0.2, maxPitch: 1.2, collisionRadius: 0.25),
          AudioListener(),
        ],
      ),
      ObjectSpec(
        'Player',
        layer: 'player',
        tags: const ['player'],
        components: [CharacterController(speed: 4, jumpSpeed: 5.5, maxStepHeight: 0.35, snapToGround: 0.25)],
        behaviours: [Player()],
        children: [
          ObjectSpec(
            'Body',
            transform: at(0, 0.9, 0, scale: const Vec3(0.8, 0.9, 0.8)),
            components: [MeshRenderer(mesh: Primitive.capsule.mesh, material: look.player)],
          ),
          ObjectSpec(
            'Visor',
            transform: at(0, 1.5, -0.33, scale: const Vec3(0.45, 0.14, 0.12)),
            components: [MeshRenderer(mesh: Primitive.cube.mesh, material: look.visor)],
          ),
        ],
      ),
      // The arena: x −17…17, z −17…9, walled.
      box('Floor', look.floor, at(0, -0.5, -4, scale: const Vec3(34, 1, 26))),
      box('Wall N', look.wall, at(0, 1, -17.25, scale: const Vec3(34, 3, 0.5))),
      box('Wall S', look.wall, at(0, 1, 9.25, scale: const Vec3(34, 3, 0.5))),
      box('Wall E', look.wall, at(17.25, 1, -4, scale: const Vec3(0.5, 3, 26))),
      box('Wall W', look.wall, at(-17.25, 1, -4, scale: const Vec3(0.5, 3, 26))),
      for (final (i, (x, z)) in const [(-16.0, 8.0), (16.0, 8.0), (16.0, -16.0), (-16.0, -16.0)].indexed)
        box('Pillar ${i + 1}', look.pillar, at(x, 2, z, scale: const Vec3(1, 4, 1))),
      // The cell behind the gate, with the last two coins.
      box('Cell N', look.wall, at(-13.25, 1, -11.25, scale: const Vec3(7.5, 3, 0.5))),
      box('Cell S', look.wall, at(-13.25, 1, -6.75, scale: const Vec3(7.5, 3, 0.5))),
      ObjectSpec.primitive(
        Primitive.cube,
        name: 'Gate',
        transform: at(-9.5, 1.2, -9, scale: const Vec3(0.6, 2.4, 4)),
        components: [MeshRenderer(mesh: Primitive.cube.mesh, material: look.gate)],
        behaviours: [Gate()],
      ),
      ObjectSpec(
        'Switch',
        layer: 'trigger',
        transform: at(-6, 0.06, -9, scale: const Vec3(1.6, 0.12, 1.6)),
        components: [
          MeshRenderer(mesh: Primitive.cylinder.mesh, material: look.pad),
          Collider(size: const Vec3(0.6, 6, 0.6), isSensor: true, mask: playerMask),
          const TriggerZone(event: 'switch'),
        ],
        behaviours: [Switch()],
      ),
      // The hazard: a blade sweeping across x 4.5…7.5 at z −2.
      ObjectSpec.primitive(
        Primitive.cube,
        name: 'Sweeper',
        transform: at(6, 0.9, -2, scale: const Vec3(3, 1.6, 0.4)),
        components: [
          MeshRenderer(mesh: Primitive.cube.mesh, material: look.hazard),
          const Oscillate(axis: Vec3(1, 0, 0), amplitude: 1.5, frequency: 0.25, space: MotionSpace.world),
        ],
        children: [
          ObjectSpec(
            'Danger',
            layer: 'trigger',
            components: [
              Collider(size: const Vec3(0.55, 0.55, 0.9), isSensor: true, mask: playerMask),
              const TriggerZone(event: 'hazard'),
            ],
          ),
        ],
      ),
      for (final (i, spot) in coinSpots.indexed)
        coinAt('Coin ${i + 1}', spot, i == coinSpots.length - 1 ? kinds.big : kinds.coin),
    ];

/// A podium with a big spinning coin, seen by a camera orbiting it: the
/// Menu's and the Results' backdrop.
List<ObjectSpec> _podium(Look look, {required double spin, required double orbit}) => [
      ...lighting(),
      box('Floor', look.floor, at(0, -0.5, 0, scale: const Vec3(40, 1, 40))),
      ObjectSpec.primitive(
        Primitive.cylinder,
        name: 'Podium',
        transform: at(0, 0.3, 0, scale: const Vec3(3, 0.3, 3)),
        components: [MeshRenderer(mesh: Primitive.cylinder.mesh, material: look.pillar)],
      ),
      ObjectSpec(
        'Big coin',
        transform: at(0, 2, 0, scale: const Vec3(2.2, 2.2, 2.2)),
        components: [Rotator(speed: spin, space: MotionSpace.world)],
        children: [
          ObjectSpec(
            'Disc',
            transform: Transform(rotation: Quat.axisAngle(Vec3.right, math.pi / 2), scale: const Vec3(0.7, 0.06, 0.7)),
            components: [MeshRenderer(mesh: Primitive.cylinder.mesh, material: look.gold)],
          ),
        ],
      ),
      ObjectSpec(
        'Camera pivot',
        transform: at(0, 1.2, 0),
        components: [Rotator(speed: orbit, space: MotionSpace.world)],
        children: [
          ObjectSpec(
            'Camera',
            transform: Transform(
              position: Vec3(0, 12 * math.sin(0.32), 12 * math.cos(0.32)),
              rotation: Quat.axisAngle(Vec3.right, -0.32),
            ),
            components: const [Camera(fovY: 0.95, far: 120), AudioListener()],
          ),
        ],
      ),
    ];

/// The title's backdrop.
List<ObjectSpec> menu(Look look) => _podium(look, spin: 0.8, orbit: 0.125);

/// The Results: the podium, the coin spinning faster, and the win jingle
/// from the load.
List<ObjectSpec> results(Look look) => [
      ..._podium(look, spin: 1.6, orbit: 0.2),
      ObjectSpec(
        'Jingle',
        components: [AudioSource(clip: Assets.audio.win, volume: 0.8, playing: true, priority: 5)],
      ),
    ];
