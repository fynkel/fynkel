// package:fynkel_physics's example, for now on the raw world of stage 5
// step 1 (docs/stage-5.md): a pile of cubes that fall, tumble and settle,
// dropped again every 8 seconds. Each fixed step's moved cubes are written
// to their objects. lib/measured.dart runs the same for the tool, with the
// step's timings.
//
//   flutter run --dart-define=TUMBLE_CUBES=500 [--dart-define=TUMBLE_PHYSICS=0]
//     [--dart-define=TUMBLE_THREAD=1|2] [--dart-define=TUMBLE_RAYS=10]

import 'dart:math' as math;

import 'package:flutter/scheduler.dart' show SchedulerBinding;
import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_physics/raw.dart';

void main() => runApp(FynkelApp(Tumble.new));

const cubeCount = int.fromEnvironment('TUMBLE_CUBES', defaultValue: 1000);

/// `--dart-define=TUMBLE_PHYSICS=0` leaves the cubes where they're dropped:
/// the same scene without the physics, to tell its cost from drawing's.
const stepping = bool.fromEnvironment('TUMBLE_PHYSICS', defaultValue: true);

/// `--dart-define=TUMBLE_THREAD=1` steps on the physics' own thread from
/// the fixed step: each fixed step writes the poses of the step before and
/// starts the next, so the step runs alongside the rest of the frame and
/// the cubes show a step late. A query in between waits for it.
///
/// `--dart-define=TUMBLE_THREAD=2` starts the step once the frame is done
/// (after Flutter's frame), so it runs between frames; the next frame
/// writes its poses before any update. Queries in updates never wait.
const threadMode = int.fromEnvironment('TUMBLE_THREAD');

/// `--dart-define=TUMBLE_RAYS=10` casts that many rays down onto the pile
/// in every update, as a game's ground checks would.
const rayCount = int.fromEnvironment('TUMBLE_RAYS');
const stepSeconds = 1 / 50;
const size = 0.5;

class Tumble extends GameObject {
  Tumble({this.count = cubeCount});

  final int count;
  final physics = RawPhysics();
  final cubes = <GameObject>[];
  final bodies = <int>[];
  var _sinceDrop = 0.0;

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 14, 22), rotation: Quat.lookRotation(Vec3(0, -10, -22))),
        components: [Camera()],
      ),
    );
    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.3))),
        components: [Light.sun()],
      ),
    );
    add(
      GameObject(
        transform: Transform(scale: Vec3(40, 1, 40)),
        components: [MeshRenderer(Mesh.plane(), Material(color: Color(0xFF3A4A5A)))],
      ),
    );
    physics.addBox(0, Vec3(20, 0.5, 20), position: Vec3(0, -0.5, 0));
    final material = Material(color: Color(0xFFE0613A));
    for (var i = 0; i < count; i++) {
      final cube = add(
        GameObject(
          transform: Transform(scale: Vec3(size, size, size)),
          components: [MeshRenderer(Mesh.cube(), material)],
        ),
      );
      final body = physics.addBody(BodyKind.dynamic, i);
      physics.addBox(body, Vec3(size / 2, size / 2, size / 2));
      cubes.add(cube);
      bodies.add(body);
    }
    drop();
  }

  @override
  void onDestroy() {
    physics.dispose();
    super.onDestroy();
  }

  /// Puts every cube back in the air: columns of 10 over a square, each
  /// a little turned so the pile tumbles.
  void drop() {
    _collect();
    final side = math.sqrt(count / 10).ceil();
    for (var i = 0; i < count; i++) {
      final column = i ~/ 10;
      final position = Vec3(
        (column % side - side / 2) * size * 1.6,
        2 + (i % 10) * size * 1.4,
        (column ~/ side - side / 2) * size * 1.6,
      );
      final rotation = Quat.axisAngle(Vec3(1, 1, 0).normalized, i * 0.37);
      physics.setBodyPose(bodies[i], position, rotation);
      cubes[i].transform = Transform(position: position, rotation: rotation, scale: Vec3(size, size, size));
    }
    _sinceDrop = 0;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    // A query needs the step done: with threadMode 2 it is by now.
    if (rayCount > 0 || threadMode == 2) _collect();
    _sinceDrop += time.delta;
    if (_sinceDrop > 8) drop();
    for (var i = 0; i < rayCount; i++) {
      hits += physics.raycast(Vec3(i * 0.7 - rayCount * 0.35, 12, i * 0.3 - 2), const Vec3(0, -1, 0), 30) == null
          ? 0
          : 1;
    }
  }

  /// How many rays hit, so they aren't optimised away.
  var hits = 0;

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    if (!stepping) return;
    switch (threadMode) {
      case 0:
        stepAndWrite(physics, cubes);
      case 1:
        _collect();
        physics.beginStep(stepSeconds);
        _running = true;
      default:
        _collect();
        _due++;
        if (_scheduled) return;
        _scheduled = true;
        SchedulerBinding.instance.addPostFrameCallback((_) => _startDue());
    }
  }

  var _running = false;
  var _scheduled = false;
  var _due = 0;

  /// The running step's poses written, if one runs.
  void _collect() {
    if (!_running) return;
    write(physics, cubes, physics.endStep());
    _running = false;
  }

  /// After the frame: the steps it was due, the last on the physics' thread.
  void _startDue() {
    _scheduled = false;
    if (!isAlive) return;
    for (; _due > 1; _due--) {
      stepAndWrite(physics, cubes);
    }
    if (_due == 0) return;
    _due = 0;
    physics.beginStep(stepSeconds);
    _running = true;
  }
}

/// One fixed step, and each moved cube's pose written to its object.
void stepAndWrite(RawPhysics physics, List<GameObject> cubes) => write(physics, cubes, physics.step(stepSeconds));

/// The [moved] cubes' poses written to their objects.
void write(RawPhysics physics, List<GameObject> cubes, int moved) {
  for (var i = 0; i < moved; i++) {
    cubes[physics.movedKey(i)].transform = Transform(
      position: physics.movedPosition(i),
      rotation: physics.movedRotation(i),
      scale: Vec3(size, size, size),
    );
  }
}
