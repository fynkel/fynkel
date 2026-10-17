// The system's thermal state, read from inside the app: what iOS and
// macOS say of the device's heat (`NSProcessInfo.thermalState`), through
// the Objective-C runtime. iOS slows the device down from serious on
// (docs/iphone-heat.md).

import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

final _process = DynamicLibrary.process();

final _getClass = _process.lookupFunction<Pointer<Void> Function(Pointer<Utf8>), Pointer<Void> Function(Pointer<Utf8>)>(
  'objc_getClass',
);
final _selector = _process.lookupFunction<Pointer<Void> Function(Pointer<Utf8>), Pointer<Void> Function(Pointer<Utf8>)>(
  'sel_registerName',
);

// objc_msgSend, called as each method's own signature: an object, or an
// NSInteger, from an object and a selector.
final _sendForObject = _process
    .lookupFunction<
      Pointer<Void> Function(Pointer<Void>, Pointer<Void>),
      Pointer<Void> Function(Pointer<Void>, Pointer<Void>)
    >('objc_msgSend');
final _sendForInteger = _process
    .lookupFunction<Int64 Function(Pointer<Void>, Pointer<Void>), int Function(Pointer<Void>, Pointer<Void>)>(
      'objc_msgSend',
    );

Pointer<Void> _sel(String name) {
  final text = name.toNativeUtf8();
  try {
    return _selector(text);
  } finally {
    calloc.free(text);
  }
}

final Pointer<Void> _processInfo = () {
  final name = 'NSProcessInfo'.toNativeUtf8();
  try {
    return _sendForObject(_getClass(name), _sel('processInfo'));
  } finally {
    calloc.free(name);
  }
}();
final Pointer<Void> _thermalState = _sel('thermalState');

/// The thermal state's names, by its number.
const thermalStates = ['nominal', 'fair', 'serious', 'critical'];

/// The thermal state now (0 nominal, 1 fair, 2 serious, 3 critical), or
/// null where the system doesn't say (Android).
int? thermalState() {
  if (!Platform.isIOS && !Platform.isMacOS) return null;
  return _sendForInteger(_processInfo, _thermalState);
}
