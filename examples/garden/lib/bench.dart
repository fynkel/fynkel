// The phone numbers steps 1 and 5 left for the garden (docs/stage-2.md §6):
// the world's extract with 10,000 objects in trees, and raycasts over
// 10,000 cubes. The scenes are the Rust probes' (crates/fynkel_ffi
// extract_probe, crates/fynkel_world/examples/raycast.rs), measured here
// through package:fynkel on the UI isolate, as a game would run them.

import 'package:fynkel/fynkel.dart';

/// Each result as a line, for Measurement.report.
List<String> bench() => [..._extract(), ..._raycasts()];

/// `view.frame()` copies the world for the render worker (the extract)
/// and hands it over without waiting for the GPU: its time is the
/// extract's, plus the handover. Median and slowest of 300 steady frames.
List<String> _extract() {
  final lines = <String>[];
  for (final (name, parentOf) in [
    ('100 × 100 wide', (int i) => i % 100 == 0 ? null : i - i % 100),
    ('100 chains 100 deep', (int i) => i % 100 == 0 ? null : i - 1),
    ('no tree', (int i) => null),
  ]) {
    final world = World();
    world.spawn(Camera(), Transform(position: Vec3(0, 1, 4), lookAt: Vec3.zero));
    world.spawn(Light.sun(), Transform(forward: Vec3(-0.4, -1, -0.3)));
    final renderer = MeshRenderer(Mesh.cube(), Material());
    final objects = [
      for (var i = 0; i < 10000; i++)
        world.spawn(Transform(position: Vec3(0, 0.01, 0.1), rotation: Quat.axisAngle(Vec3.up, i * 1e-3)), renderer),
    ];
    for (var i = 0; i < objects.length; i++) {
      if (parentOf(i) case final p?) objects[i].parent = objects[p];
    }
    final view = HeadlessView(world, width: 64, height: 64);
    for (var i = 0; i < 20; i++) {
      view.frame();
    }
    final times = <int>[];
    final clock = Stopwatch();
    for (var i = 0; i < 300; i++) {
      clock
        ..reset()
        ..start();
      view.frame();
      times.add(clock.elapsedMicroseconds);
    }
    view.dispose();
    world.dispose();
    times.sort();
    lines.add('extract, 10,000 objects, $name: median ${_ms(times[times.length ~/ 2])}, slowest ${_ms(times.last)}');
  }
  return lines;
}

/// The grid of the Rust example, flat and in chains, through
/// package:fynkel's `World.raycast`.
List<String> _raycasts() {
  final lines = <String>[];
  for (final deep in [false, true]) {
    final world = World();
    final cube = Mesh.cube();
    final material = Material();
    (GameObject, Vec3)? previous;
    for (var i = 0; i < 100; i++) {
      for (var j = 0; j < 100; j++) {
        final position = Vec3(i * 2.0 - 99, 0, j * 2.0 - 99);
        // Deep, each row is a chain of 100, each cube under the one before.
        final chained = deep && j > 0;
        final local = chained ? position - previous!.$2 : position;
        final object = world.spawn(Transform(position: local), MeshRenderer(cube, material));
        if (chained) object.parent = previous!.$1;
        previous = (object, position);
      }
    }
    final hit = Ray(Vec3(1, 50, 1), Vec3(0, -1, 0.01));
    final miss = Ray(Vec3(0.3, 50, 0.3), Vec3.up);
    if (world.raycast(hit) == null || world.raycast(miss) != null) throw StateError('the bench rays are wrong');
    lines.add(
      'raycast, 100 × 100 cubes${deep ? ' in chains of 100' : ''}: '
      'hit ${castTimes(world, hit)}; miss ${castTimes(world, miss)}',
    );
    world.dispose();
  }
  return lines;
}

/// Best and median of 200 casts of [ray] in [world].
String castTimes(World world, Ray ray) {
  final all = <int>[];
  final clock = Stopwatch();
  for (var i = 0; i < 200; i++) {
    clock
      ..reset()
      ..start();
    world.raycast(ray);
    all.add(clock.elapsedMicroseconds);
  }
  all.sort();
  return 'best ${all.first} µs, median ${all[all.length ~/ 2]} µs';
}

String _ms(int micros) => '${(micros / 1000).toStringAsFixed(2)} ms';
