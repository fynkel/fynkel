import 'dart:ffi';
import 'dart:io';
import 'dart:math' as math;

import 'package:ffi/ffi.dart';
import 'package:flutter/services.dart';

/// dart:ffi bindings to spikes/bevy_flutter/native (the `bf_*` C ABI).
class BevyLib {
  BevyLib._(this.lib)
      : create = lib.lookupFunction<Pointer<Void> Function(Uint32, Uint32, Pointer<Utf8>, Uint32),
            Pointer<Void> Function(int, int, Pointer<Utf8>, int)>('bf_create'),
        lastError = lib.lookupFunction<Uint32 Function(Pointer<Utf8>, Uint32), int Function(Pointer<Utf8>, int)>(
            'bf_last_error'),
        surfaceCount = lib.lookupFunction<Uint32 Function(Pointer<Void>), int Function(Pointer<Void>)>(
            'bf_surface_count'),
        surface = lib.lookupFunction<Pointer<Void> Function(Pointer<Void>, Uint32),
            Pointer<Void> Function(Pointer<Void>, int)>('bf_surface'),
        frame = lib.lookupFunction<Void Function(Pointer<Void>, Pointer<Float>, Uint32),
            void Function(Pointer<Void>, Pointer<Float>, int)>('bf_frame'),
        resize = lib.lookupFunction<Int32 Function(Pointer<Void>, Uint32, Uint32), int Function(Pointer<Void>, int, int)>(
            'bf_resize'),
        stats = lib.lookupFunction<Uint32 Function(Pointer<Void>, Pointer<Double>, Uint32),
            int Function(Pointer<Void>, Pointer<Double>, int)>('bf_stats'),
        destroy = lib.lookupFunction<Void Function(Pointer<Void>), void Function(Pointer<Void>)>('bf_destroy'),
        setQueue = lib.lookupFunction<Void Function(Pointer<Void>, Int32), void Function(Pointer<Void>, int)>(
            'bf_set_queue');

  /// macOS: the cdylib from `cargo build --release` (path overridable with
  /// `--dart-define=BEVY_LIB=...`). iOS: the staticlib is linked into Runner.
  static final BevyLib instance = BevyLib._(Platform.isIOS
      ? DynamicLibrary.process()
      : DynamicLibrary.open(const String.fromEnvironment('BEVY_LIB',
          defaultValue: '/Users/oumari/Desktop/Code/fynkelengine/benchmarks/compare/bevy/target/release/'
              'libbevy_flutter_spike.dylib')));

  final DynamicLibrary lib;
  final Pointer<Void> Function(int, int, Pointer<Utf8>, int) create;
  final int Function(Pointer<Utf8>, int) lastError;
  final int Function(Pointer<Void>) surfaceCount;
  final Pointer<Void> Function(Pointer<Void>, int) surface;
  final void Function(Pointer<Void>, Pointer<Float>, int) frame;
  final int Function(Pointer<Void>, int, int) resize;
  final int Function(Pointer<Void>, Pointer<Double>, int) stats;
  final void Function(Pointer<Void>) destroy;
  final void Function(Pointer<Void>, int) setQueue;

  String error() {
    final buf = calloc<Uint8>(1024).cast<Utf8>();
    lastError(buf, 1024);
    final s = buf.toDartString();
    calloc.free(buf);
    return s;
  }
}

/// Where Flutter put the bundled assets (Bevy's AssetPlugin reads files).
String flutterAssetsDir() {
  final exe = File(Platform.resolvedExecutable).parent.path;
  return Platform.isIOS
      ? '$exe/Frameworks/App.framework/flutter_assets/assets'
      : '$exe/../Frameworks/App.framework/Resources/flutter_assets/assets';
}

/// One Bevy app rendering into an IOSurface ring, shown as a Flutter texture.
class BevyViewport {
  BevyViewport._(this.ctx, this.textureId, this.width, this.height, this.cubes)
      : _transforms = calloc<Float>(cubes * 4),
        _stats = calloc<Double>(8);

  static const _channel = MethodChannel('bevy_view');

  final Pointer<Void> ctx;
  final int textureId;
  final int cubes;
  int width, height;
  final Pointer<Float> _transforms;
  final Pointer<Double> _stats;

  static Future<BevyViewport> create(int width, int height, int cubes) async {
    final b = BevyLib.instance;
    final assets = flutterAssetsDir().toNativeUtf8();
    final ctx = b.create(width, height, assets, cubes);
    calloc.free(assets);
    if (ctx == nullptr) throw StateError('bf_create: ${b.error()}');
    // `--dart-define=SPIKE_QUEUE=false`: show only the newest frame (the
    // first iPhone runs).
    b.setQueue(ctx, const bool.fromEnvironment('SPIKE_QUEUE', defaultValue: true) ? 1 : 0);
    final id = await _channel.invokeMethod<int>('registerTexture', {
      'ctx': ctx.address,
      'acquireFn': b.lib.lookup<Void>('bf_acquire').address,
      'setCallbackFn': b.lib.lookup<Void>('bf_set_frame_ready_callback').address,
      'iosurfaces': _surfaces(ctx),
    });
    return BevyViewport._(ctx, id!, width, height, cubes);
  }

  static List<int> _surfaces(Pointer<Void> ctx) {
    final b = BevyLib.instance;
    return [for (var i = 0; i < b.surfaceCount(ctx); i++) b.surface(ctx, i).address];
  }

  /// The game side of a frame: the same wave as native/examples/smoke.rs.
  void frame(double t) {
    final out = _transforms;
    for (var i = 0; i < cubes; i++) {
      final col = (i % 40).toDouble(), row = (i ~/ 40).toDouble();
      out[i * 4] = (col - 19.5) * 0.9;
      out[i * 4 + 1] = 0.6 + 0.5 * math.sin(t * 2.0 + i * 0.15);
      out[i * 4 + 2] = (row - 12.0) * 0.9 - 6.0;
      out[i * 4 + 3] = t + i * 0.01;
    }
    BevyLib.instance.frame(ctx, out, cubes);
  }

  /// Rebuilds the ring; returns the milliseconds the native resize took.
  Future<double> resize(int w, int h) async {
    if (w == width && h == height) return 0;
    final sw = Stopwatch()..start();
    if (BevyLib.instance.resize(ctx, w, h) != 0) throw StateError('bf_resize: ${BevyLib.instance.error()}');
    final ms = sw.elapsedMicroseconds / 1000;
    width = w;
    height = h;
    await _channel.invokeMethod('rebindTexture', {'textureId': textureId, 'iosurfaces': _surfaces(ctx)});
    return ms;
  }

  /// [frames, dropped, coalesced, presented, update p50, p95, gpu p50, p95].
  List<double> nativeStats() {
    final n = BevyLib.instance.stats(ctx, _stats, 8);
    return [for (var i = 0; i < n; i++) _stats[i]];
  }

  Future<Map<String, int>> textureStats() async =>
      (await _channel.invokeMapMethod<String, int>('stats', {'textureId': textureId})) ?? {};

  Future<void> dispose() async {
    await _channel.invokeMethod('unregisterTexture', {'textureId': textureId});
    BevyLib.instance.destroy(ctx);
    calloc.free(_transforms);
    calloc.free(_stats);
  }
}
