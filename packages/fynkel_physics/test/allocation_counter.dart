// The other side of allocations.dart: an isolate of its own group, so the
// VM service's answers, tens of thousands of objects each, are decoded in
// a heap the count doesn't see. The VM tallies what an isolate allocates
// a batch at a time, not object by object, so a decode in the counted
// isolate lands inside or outside the window by chance: that was the
// alloc tests' flake (stage 6, R20).
//
// It's started with the VM service's address, the counted isolate's id,
// and a port; it answers with its own port, then each request with one
// message: ['mark'] → null, a count kept; ['zero'] → null, what came since
// kept as a window's cost; ['count', minimum] → {class: [id, instances]}
// made since, beyond that cost, for those with at least minimum; ['now']
// → the VM's clock; ['trace', classId, on] → null; ['traces', classId,
// since] → lines.

import 'dart:isolate';

import 'package:vm_service/vm_service.dart';
import 'package:vm_service/vm_service_io.dart';

Future<void> main(List<String> args, SendPort reply) async {
  final service = await vmServiceConnectUri(args[0]);
  final isolate = args[1];
  final requests = ReceivePort();
  reply.send(requests.sendPort);
  await for (final request in requests) {
    final [what as String, ...rest] = request as List<Object?>;
    try {
      await _answer(service, isolate, what, rest, reply);
    } catch (error) {
      // Each request gets its answer, or the test waits for good.
      reply.send(what == 'traces' ? ['no traces: $error'] : null);
    }
    if (what == 'close') requests.close();
  }
}

Future<void> _answer(VmService service, String isolate, String what, List<Object?> rest, SendPort reply) async {
  switch (what) {
    case 'mark':
      _last = await _count(service, isolate);
      reply.send(null);
    case 'zero':
      final now = await _count(service, isolate);
      _zero = _beyond(now, _last);
      _last = now;
      reply.send(null);
    case 'count':
      final minimum = rest[0]! as num;
      final now = await _count(service, isolate);
      final made = _beyond(now, _last);
      _last = now;
      reply.send({
        for (final MapEntry(key: (name, id), :value) in made.entries)
          if (value - (_zero[(name, id)] ?? 0) >= minimum) name: [id, value - (_zero[(name, id)] ?? 0)],
      });
    case 'now':
      reply.send((await service.getVMTimelineMicros()).timestamp!);
    case 'trace':
      // Traces are the profiler's samples; it's off under `dart test`.
      await service.setFlag('profiler', 'true');
      await service.setTraceClassAllocation(isolate, rest[0]! as String, rest[1]! as bool);
      reply.send(null);
    case 'traces':
      final samples = await service.getAllocationTraces(
        isolate,
        classId: rest[0]! as String,
        timeOriginMicros: rest[1]! as int,
      );
      reply.send(_mostSeen(samples));
    case 'close':
      await service.dispose();
      reply.send(null);
  }
}

// The last count, and what a window without frames made: each count of
// the VM's internal classes, which a reset doesn't clear, gains hundreds
// from every request. So nothing is reset; a window is the difference of
// two counts, and what a window of no frames made is taken off. Only the
// classes over a minimum are sent: a big answer, decoded in the counted
// isolate, would be tallied in the next window.
var _last = <(String, String), int>{};
var _zero = <(String, String), int>{};

Future<Map<(String, String), int>> _count(VmService service, String isolate) async {
  final profile = await service.getAllocationProfile(isolate);
  return {for (final m in profile.members!) ('${m.classRef!.name}', m.classRef!.id!): m.instancesAccumulated ?? 0};
}

Map<(String, String), int> _beyond(Map<(String, String), int> now, Map<(String, String), int> before) => {
  for (final MapEntry(:key, :value) in now.entries) key: value - (before[key] ?? 0),
};

/// The five stacks seen most in [samples], eight calls deep, with counts.
List<String> _mostSeen(CpuSamples samples) {
  final functions = samples.functions ?? const <ProfileFunction>[];
  final stacks = <String, int>{};
  for (final sample in samples.samples ?? const <CpuSample>[]) {
    final stack = [
      for (final index in (sample.stack ?? const <int>[]).take(8))
        if (index < functions.length) _name(functions[index].function),
    ].join(' < ');
    stacks[stack] = (stacks[stack] ?? 0) + 1;
  }
  final top = stacks.entries.toList()..sort((a, b) => b.value.compareTo(a.value));
  return [
    '${samples.samples?.length ?? 0} traced; most seen:',
    for (final MapEntry(:key, :value) in top.take(5)) '  $value × $key',
  ];
}

String _name(Object? function) => switch (function) {
  FuncRef(:final name, :final owner) => '${owner is ClassRef ? '${owner.name}.' : ''}$name',
  NativeFunction(:final name) => 'native $name',
  _ => '$function',
};
