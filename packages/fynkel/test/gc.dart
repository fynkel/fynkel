// Forcing a garbage collection, with the VM service, for tests of what a
// world lets go once nothing reaches it.

import 'dart:developer';
import 'dart:isolate' as isolates;

import 'package:test/test.dart';
import 'package:vm_service/vm_service.dart';
import 'package:vm_service/vm_service_io.dart';

/// Connects to the VM service for the file's tests, and gives back a
/// function that collects all garbage, then lets the finalizers run.
Future<void> Function() garbageCollector() {
  late VmService service;
  late String isolate;

  setUpAll(() async {
    final info = await Service.controlWebServer(enable: true, silenceOutput: true);
    service = await vmServiceConnectUri(info.serverWebSocketUri.toString());
    isolate = Service.getIsolateId(isolates.Isolate.current)!;
  });

  tearDownAll(() => service.dispose());

  return () async {
    // Twice: what a finalizer drops the first time goes the second.
    for (var i = 0; i < 2; i++) {
      await service.getAllocationProfile(isolate, gc: true);
      await pumpEventQueue();
    }
  };
}
