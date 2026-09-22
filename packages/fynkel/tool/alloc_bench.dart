// Dart allocations and GC per frame on the M1 test scene (blueprint §15,
// M1 gate: "steady-state Dart allocation per frame is zero or documented").
//
// Runs the hub + satellites scene (the same motion as hosts/headless and the
// Flutter host, plus one `find` and one typed `get` per tick — what a
// behaviour does) on package:fynkel for a warm-up and a sample window, and
// reads the VM's allocation profile and GC timeline over the sample through
// the service protocol. JIT numbers (`dart run`); AOT (Flutter release)
// allocates the same objects, its GC pauses differ.
//
//   dart run tool/alloc_bench.dart [--frames 600] [--satellites 24] [--json]
//
// Prints one `[alloc]` line that benchmarks/m0/capture.py parses.

import 'dart:convert';
import 'dart:developer' as dev;
import 'dart:io';
import 'dart:isolate' as iso;
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:vm_service/vm_service.dart';
import 'package:vm_service/vm_service_io.dart';

Future<void> main(List<String> args) async {
  var frames = 600, satellites = 24, json = false;
  for (var i = 0; i < args.length; i++) {
    switch (args[i]) {
      case '--frames':
        frames = int.parse(args[++i]);
      case '--satellites':
        satellites = int.parse(args[++i]);
      case '--json':
        json = true;
    }
  }

  final info = await dev.Service.controlWebServer(enable: true, silenceOutput: true);
  final uri = info.serverUri;
  if (uri == null) {
    stderr.writeln('VM service unavailable; run with `dart run` (JIT)');
    exit(2);
  }
  final ws = uri.replace(scheme: 'ws', path: '${uri.path}ws');
  final service = await vmServiceConnectUri(ws.toString());
  final isolateId = dev.Service.getIsolateId(iso.Isolate.current)!;

  final world = World.create()..throwOnDiagnostics = true;
  final scene = _Scene(satellites: satellites);
  // Warm up: spawn, resolve, reach steady state (JIT-compiled hot paths).
  for (var i = 0; i < 1200; i++) {
    world.frame(1 / 60, fixedUpdate: (tick) => scene.fixedUpdate(world, tick));
  }
  if (!scene.isReady) throw StateError('scene not ready after warm-up');

  // Bytes: heap usage before/after chunks of frames after a forced GC. A
  // chunk in which a scavenge ran is not counted for bytes (its garbage is
  // gone) but its GC is. The service round trips themselves allocate a
  // little, which a zero-frame chunk measures and removes. The per-class
  // counters are only maintained by unoptimized code and allocation stubs,
  // so once the JIT inlines an allocation they under-count; they name the
  // classes, the heap delta is the number.
  final gcEvents = <Event>[];
  await service.streamListen(EventStreams.kGC);
  service.onGCEvent.listen(gcEvents.add);

  Future<int> heap() async => (await service.getMemoryUsage(isolateId)).heapUsage ?? 0;
  Future<int> chunk(int n) async {
    final before = await heap();
    for (var i = 0; i < n; i++) {
      world.frame(1 / 60, fixedUpdate: (tick) => scene.fixedUpdate(world, tick));
    }
    return await heap() - before;
  }

  const chunkFrames = 50;
  await service.getAllocationProfile(isolateId, reset: true, gc: true);
  await Future<void>.delayed(const Duration(milliseconds: 20)); // forced-GC events land
  final overhead = await chunk(0);
  final firstEvent = gcEvents.length;
  var countedBytes = 0, countedFrames = 0, seen = gcEvents.length;
  final sw = Stopwatch()..start();
  for (var done = 0; done < frames; done += chunkFrames) {
    final n = math.min(chunkFrames, frames - done);
    final delta = await chunk(n);
    if (gcEvents.length == seen) {
      countedBytes += delta - overhead;
      countedFrames += n;
    }
    seen = gcEvents.length;
  }
  sw.stop();
  final profile = await service.getAllocationProfile(isolateId);
  await Future<void>.delayed(const Duration(milliseconds: 20));
  await service.dispose();

  final classes = <_ClassAlloc>[];
  var objects = 0;
  for (final m in profile.members ?? const <ClassHeapStats>[]) {
    final n = m.instancesAccumulated ?? 0;
    if (n == 0) continue;
    objects += n;
    classes.add(_ClassAlloc(m.classRef?.name ?? '?', n, m.accumulatedSize ?? 0));
  }
  classes.sort((a, b) => b.bytes.compareTo(a.bytes));

  // GC events carry cumulative collection counts and times per space; the
  // window's pauses are the deltas between successive events.
  var gcCount = 0;
  var gcMs = 0.0;
  final reasons = <String, int>{};
  double cumulative(Event e) {
    final j = e.json ?? const {};
    double t(String space) => ((j[space] as Map?)?['time'] as num?)?.toDouble() ?? 0;
    return t('new') + t('old');
  }

  for (var i = firstEvent; i < gcEvents.length; i++) {
    final e = gcEvents[i];
    gcCount++;
    gcMs += (cumulative(e) - (i > 0 ? cumulative(gcEvents[i - 1]) : cumulative(e))) * 1000;
    final r = '${e.json?['gcType']}/${e.json?['reason']}';
    reasons[r] = (reasons[r] ?? 0) + 1;
  }

  final entities = 1 + satellites;
  final heapBytesPerFrame = countedFrames == 0 ? double.nan : countedBytes / countedFrames;
  final perFrameObjects = objects / frames;
  final top = classes.take(8).map((c) => '${c.name}×${(c.count / frames).toStringAsFixed(1)}').join(' ');
  if (json) {
    print(jsonEncode({
      'frames': frames,
      'entities': entities,
      'heap_bytes_per_frame': heapBytesPerFrame,
      'frames_counted_for_bytes': countedFrames,
      'counted_objects_per_frame': perFrameObjects,
      'frame_us': sw.elapsedMicroseconds / frames, // includes the service round trips per chunk
      'gc_count': gcCount,
      'gc_total_ms': gcMs,
      'gc_reasons': reasons,
      'top_classes': [for (final c in classes.take(12)) {'class': c.name, 'per_frame': c.count / frames, 'bytes_per_frame': c.bytes / frames}],
    }));
  } else {
    print('[alloc] frames=$frames entities=$entities heap_bytes/frame=${heapBytesPerFrame.toStringAsFixed(0)} '
        '(over $countedFrames frames) counted_objects/frame=${perFrameObjects.toStringAsFixed(1)} '
        'gc=$gcCount gc_total_ms=${gcMs.toStringAsFixed(2)}${reasons.isEmpty ? '' : ' $reasons'}');
    print('[alloc] top: $top');
  }
  world.dispose();
}

class _ClassAlloc {
  _ClassAlloc(this.name, this.count, this.bytes);
  final String name;
  final int count, bytes;
}

/// hosts/headless `DemoScene` on the SDK (same LCG, same motion).
class _Scene {
  _Scene({int satellites = 24, int seed = 42}) {
    final rng = _Lcg(seed ^ 0x9E3779B97F4A7C15);
    for (var i = 0; i < satellites; i++) {
      _satellites.add(_Satellite(
        1.6 + rng.unit() * 2.2 + (i ~/ 12) * 0.35,
        rng.unit() * 2 * math.pi,
        0.3 + rng.unit() * 0.9,
        -2.0 + rng.unit() * 4.0,
        -0.6 + rng.unit() * 1.2,
        0.18 + rng.unit() * 0.22,
      ));
    }
  }

  final List<_Satellite> _satellites = [];
  GameObject? hub;
  bool _spawned = false;
  bool get isReady => hub?.isAlive == true && _satellites.every((s) => s.object?.isAlive == true);

  void fixedUpdate(World world, int tick) {
    final t = tick * world.fixedDt;
    final hub = this.hub ??= world.spawn(name: 'hub');
    if (hub.isPending) return;
    if (!_spawned) {
      _spawned = true;
      for (final s in _satellites) {
        s.object = world.spawn(transform: _local(s, t), parent: hub);
      }
    }
    // The read path a behaviour takes every tick (M3, the allocation gap):
    // a lookup by name is the entity table's byte comparison, a typed read
    // decodes one record from the object's retained query copy.
    if (world.find('hub') != hub) throw StateError('find');
    if (hub.get<Name>() == null) throw StateError('get');
    // Hot path (C1): reusable writers, no Transform/Vec3/Quat per write.
    _w.identity().setRotationAxisAngle(0, 1, 0, t * 0.8).rotateAxisAngle(1, 0, 0, t * 0.35).setUniformScale(0.8);
    hub.writeLocal(_w);
    for (final s in _satellites) {
      final o = s.object;
      if (o != null && !o.isPending) o.writeLocal(_localInto(s, t, _w));
    }
  }

  final TransformWriter _w = TransformWriter();

  static Transform _local(_Satellite s, double t) => _localInto(s, t, TransformWriter()).toTransform();

  static TransformWriter _localInto(_Satellite s, double t, TransformWriter w) {
    final a = s.phase + s.angularSpeed * t;
    return w
        .setPosition(math.cos(a) * s.radius, s.height, math.sin(a) * s.radius)
        .setRotationAxisAngle(0, 1, 0, s.spinSpeed * t)
        .setUniformScale(s.scale);
  }
}

class _Satellite {
  _Satellite(this.radius, this.phase, this.angularSpeed, this.spinSpeed, this.height, this.scale);
  final double radius, phase, angularSpeed, spinSpeed, height, scale;
  GameObject? object;
}

class _Lcg {
  _Lcg(this._state);
  int _state;
  int next() => _state = _state * 6364136223846793005 + 1442695040888963407;
  double unit() => ((next() >>> 40) & 0xFFFFFF) / (1 << 24);
}
