// A long run (stage 6, PR 13): round after round, every scene of the lab
// added to one world, run, its controls used, and destroyed, then the
// world disposed, as a game moves between levels for hours. The Dart heap
// (after a full collection), what the engine has malloc'd and the memory
// the process holds must stay flat, and nothing may throw.
//
// Off by default; CI's long run sets the rounds, and the VM service it
// collects with:
//
//   FYNKEL_SOAK_ROUNDS=120 flutter test --enable-vmservice test/soak_test.dart

import 'dart:developer';
import 'dart:ffi';
import 'dart:io';
import 'dart:math' as math;
import 'dart:isolate';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/lab.dart';
import 'package:lab/scenes.dart';
import 'package:vm_service/vm_service_io.dart';

Future<Uint8List> files(String asset) async => File('assets/$asset').readAsBytesSync();

final rounds = int.tryParse(Platform.environment['FYNKEL_SOAK_ROUNDS'] ?? '') ?? 0;

// FYNKEL_SOAK_DRAW=1 draws each round too (GT3). Not in CI yet: drawn,
// the process's lowest readings wander by 30 MB or more over a run on the
// runner (0.85 MB a round, against 0.25 allowed, 2026-10-27) though the
// graphics memory holds flat, so its check waits for one that reads the
// GPU's memory itself.
final drawing = Platform.environment['FYNKEL_SOAK_DRAW'] == '1';

void main() {
  test(
    'every scene, again and again, in one world each round: memory stays flat',
    () async {
      final info = await Service.controlWebServer(enable: true, silenceOutput: true);
      if (info.serverWebSocketUri == null) fail('run it with flutter test --enable-vmservice');
      final service = await vmServiceConnectUri(info.serverWebSocketUri.toString());
      addTearDown(service.dispose);
      final isolate = Service.getIsolateId(Isolate.current)!;
      final footprint = <int>[];
      final malloced = <int>[];
      final heap = <int>[];
      for (var round = 0; round < rounds; round++) {
        final world = World(source: files);
        // Drawn too, small, so what the GPU side keeps shows (GT3): every
        // 30th frame, read back, so none is replaced before it's drawn.
        final view = drawing ? HeadlessView(world, width: 64, height: 64) : null;
        for (final make in scenes) {
          final scene = world.add(make());
          await scene.loaded;
          void frames(int n) {
            for (var i = 0; i < n; i++) {
              world.update(1 / 60);
              if (view != null && i % 30 == 0) view.read(view.frame());
            }
          }

          frames(120);
          for (final control in scene.controls) {
            switch (control) {
              case Button(:final onPressed):
                onPressed();
              case Toggle(:final value, :final onChanged):
                onChanged(!value());
                frames(10);
                onChanged(!value());
              case Slider(:final onChanged, :final min, :final max):
                onChanged(min + (max - min) / 3);
            }
            frames(20);
          }
          frames(120);
          await pumpEventQueue();
          final errors = scene.log.value.where((l) => l.error && !l.text.contains('a bug in buggy'));
          expect(errors, isEmpty, reason: '${scene.title}, round $round');
          scene.destroy();
          world.update(1 / 60);
        }
        view?.dispose();
        world.dispose();
        await pumpEventQueue();
        // Drawn, what a round frees goes back over the next moments, not at
        // once: the lowest of a few readings, so each lands on what's kept.
        var low = _footprint();
        for (var i = 0; i < 6; i++) {
          await Future<void>.delayed(const Duration(milliseconds: 150));
          low = math.min(low, _footprint());
        }
        footprint.add(low);
        malloced.add(_malloced());
        // A full collection, so what's left is what's kept: after the third
        // round and the last only, as asking for a profile of every class
        // grows the process's memory by some 20 MB itself.
        if (round == 2 || round == rounds - 1) {
          final profile = await service.getAllocationProfile(isolate, gc: true);
          heap.add(profile.memoryUsage!.heapUsage!);
        }
        // ignore: avoid_print
        print(
          'round ${round + 1}: process ${mb(footprint.last)} MB, malloc ${mb(malloced.last)} MB${heap.isEmpty ? '' : ', Dart heap ${mb(heap.last)} MB'}',
        );
      }
      // The first rounds fill caches. After them the JIT still compiles a
      // little more each round (over 60 rounds: 2.5 MB from the 3rd to the
      // 30th, 0.9 MB from the 30th to the 60th), but a leak of even 100 KB
      // a scene would add 3.5 MB a round.
      final counted = rounds - 3;
      expect((heap.last - heap.first) / counted, lessThan(250e3), reason: 'Dart heap: ${heap.map(mb)}');
      // Memory goes up for a long while before it levels off: caches fill
      // (one scene alone, the skinned one, takes some 180 rounds to fill
      // its 20 MB). So the first third of the rounds is warm-up, and only
      // the rest is held to the limit. What the process holds is a saw: it
      // climbs, then drops as freed pages go back. What's kept shows at
      // the drops, so the lowest of the later rounds is held against the
      // lowest of the earlier. Malloc's count is steady (half a MB either
      // way), so it's held to far less: a leak in the engine shows there.
      // It sits at one of two levels a MB apart, round by round (9.7 or
      // 10.8 MB), in a mix that shifts: the middle value jumps a whole MB
      // when a few more rounds land high, though nothing grew (2026-10-23:
      // 40 % high in one half, 52 % in the other). So its halves are
      // compared by their mean, which a round's dip moves by under 1 KB a
      // round, and a leak of 20 KB a round still fails.
      final warm = rounds ~/ 3;
      final half = warm + (rounds - warm) ~/ 2;
      double growth(List<int> values, int Function(List<int>) pick) =>
          (pick(values.sublist(half)) - pick(values.sublist(warm, half))) / (rounds - half);
      int lowest(List<int> values) => values.reduce(math.min);
      int mean(List<int> values) => values.reduce((a, b) => a + b) ~/ values.length;

      expect(growth(footprint, lowest), lessThan(250e3), reason: 'process: ${footprint.map(mb)}');
      expect(growth(malloced, mean), lessThan(20e3), reason: 'malloc: ${malloced.map(mb)}');
    },
    skip: rounds < 10 ? 'set FYNKEL_SOAK_ROUNDS (10 or more) to run it' : false,
    timeout: Timeout.none,
  );
}

String mb(int bytes) => (bytes / 1e6).toStringAsFixed(1);

// The process's physical footprint, as Activity Monitor shows it: unlike
// its resident size, without the program's code paged in from disk, which
// grows as more of it runs. Elsewhere than macOS, the resident size.
int _footprint() {
  if (!Platform.isMacOS) return ProcessInfo.currentRss;
  // rusage_info_v0: a 16-byte uuid, then 64-bit counts, the 8th of them
  // the footprint.
  final info = calloc<Uint64>(12);
  try {
    if (_procPidRusage(pid, 0, info.cast()) != 0) return ProcessInfo.currentRss;
    return info[9];
  } finally {
    calloc.free(info);
  }
}

// What malloc has handed out and not had back, in bytes. Zero elsewhere
// than macOS.
int _malloced() => Platform.isMacOS ? _mstats().bytesUsed : 0;

@Native<Int Function(Int, Int, Pointer<Void>)>(symbol: 'proc_pid_rusage')
external int _procPidRusage(int pid, int flavor, Pointer<Void> buffer);

final class _MStats extends Struct {
  @Size()
  external int bytesTotal;
  @Size()
  external int chunksUsed;
  @Size()
  external int bytesUsed;
  @Size()
  external int chunksFree;
  @Size()
  external int bytesFree;
}

@Native<_MStats Function()>(symbol: 'mstats')
external _MStats _mstats();
