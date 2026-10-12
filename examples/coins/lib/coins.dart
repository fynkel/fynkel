// The game: walk where you tap and pick up the coins. The last coin of a
// level takes you to the next; your score and you go with you. Kenney's
// models (assets/CREDITS.md). A coin is a trigger the player walks into.

import 'dart:math' as math;

import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

class CoinsGame extends GameObject {
  /// Outlives the levels.
  final player = Player();
  final _levels = [Level.meadow, Level.dusk];
  int _level = -1;
  Level? _current;

  @override
  void onLoad() {
    super.onLoad();
    add(player);
    add(FollowCamera(player));
    nextLevel();
  }

  /// Adds the next level and, once it's there, destroys this one, so
  /// there's always a level on screen.
  Future<void> nextLevel() async {
    final old = _current;
    _level = (_level + 1) % _levels.length;
    await add(_current = _levels[_level]()).loaded;
    old?.destroy();
  }
}

/// One level: its sky, light, ground and props, eight coins, a swarm of
/// motes, and what the player carries here. A tap on anything in it walks
/// the player there.
class Level extends GameObject {
  Level._(this.sky, this.sun, this.ground, this.props, this.carried, this.motes, this.seed);

  Level.meadow()
    : this._(
        Color.rgb(0.55, 0.75, 0.95),
        Color.white,
        Color.rgb(0.3, 0.45, 0.22),
        ['tree_oak', 'tree_default', 'plant_bush', 'flower_redA'],
        'flower_yellowA',
        Color.rgb(1, 1, 0.8),
        1,
      );

  Level.dusk()
    : this._(
        Color.rgb(0.3, 0.22, 0.4),
        Color.rgb(1, 0.8, 0.65),
        Color.rgb(0.22, 0.25, 0.3),
        ['tree_pineRoundA', 'rock_largeA', 'rock_smallC', 'stump_round', 'log'],
        'mushroom_red',
        Color.rgb(0.6, 1, 0.4),
        2,
      );

  final Color sky, sun, ground, motes;
  final List<String> props;
  final String carried;
  final int seed;

  @override
  void onLoad() {
    super.onLoad();
    final random = math.Random(seed);
    Vec3 somewhere(double near, double far) {
      final angle = random.nextDouble() * 2 * math.pi, radius = near + random.nextDouble() * (far - near);
      return Vec3(math.cos(angle) * radius, 0, math.sin(angle) * radius);
    }

    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.3))),
        components: [Light.sun(color: sun, intensity: 2.5)],
      ),
    );
    add(GameObject(components: [Light.ambient(color: sky, intensity: 0.6)]));
    final groundMaterial = Material(color: ground, roughness: 1);
    add(
      GameObject(
        transform: Transform(scale: Vec3(40, 1, 40)),
        components: [MeshRenderer(Mesh.plane(), groundMaterial)],
      ),
    );
    for (final asset in props) {
      for (var i = 0; i < 4; i++) {
        add(Prop(asset, at: somewhere(10, 16), turn: random.nextDouble() * 2 * math.pi));
      }
    }
    final gold = Material(
      color: Color.rgb(1, 0.75, 0.2),
      roughness: 0.3,
      emissive: Color.rgb(1, 0.6, 0.1),
      emissiveIntensity: 0.4,
    );
    for (var i = 0; i < 8; i++) {
      add(Coin(somewhere(2, 8) + Vec3(0, 0.7, 0), gold));
    }
    add(Swarm(300, motes));
  }

  @override
  void onStart() {
    super.onStart();
    world.first<FollowCamera>()!.get<Camera>()!.clearColor = sky;
    world.first<Player>()!
      ..transform = Transform()
      ..walkTo(Vec3.zero)
      ..carry(Prop(carried, scale: 4));
  }

  @override
  bool onClick(Finger finger) {
    world.first<Player>()!.walkTo(finger.over!.point);
    return true;
  }
}

/// One of Kenney's Nature Kit models.
class Prop extends GameObject {
  Prop(this.asset, {Vec3 at = Vec3.zero, double turn = 0, double scale = 3})
    : super(
        transform: Transform(position: at, rotation: Quat.axisAngle(Vec3.up, turn), scale: Vec3(scale, scale, scale)),
      );

  final String asset;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    add(await assets.instantiate('props/$asset.glb'));
  }
}

/// Turns its object about its up axis.
class Spin extends Behaviour {
  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, 3 * time.delta);
  }
}

/// Makes its object rise and fall about where it started.
class Bob extends Behaviour {
  late final double _height = position.y;
  double _time = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    _time += time.delta;
    position = Vec3(position.x, _height + 0.15 * math.sin(3 * _time), position.z);
  }
}

class Coin extends GameObject {
  Coin(Vec3 at, Material gold)
    : super(
        transform: Transform(position: at, scale: Vec3(0.6, 0.6, 0.15)),
        components: [
          MeshRenderer(Mesh.sphere(), gold),
          // 0.5 m across its middle: the player picks it up 0.9 m away.
          SphereCollider(radius: 0.85, isTrigger: true),
          Spin(),
          Bob(),
        ],
      );
}

class Player extends GameObject {
  /// The coins picked up, for the HUD.
  final score = ValueNotifier(0);
  late final GameObject _hand;
  GameObject? _carried;
  Vec3 _goal = Vec3.zero;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    _hand = add(await assets.instantiate('character.glb')).find('arm-right')!;
    // Kinematic: it walks where it's told, and meets the coins' triggers.
    addComponent(CapsuleCollider(radius: 0.4, height: 1.6, center: Vec3(0, 0.8, 0)));
    addComponent(Rigidbody(isKinematic: true));
    addComponent(Collector());
  }

  void walkTo(Vec3 point) => _goal = Vec3(point.x, 0, point.z);

  /// Puts down what it carries and takes [item] in its right hand.
  void carry(GameObject item) {
    _carried?.destroy();
    _carried = _hand.add(item)..localPosition += Vec3(0, -0.7, 0.45);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final to = _goal - transform.position;
    if (to.length < 0.05) return;
    final step = math.min(to.length, 5 * time.delta);
    transform = Transform(
      position: transform.position + to.normalized * step,
      rotation: Quat.axisAngle(Vec3.up, math.atan2(to.x, to.z)),
    );
  }
}

/// Picks up the coins the player walks into. A behaviour of its own,
/// kept apart from the walking.
class Collector extends Behaviour with Triggers {
  @override
  void onTriggerEnter(Collider other) {
    if (other.gameObject case final Coin coin when !coin.isDestroyed) {
      coin.destroy();
      (gameObject as Player).score.value++;
      if (world.all<Coin>().isEmpty) gameObject.ancestor<CoinsGame>()!.nextLevel();
    }
  }
}

/// Behind and above the player, looking at it: a [Follow] and a [LookAt].
class FollowCamera extends GameObject {
  FollowCamera(GameObject player)
    : super(
        components: [
          Camera(fovY: 50.deg),
          Follow(player, offset: Vec3(0, 7, 9)),
          LookAt(player, offset: Vec3(0, 1, 0)),
        ],
      );
}

/// A few hundred plain objects, motes drifting over the level, moved by
/// this one object's [onUpdate] rather than each by its own.
class Swarm extends GameObject {
  Swarm(this.count, this.color);

  final int count;
  final Color color;
  final _motes = <GameObject>[];
  final _orbits = <(double radius, double height, double speed, double phase)>[];
  double _time = 0;

  @override
  void onLoad() {
    super.onLoad();
    final random = math.Random(count), glow = Material(color: color, emissive: color, emissiveIntensity: 3);
    for (var i = 0; i < count; i++) {
      final speed = (0.1 + 0.3 * random.nextDouble()) * (i.isEven ? 1 : -1);
      _orbits.add((
        2 + 12 * random.nextDouble(),
        0.5 + 3 * random.nextDouble(),
        speed,
        2 * math.pi * random.nextDouble(),
      ));
      _motes.add(add(GameObject(components: [MeshRenderer(Mesh.cube(), glow)])));
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _time += time.delta;
    for (var i = 0; i < count; i++) {
      final (radius, height, speed, phase) = _orbits[i];
      final angle = phase + speed * _time;
      _motes[i].transform = Transform(
        position: Vec3(radius * math.cos(angle), height + 0.3 * math.sin(2 * angle), radius * math.sin(angle)),
        scale: const Vec3(0.06, 0.06, 0.06),
      );
    }
  }
}
