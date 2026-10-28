import 'dart:convert';
import 'dart:ffi';

import 'package:ffi/ffi.dart';

import 'bindings.g.dart';
import 'native.dart';
import 'world.dart' show Quality;

/// What the device is, as Unity's `SystemInfo` says: for a game that
/// chooses its quality by device rather than taking [Quality.device].
///
/// ```dart
/// // An old iPad's GPU: low, with fewer samples.
/// if (SystemInfo.graphicsDeviceName case 'Apple A9 GPU' || 'Apple A10 GPU') {
///   world.quality = Quality.low.copyWith(antiAliasing: 2);
/// }
/// ```
///
/// Each starts the GPU if no view has yet.
abstract final class SystemInfo {
  static (String, int, String)? _gpu;

  static (String, int, String) get _described => _gpu ??= using<(String, int, String)>((arena) {
    final length = arena<Size>();
    check(fy_gpu_describe(nullptr, 0, length));
    final bytes = arena<Uint8>(length.value == 0 ? 1 : length.value);
    check(fy_gpu_describe(bytes, length.value, length));
    final [name, vendor, api] = utf8.decode(bytes.asTypedList(length.value)).split('\n');
    return (name, int.parse(vendor), api);
  });

  /// The GPU's name, as its driver gives it: "Apple A10 GPU", "Mali-G76",
  /// "Adreno (TM) 640", "Apple M2 Pro".
  static String get graphicsDeviceName => _described.$1;

  /// Its maker's PCI id (Apple's is 0x106B, Arm's 0x13B5, Qualcomm's
  /// 0x5143), or 0 where the driver doesn't say.
  static int get graphicsDeviceVendorId => _described.$2;

  /// What's drawing with it: `metal`, `vulkan`, `gl` or `dx12`.
  static String get graphicsApi => _described.$3;
}
