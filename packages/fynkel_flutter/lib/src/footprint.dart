// The process's physical footprint, read from inside the app: the number
// `footprint` reports on the Mac, and the one iOS kills an app over. On an
// iPhone no outside tool can read it, so the lifecycle check asks the
// kernel itself (`task_info` with `TASK_VM_INFO`).
//
// On Android it's the process's PSS, the number Android's own tools lead
// with, from /proc/self/smaps_rollup; there bin/measure.dart reads
// `dumpsys meminfo` from outside instead, and this is only for the stats
// line.

import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

@Native<Uint32>(symbol: 'mach_task_self_')
external int get _machTaskSelf;

@Native<Int32 Function(Uint32, Int32, Pointer<Uint8>, Pointer<Uint32>)>(symbol: 'task_info')
external int _taskInfo(int task, int flavor, Pointer<Uint8> info, Pointer<Uint32> count);

const _taskVmInfo = 22;

/// Where `phys_footprint` sits in `struct task_vm_info`: after two 32-bit
/// and seventeen 64-bit fields.
const _physFootprintAt = 144;

/// Where `ledger_tag_graphics_footprint` sits: the part of the footprint
/// Metal tagged as its own (buffers, textures, the driver's pools). Checked
/// on the Mac: a 100 MB Metal buffer shows here, a 100 MB malloc doesn't.
/// On the iPhone IOSurfaces aren't in it: ours change it by nothing when
/// the view shrinks, and the rest by their exact size.
const _graphicsAt = 272;

/// Room for the whole struct, in the 32-bit words `task_info` counts in.
const _words = 128;

/// The physical footprint in bytes.
int physicalFootprint() => footprint().total;

/// The physical footprint in bytes, and the GPU's part of it. On Android:
/// the PSS, and 0 for the GPU's part, which only `dumpsys meminfo` knows.
({int total, int graphics}) footprint() {
  if (Platform.isAndroid) return (total: _pss(), graphics: 0);
  final info = calloc<Uint8>(_words * 4);
  final count = calloc<Uint32>()..value = _words;
  try {
    final status = _taskInfo(_machTaskSelf, _taskVmInfo, info, count);
    if (status != 0) throw StateError('task_info failed: $status');
    if (count.value * 4 < _graphicsAt + 8) throw StateError('task_info gave no graphics footprint');
    return (total: (info + _physFootprintAt).cast<Uint64>().value, graphics: (info + _graphicsAt).cast<Int64>().value);
  } finally {
    calloc.free(info);
    calloc.free(count);
  }
}

/// The process's PSS in bytes.
int _pss() {
  final kb = RegExp(
    r'^Pss:\s+(\d+) kB',
    multiLine: true,
  ).firstMatch(File('/proc/self/smaps_rollup').readAsStringSync());
  if (kb == null) throw StateError('no Pss in /proc/self/smaps_rollup');
  return int.parse(kb.group(1)!) * 1024;
}
