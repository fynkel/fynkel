// Stage 14, phase 3: all of it together. One scene with every system on
// at once, as a game has them: a skinned crowd, boxes falling and
// tumbling, sparks bouncing off the floor and a ball with a trail,
// sounds circling in 3D, a lava plinth in its own shader, lamps and
// shadows, water and a flag changed every frame, and a road swept again
// every frame as a knot moves. Each part can be turned off, to see what
// it costs, and the quality level picked.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';
import 'meshes_change.dart' show Flag, Water;
import 'particles_look.dart' show softDot;
import 'sound_3d.dart' show boom, buzz;
import 'splines.dart' show Road;

/// The parts, by the names their switches have.
const allParts = ['crowd', 'boxes', 'sparks', 'sounds', 'lamps', 'water', 'road'];

class AllTogetherScene extends LabScene {
  /// The scene with [quality] picked (by name; empty for the device's).
  AllTogetherScene({this.quality = ''});

  final String quality;

  @override
  String get title => 'All of it together';
  @override
  String get step => 'stage 14';
  @override
  String get about =>
      'Every system on at once: 12 skinned characters, boxes falling on the floor, sparks bouncing off it and a ball '
      'with a trail, 8 sounds circling in 3D with a boom each time a box lands, a lava plinth in its own shader, '
      'four lamps and the sun\'s shadows, water and a flag changed every frame, and a road swept again every frame. '
      'Turn a part off to see what it costs.';

  @override
  Vec3 get eye => Vec3(0, 7, 13);
  @override
  Vec3 get target => Vec3(0, 0.5, -1);

  final _parts = <String, GameObject>{};
  final _boxes = <Rigidbody>[];
  final _bees = <GameObject>[];
  final _clips = <AudioClip>[];
  final _textures = <Texture2D>[];
  late final Road road;
  late final AudioClip _boom;
  var _seconds = 0.0, _landed = 0, _sparkHits = 0, _fps = '–';
  final _people = <GameObject>[];

  /// The crowd, once loaded.
  List<GameObject> get people => _people;

  /// How many sparks have hit the floor.
  int get sparkHits => _sparkHits;

  /// How many boxes have landed, each with a boom.
  int get landed => _landed;

  /// Whether [part] (one of [allParts]) is on.
  bool isOn(String part) => _parts[part]!.active;
  void turn(String part, bool on) => _parts[part]!.active = on;

  GameObject _part(String name) => _parts[name] = add(GameObject(name: name));

  @override
  void build() {
    if (quality.isNotEmpty) world.quality = Quality.levels.firstWhere((q) => q.name == quality);
    find('sun')!.get<Light>()!.shadows = true;
    add(floorCollider());
    final ground = add(GameObject(name: 'ground plane'));
    _part('crowd');
    _boxesPart();
    _sparksPart(ground);
    _soundsPart();
    _lampsPart();
    _part('water')
      ..add(Water()..transform = Transform(position: Vec3(4.5, 0.05, 1.5)))
      ..add(Flag()..transform = Transform(position: Vec3(5, 0, -3)));
    road = _part('road').add(Road());
    add(shape('lava', Mesh.cube(), const Color(0xFF802010), at: Vec3(0, 0.4, 2.6), size: Vec3(2.2, 0.8, 0.8)));
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    final lava = await assets.shader('shaders/lava.wgsl');
    if (isDestroyed) return;
    find('lava')!.get<MeshRenderer>()!.material = Material(
      shader: lava,
      values: {'hot': const Color(0xFFFF6A10), 'scale': 3.0, 'flow': 0.3},
      uvScale: Vec2(2.5, 0.8),
    );
    final crowd = _parts['crowd']!;
    for (var i = 0; i < 12; i++) {
      final made = await assets.instantiate(i.isEven ? 'kaykit/knight.glb' : 'kaykit/rogue.glb');
      if (isDestroyed) return;
      final person = crowd.add(made)..position = Vec3((i % 6 - 2.5) * 1.3, 0, -1.5 - (i ~/ 6) * 1.4);
      _people.add(person);
      final animator = person.animator!;
      final clips = animator.clipNames.toList();
      final random = math.Random(i);
      animator
        ..play(clips[random.nextInt(clips.length)])
        ..time = random.nextDouble() * animator.length;
    }
  }

  void _boxesPart() {
    final boxes = _part('boxes');
    for (var i = 0; i < 8; i++) {
      final box = boxes.add(
        shape(
          'box $i',
          Mesh.cube(),
          Color.lerp(const Color(0xFFE0613A), const Color(0xFF3A8EE0), i / 7),
          size: Vec3(0.5, 0.5, 0.5),
          components: [BoxCollider(), Rigidbody(), _Landing(_land)],
        ),
      );
      _boxes.add(box.get<Rigidbody>()!);
      _drop(i);
    }
  }

  /// Box [i] up in the air again, somewhere over the left of the floor.
  void _drop(int i) {
    final random = math.Random(i * 31 + _landed);
    _boxes[i].gameObject.transform = Transform(
      position: Vec3(-6 + random.nextDouble() * 3, 3 + random.nextDouble() * 3, random.nextDouble() * 3),
      rotation: Quat.axisAngle(Vec3(1, 0, 1).normalized, random.nextDouble() * 3),
      scale: Vec3(0.5, 0.5, 0.5),
    );
    _boxes[i]
      ..linearVelocity = Vec3.zero
      ..angularVelocity = Vec3.zero;
  }

  void _land(GameObject box) {
    if (!isOn('sounds')) return;
    _landed++;
    world.audio.playAt(_boom, box.position, volume: 0.3, maxDistance: 40);
  }

  void _sparksPart(GameObject ground) {
    final dot = softDot();
    _textures.add(dot);
    final sparks = _part('sparks');
    sparks.add(
      GameObject(
        name: 'fountain',
        transform: Transform(position: Vec3(-2, 2.5, 2), rotation: Quat.axisAngle(Vec3.right, math.pi)),
        components: [
          ParticleSystem(
            randomSeed: 21,
            startLifetime: const Range(1.5, 2.2),
            startSpeed: const Range(2, 4),
            startSize: const Range(0.05, 0.09),
            startColor: const Color(0xFFFFC060),
            gravityModifier: 1,
            simulationSpace: ParticleSpace.world,
            emission: const Emission(rateOverTime: 120),
            shape: ParticleShape.cone(angle: 25.deg, radius: 0.05),
            collision: ParticleCollision(
              planes: [ground],
              bounce: const ParticleCurve(0.5),
              sendCollisionMessages: true,
            ),
            onParticleCollision: (_, count) => _sparkHits += count,
            colorOverLifetime: ColorGradient.fadeOut(),
            material: Material(baseColorTexture: dot, blending: Blending.additive),
          ),
        ],
      ),
    );
    sparks.add(
      shape(
        'trailed ball',
        Mesh.sphere(),
        const Color(0xFFE0613A),
        size: Vec3(0.35, 0.35, 0.35),
        components: [TrailRenderer(time: 0.8, widthMultiplier: 0.25, widthCurve: (t) => 1 - t)],
      ),
    );
  }

  void _soundsPart() {
    final buzzing = buzz();
    _boom = boom();
    _clips.addAll([buzzing, _boom]);
    final sounds = _part('sounds');
    for (var i = 0; i < 8; i++) {
      final bee = sounds.add(
        shape(
          'bee $i',
          Mesh.sphere(),
          const Color(0xFFF2D04A),
          size: Vec3(0.15, 0.15, 0.15),
          components: [
            AudioSource(clip: buzzing, loop: true, spatialBlend: 1, volume: 0.1, pitch: 0.7 + i * 0.1, maxDistance: 25),
          ],
        ),
      );
      _bees.add(bee);
    }
  }

  void _lampsPart() {
    final lamps = _part('lamps');
    const colours = [Color(0xFFFF7040), Color(0xFF40A0FF), Color(0xFF70FF70), Color(0xFFFF60D0)];
    for (var i = 0; i < 4; i++) {
      lamps.add(
        GameObject(
          name: 'lamp $i',
          transform: Transform(position: Vec3(-5 + i * 3.3, 1.2, 0.8)),
          components: [Light.point(color: colours[i], intensity: 4, range: 5)],
        ),
      );
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _seconds += time.delta;
    final t = _seconds;
    // The bees circle the middle, the ball goes round the fountain.
    for (var i = 0; i < _bees.length; i++) {
      final a = t * (0.4 + i * 0.05) + i * math.pi / 4;
      _bees[i].position = Vec3(math.cos(a) * (3 + i * 0.3), 1 + 0.4 * math.sin(t + i), math.sin(a) * 2.5);
    }
    _parts['sparks']!.find('trailed ball')!.position = Vec3(-2 + math.cos(t * 2) * 1.2, 0.2, 2 + math.sin(t * 2));
    // A knot of the road moves, so it's swept again each frame.
    road.spline[2] = Knot(Vec3(-3 + math.sin(t) * 0.8, 0.3, -1.6 + math.cos(t * 0.7) * 0.6));
    // Boxes that came to rest, or fell off, go up again.
    for (var i = 0; i < _boxes.length; i++) {
      final body = _boxes[i];
      if (body.isSleeping || body.gameObject.position.y < -5) _drop(i);
    }
    final quality = world.quality?.name ?? 'the device\'s (${Quality.device.name})';
    readout.value = 'quality $quality   $_landed boxes landed   ${world.audio.stats.voices} voices   $_fps fps';
  }

  @override
  void onDestroy() {
    super.onDestroy();
    for (final clip in _clips) {
      clip.dispose();
    }
    for (final texture in _textures) {
      texture.dispose();
    }
  }

  @override
  List<Control> get controls => [
    for (final part in allParts) Toggle(part, () => isOn(part), (on) => turn(part, on)),
    for (final level in Quality.levels) Button(level.name, () => world.quality = level),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('all together', s));
}

/// Tells [landed] when its box first touches something after a drop.
class _Landing extends Behaviour with Collisions {
  _Landing(this.landed);

  final void Function(GameObject box) landed;

  @override
  void onCollisionEnter(Collision collision) => landed(gameObject);
}
