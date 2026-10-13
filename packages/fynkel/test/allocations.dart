// Counting what frames allocate, with the VM service's allocation profile.
// Each test file runs one scenario only: in the JIT one scenario's code
// can box doubles in the next one's.
//
// The profile is asked for by allocation_counter.dart, in an isolate of
// its own group, so decoding the answers isn't counted (see there); here
// the window between a reset and a count holds the frames and two small
// port messages. package:fynkel_physics/test has a copy of both files.

import 'dart:async';
import 'dart:developer';
import 'dart:io';
import 'dart:isolate';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

// A constant: `1 / 60` written in the loop is worked out, and boxed, at
// each call there.
const _dt = 1 / 60;

/// Starts the counter for the file's tests, and gives back
/// [madeEachFrame].
Future<Map<String, int>> Function(World world, int frames, {int tries}) allocations() {
  late SendPort counter;
  late StreamIterator<Object?> answers;
  late ReceivePort port;

  Future<T> ask<T>(List<Object?> request) async {
    counter.send(request);
    if (!await answers.moveNext()) throw StateError('the allocation counter stopped');
    return answers.current as T;
  }

  setUpAll(() async {
    final info = await Service.controlWebServer(enable: true, silenceOutput: true);
    port = ReceivePort();
    answers = StreamIterator(port);
    await Isolate.spawnUri(
      Uri.file('${Directory.current.path}/test/allocation_counter.dart'),
      [info.serverWebSocketUri.toString(), Service.getIsolateId(Isolate.current)!],
      port.sendPort,
      packageConfig: await Isolate.packageConfig,
    );
    await answers.moveNext();
    counter = answers.current! as SendPort;
  });

  tearDownAll(() async {
    await ask<void>(['close']);
    port.close();
  });

  // Each class's id in the VM, by name, from the last count.
  final classIds = <String, String>{};

  /// Counts [frames] frames' allocations, beyond those of no frames: the
  /// classes made at least [minimum] times.
  Future<Map<String, int>> made(World world, int frames, num minimum) async {
    await ask<void>(['mark']);
    await ask<void>(['zero']);
    for (var i = 0; i < frames; i++) {
      world.update(_dt);
    }
    final counts = await ask<Map<Object?, Object?>>(['count', minimum]);
    final made = <String, int>{};
    for (final MapEntry(:key, :value) in counts.entries) {
      final [id as String, instances as int] = value! as List<Object?>;
      classIds[key! as String] = id;
      made[key as String] = instances;
    }
    return made;
  }

  /// Prints where [frames] frames make [classes], from the VM's allocation
  /// traces, so a failure names its source.
  Future<void> trace(World world, int frames, Iterable<String> classes) async {
    for (final name in classes) {
      final id = classIds[name];
      if (id == null) continue;
      final since = await ask<int>(['now']);
      await ask<void>(['trace', id, true]);
      for (var i = 0; i < frames; i++) {
        world.update(_dt);
      }
      final lines = await ask<List<Object?>>(['traces', id, since]);
      await ask<void>(['trace', id, false]);
      // ignore: avoid_print
      print('$name made over $frames frames: ${lines.join('\n')}');
    }
  }

  /// The classes [frames] frames made at least one of every two frames,
  /// with how many, beyond what asking for the count makes. The VM tallies
  /// allocations a batch at a time, so a window can gain or lose a few of
  /// what was made around it: a class made once a frame shows up as a
  /// little less, hence half.
  ///
  /// When it finds some, it warms and counts again, up to [tries] times:
  /// under `dart test`, files share the background compiler, so warmed code
  /// can still be waiting to be optimized, and boxes or makes lists until
  /// it is. What's still made the last time is what a frame makes, and
  /// where it's made is printed.
  return (World world, int frames, {int tries = 3}) async {
    var found = <String, int>{};
    for (var attempt = 0; attempt < tries; attempt++) {
      // Time for the compiler, busy with the other files' code, to finish.
      if (attempt > 0) await Future<void>.delayed(const Duration(milliseconds: 500));
      // Warm, and compiled: the VM optimizes a function after tens of
      // thousands of calls, and until then boxes its doubles.
      for (var i = 0; i < 100000; i++) {
        world.update(_dt);
      }
      found = await made(world, frames, frames / 2);
      if (found.isEmpty) break;
    }
    if (found.isNotEmpty) await trace(world, frames, found.keys);
    return found;
  };
}
