// The process's physical footprint, read from inside the app: the number
// `footprint` reports on the Mac, and the one iOS kills an app over. On an
// iPhone no outside tool can read it, so the lifecycle check asks the
// kernel itself (`task_info` with `TASK_VM_INFO`).

import 'dart:ffi';

import 'package:ffi/ffi.dart';

@Native<Uint32>(symbol: 'mach_task_self_')
external int get _machTaskSelf;

@Native<Int32 Function(Uint32, Int32, Pointer<Uint8>, Pointer<Uint32>)>(symbol: 'task_info')
external int _taskInfo(int task, int flavor, Pointer<Uint8> info, Pointer<Uint32> count);

const _taskVmInfo = 22;

/// Where `phys_footprint` sits in `struct task_vm_info`: after two 32-bit
/// and seventeen 64-bit fields.
const _physFootprintAt = 144;

/// Room for the whole struct, in the 32-bit words `task_info` counts in.
const _words = 128;

/// The physical footprint in bytes.
int physicalFootprint() {
  final info = calloc<Uint8>(_words * 4);
  final count = calloc<Uint32>()..value = _words;
  try {
    final status = _taskInfo(_machTaskSelf, _taskVmInfo, info, count);
    if (status != 0) throw StateError('task_info failed: $status');
    if (count.value * 4 < _physFootprintAt + 8) throw StateError('task_info gave no phys_footprint');
    return (info + _physFootprintAt).cast<Uint64>().value;
  } finally {
    calloc.free(info);
    calloc.free(count);
  }
}
