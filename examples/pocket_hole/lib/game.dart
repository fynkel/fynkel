import 'dart:math' as math;
import 'package:flutter/foundation.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

enum PlayState { title, playing, paused, cleared }

const palette = [0xFFFF598F, 0xFFFFBA43, 0xFFFFE65B, 0xFF46D6A0, 0xFF42BFF0, 0xFF8D77EF];

/// Deterministic layouts: every collectible is reachable inside the board.
List<Vec3> layout(int level) {
  final result = <Vec3>[];
  final style = (level - 1) % 4;
  for (var group = 0; group < 6; group++) {
    final cx = group.isEven ? -1.65 : 1.65;
    final cz = -3.4 + (group ~/ 2) * 2.5;
    for (var x = 0; x < 4; x++) {
      for (var y = 0; y < (style == 0 ? 2 : 3); y++) {
        if (style == 2 && x == y) continue;
        result.add(Vec3(cx + (x - 1.5) * .39, .2 + y * .4, cz));
        if (style == 1 || style == 3) {
          result.add(Vec3(cx + (x - 1.5) * .39, .2 + y * .4, cz + .43));
        }
      }
    }
  }
  return result;
}

class PocketGame extends GameObject {
  PocketGame({this.level = 1, this.onCompleted, this.onCollect});
  final void Function(int)? onCompleted;
  final VoidCallback? onCollect;
  final revision = ValueNotifier(0);
  PlayState state = PlayState.title;
  int level;
  int collected = 0;
  int total = 0;
  double elapsed = 0;
  Vec3 target = Vec3(0, 0, 4);
  late GameObject hole;
  late GameObject floor;
  static const holeRadius = .715;
  final blocks = <PocketBlock>[];
  double get progress => total == 0 ? 0 : collected / total;
  void notify() => revision.value++;

  GameObject shape(Vec3 at, Vec3 size, int color, {bool round = false}) => add(
    GameObject(
      transform: Transform(position: at, scale: size),
      components: [MeshRenderer(round ? Mesh.cylinder() : Mesh.cube(), Material(color: Color(color), roughness: .8))],
    ),
  );

  @override
  void onLoad() {
    super.onLoad();
    time.fixedDelta = 1 / 120;
    physics.defaultSolverIterations = 8;
    physics.gravity = Vec3(0, -24, 0);
    floor = add(
      GameObject(
        name: 'floor with circular opening',
        components: [
          MeshCollider(mesh: openingMesh(), material: const PhysicsMaterial(dynamicFriction: .28, bounciness: .02)),
        ],
      ),
    );
    add(
      GameObject(
        transform: Transform.lookAt(Vec3(0, 13.5, 10), Vec3(0, 0, -.3)),
        components: [Camera(orthographic: true, orthographicSize: 8.3, clearColor: Color(0xFFE7E9FF), showSky: false)],
      ),
    );
    add(GameObject(components: [Light.sun(direction: Vec3(-.5, -1, -.4), intensity: 2, shadows: true)]));
    add(GameObject(components: [Light.ambient(intensity: .8)]));
    shape(Vec3(0, -.32, 0), Vec3(6.8, .6, 12.3), 0xFFB69AE5);
    shape(Vec3(0, -.025, 0), Vec3(6.65, .06, 12.1), 0xFFD6B7F5);
    for (final x in [-3.35, 3.35]) {
      shape(Vec3(x, .12, 0), Vec3(.14, .3, 12.3), 0xFFBD91E3);
      add(
        GameObject(
          transform: Transform(position: Vec3(x, .8, 0), scale: Vec3(.14, 2, 12.3)),
          components: [BoxCollider()],
        ),
      );
    }
    for (final z in [-6.1, 6.1]) {
      shape(Vec3(0, .12, z), Vec3(6.8, .3, .14), 0xFFBD91E3);
      add(
        GameObject(
          transform: Transform(position: Vec3(0, .8, z), scale: Vec3(6.8, 2, .14)),
          components: [BoxCollider()],
        ),
      );
    }
    hole = add(GameObject(transform: Transform(position: target)));
    hole.add(
      GameObject(
        transform: Transform(position: Vec3(0, .025, 0), scale: Vec3(1.65, .012, 1.65)),
        components: [MeshRenderer(Mesh.cylinder(), Material(color: Color(0xFF8B79A8)))],
      ),
    );
    hole.add(
      GameObject(
        transform: Transform(position: Vec3(0, .045, 0), scale: Vec3(1.43, .012, 1.43)),
        components: [MeshRenderer(Mesh.cylinder(), Material(color: Color(0xFF090A19), emissive: Color(0xFF090A19)))],
      ),
    );
    reset();
  }

  void reset() {
    for (final b in blocks) {
      b.destroy();
    }
    blocks.clear();
    final positions = layout(level);
    total = positions.length;
    collected = 0;
    elapsed = 0;
    target = Vec3(0, 0, 4.5);
    hole.position = target;
    floor.position = target;
    time.scale = state == PlayState.playing ? 1 : 0;
    for (var i = 0; i < positions.length; i++) {
      blocks.add(add(PocketBlock(positions[i], palette[i % palette.length])));
    }
    notify();
  }

  void start() {
    state = PlayState.playing;
    reset();
  }

  void pause() {
    if (state == PlayState.playing) {
      state = PlayState.paused;
      time.scale = 0;
      notify();
    }
  }

  void resume() {
    if (state == PlayState.paused) {
      state = PlayState.playing;
      time.scale = 1;
      notify();
    }
  }

  void next() {
    level++;
    start();
  }

  void move(double dx, double dy) {
    if (state != PlayState.playing) return;
    target = Vec3((target.x + dx).clamp(-2.55, 2.55), 0, (target.z + dy).clamp(-5.2, 5.2));
    hole.position = target;
  }

  /// A reusable triangle ring with a real circular opening. Its outer
  /// edge stays beyond the entire board at every allowed hole position.
  static Mesh openingMesh() {
    const segments = 64;
    final vertices = <Vec3>[];
    final triangles = <int>[];
    for (var i = 0; i < segments; i++) {
      final a = i * 2 * math.pi / segments;
      vertices.add(Vec3(math.cos(a) * holeRadius, 0, math.sin(a) * holeRadius));
      vertices.add(Vec3(math.cos(a) * 20, 0, math.sin(a) * 20));
    }
    for (var i = 0; i < segments; i++) {
      final a = i * 2, b = (i + 1) % segments * 2;
      triangles.addAll([a, b, a + 1, b, b + 1, a + 1]);
    }
    return Mesh.custom(positions: vertices, indices: triangles);
  }

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    if (state != PlayState.playing) return;
    final previous = floor.position;
    hole.position = target;
    // Apply the opening at the next physics tick, with no follow easing.
    floor.position = target;
    if ((target - previous).length > .00001) {
      final minX = math.min(previous.x, target.x) - 1.2;
      final maxX = math.max(previous.x, target.x) + 1.2;
      final minZ = math.min(previous.z, target.z) - 1.2;
      final maxZ = math.max(previous.z, target.z) + 1.2;
      for (final block in blocks) {
        final p = block.position;
        if (!block.done && p.x >= minX && p.x <= maxX && p.z >= minZ && p.z <= maxZ) {
          block.get<Rigidbody>()!.wakeUp();
        }
      }
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (state != PlayState.playing) return;
    elapsed += time.delta;
    var changed = false;
    for (final block in blocks) {
      if (block.done) continue;
      // Collection follows an actual fall, never proximity or a timer.
      if (block.position.y < -.7) {
        block.done = true;
        block.active = false;
        collected++;
        changed = true;
      }
    }
    if (changed) {
      onCollect?.call();
      if (collected == total) {
        state = PlayState.cleared;
        time.scale = 0;
        onCompleted?.call(level + 1);
      }
      notify();
    }
  }
}

class PocketBlock extends GameObject {
  PocketBlock(Vec3 at, int color)
    : super(
        transform: Transform(position: at, scale: Vec3.one * .37),
        components: [
          MeshRenderer(Mesh.cube(), Material(color: Color(color), roughness: .65)),
          BoxCollider(material: const PhysicsMaterial(dynamicFriction: .28, bounciness: .05)),
          Rigidbody(
            mass: .15,
            angularDamping: .06,
            interpolation: RigidbodyInterpolation.interpolate,
            collisionDetectionMode: CollisionDetectionMode.continuous,
          ),
        ],
      );
  bool done = false;
}
