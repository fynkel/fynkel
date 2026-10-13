// A long run (stage 6, PR 13): round after round, every scene of the lab
// added to one world, run, its controls used, and destroyed, then the
// world disposed, as a game moves between levels for hours. The Dart heap
// (after a full collection) and the memory the process holds must stay
// flat, and nothing may throw.
//
// Off by default; CI's long run sets the rounds, and the VM service it
// collects with:
//
//   FYNKEL_SOAK_ROUNDS=40 flutter test --enable-vmservice test/soak_test.dart

import 'dart:developer';
import 'dart:io';
import 'dart:math' as math;
import 'dart:isolate';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/lab.dart';
import 'package:lab/scenes.dart';
import 'package:vm_service/vm_service_io.dart';

Future<Uint8List> files(String asset) async => File('assets/$asset').readAsBytesSync();

final rounds = int.tryParse(Platform.environment['FYNKEL_SOAK_ROUNDS'] ?? '') ?? 0;

void main() {
  test(
    'every scene, again and again, in one world each round: memory stays flat',
    () async {
      final info = await Service.controlWebServer(enable: true, silenceOutput: true);
      if (info.serverWebSocketUri == null) fail('run it with flutter test --enable-vmservice');
      final service = await vmServiceConnectUri(info.serverWebSocketUri.toString());
      addTearDown(service.dispose);
      final isolate = Service.getIsolateId(Isolate.current)!;
      final rss = <int>[];
      final heap = <int>[];
      for (var round = 0; round < rounds; round++) {
        final world = World(source: files);
        for (final make in scenes) {
          final scene = world.add(make());
          await scene.loaded;
          void frames(int n) {
            for (var i = 0; i < n; i++) {
              world.update(1 / 60);
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
        world.dispose();
        await pumpEventQueue();
        rss.add(ProcessInfo.currentRss);
        // A full collection, so what's left is what's kept: after the third
        // round and the last only, as asking for a profile of every class
        // grows the process's memory by some 20 MB itself.
        if (round == 2 || round == rounds - 1) {
          final profile = await service.getAllocationProfile(isolate, gc: true);
          heap.add(profile.memoryUsage!.heapUsage!);
        }
        // ignore: avoid_print
        print('round ${round + 1}: process ${mb(rss.last)} MB${heap.isEmpty ? '' : ', Dart heap ${mb(heap.last)} MB'}');
      }
      // The first rounds fill caches. After them the JIT still compiles a
      // little more each round (over 60 rounds: 2.5 MB from the 3rd to the
      // 30th, 0.9 MB from the 30th to the 60th), but a leak of even 100 KB
      // a scene would add 3.5 MB a round.
      final counted = rounds - 3;
      expect((heap.last - heap.first) / counted, lessThan(250e3), reason: 'Dart heap: ${heap.map(mb)}');
      // The process's memory is a saw: it climbs, then drops as freed pages
      // go back. What's kept shows at the drops, so the lowest of the later
      // rounds is held against the lowest of the earlier.
      final half = 3 + counted ~/ 2;
      final early = rss.sublist(3, half).reduce(math.min);
      final late = rss.sublist(half).reduce(math.min);
      expect((late - early) / (rounds - half), lessThan(250e3), reason: 'process: ${rss.map(mb)}');
    },
    skip: rounds < 10 ? 'set FYNKEL_SOAK_ROUNDS (10 or more) to run it' : false,
    timeout: Timeout.none,
  );
}

String mb(int bytes) => (bytes / 1e6).toStringAsFixed(1);
