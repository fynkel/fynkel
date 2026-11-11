// What a tool outside the app can read through its VM service, in debug
// and profile builds: `ext.fynkel.views`, each view's counts so far and the
// app's own memory and heat. So an app can be measured as it is, with no
// code of its own for it (fynkel_measure's `attach`): the tool asks once a
// second and takes the differences.

import 'dart:convert';
import 'dart:developer' as developer;

import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';
import 'package:fynkel/fynkel.dart' show ViewStats;

import 'footprint.dart';
import 'thermal.dart';

/// What a view reads for [viewsReport]: its counts so far, its size.
/// [id] changes with the engine's view, whose counts start again from 0.
typedef ViewReading = ({int id, int ticks, int width, int height, ViewStats? stats});

final _views = <Object, ViewReading Function()>{};

/// Reads the view [key] with [read] in each report, until [forget].
void inspect(Object key, ViewReading Function() read) {
  _register();
  _views[key] = read;
}

/// Stops reading the view [key].
void forget(Object key) => _views.remove(key);

var _registered = false;

void _register() {
  if (_registered || kReleaseMode) return;
  _registered = true;
  developer.registerExtension('ext.fynkel.views', (method, parameters) async {
    return developer.ServiceExtensionResponse.result(jsonEncode(viewsReport()));
  });
}

// Flutter's own frames, from the first report on: how many, and the build
// and raster thread's time for them.
var _flutterFrames = 0, _buildMicros = 0, _rasterMicros = 0, _longestRasterMicros = 0;
var _hearing = false;

void _hear(List<FrameTiming> timings) {
  for (final timing in timings) {
    _flutterFrames++;
    _buildMicros += timing.buildDuration.inMicroseconds;
    final raster = timing.rasterDuration.inMicroseconds;
    _rasterMicros += raster;
    if (raster > _longestRasterMicros) _longestRasterMicros = raster;
  }
}

/// What `ext.fynkel.views` answers: the time, the app's footprint (and the
/// GPU's part where it's known), the thermal state where the system says
/// it, Flutter's frames since the first report, and each live view's counts
/// since it was made. The longest raster is since the last report.
Map<String, Object?> viewsReport() {
  if (!_hearing) {
    _hearing = true;
    SchedulerBinding.instance.addTimingsCallback(_hear);
  }
  final memory = footprint();
  final report = {
    'micros': DateTime.now().microsecondsSinceEpoch,
    'footprint': memory.total,
    'graphics': memory.graphics,
    'thermal': thermalState(),
    'flutterFrames': _flutterFrames,
    'buildMicros': _buildMicros,
    'rasterMicros': _rasterMicros,
    'longestRasterMicros': _longestRasterMicros,
    'views': [for (final read in _views.values) _view(read())],
  };
  _longestRasterMicros = 0;
  return report;
}

Map<String, Object?> _view(ViewReading reading) {
  final s = reading.stats ?? const ViewStats();
  return {
    'id': reading.id,
    'ticks': reading.ticks,
    'width': reading.width,
    'height': reading.height,
    'posted': s.posted,
    'rendered': s.rendered,
    'coalesced': s.coalesced,
    'skipped': s.skipped,
    'presented': s.presented,
    'dropped': s.dropped,
    'busyMicros': s.busy.inMicroseconds,
    'textureBytes': s.textureBytes,
    'meshBytes': s.meshBytes,
    'skyBytes': s.skyBytes,
    'shadowBytes': s.shadowBytes,
    'targetBytes': s.targetBytes,
    'frameBytes': s.frameBytes,
    'behindBytes': s.behindBytes,
    'bloomBytes': s.bloomBytes,
    'effectBytes': s.effectBytes,
  };
}
