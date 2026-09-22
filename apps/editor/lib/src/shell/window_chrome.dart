// The window's own title bar (item 9b): on macOS the editor draws it — the
// native one is transparent, the traffic lights sit on the editor's bar —
// so the bar has to do what a title bar does: a drag on empty space moves
// the window, a double-click zooms (or minimises, per System Settings), and
// the bar leaves room for the traffic lights except in full screen.
//
// The native half is `macos/Runner/MainFlutterWindow.swift`. Anywhere it is
// missing (a widget test, another platform) every call is a no-op and the
// bar leaves no room.
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

final class WindowChrome {
  WindowChrome._() {
    _channel.setMethodCallHandler((call) async {
      if (call.method == 'fullScreen') fullScreen.value = call.arguments == true;
      return null;
    });
    _ask();
  }

  static final instance = WindowChrome._();

  static const _channel = MethodChannel('fynkel.editor/window');

  /// Whether the native side answered: the title bar is the editor's.
  final ownsTitleBar = ValueNotifier<bool>(false);

  final fullScreen = ValueNotifier<bool>(false);

  /// Room the traffic lights need at the left of the bar.
  double get leadingInset => ownsTitleBar.value && !fullScreen.value ? 76 : 0;

  Future<void> _ask() async {
    if (!Platform.isMacOS) return;
    try {
      final r = await _channel.invokeMethod<bool>('isFullScreen');
      fullScreen.value = r ?? false;
      ownsTitleBar.value = true;
    } on MissingPluginException {
      // No native half (a widget test): the bar behaves like any toolbar.
    } on PlatformException {
      // Same.
    }
  }

  /// Asks the native half again (a test that mocks the channel).
  @visibleForTesting
  Future<void> refresh() => _ask();

  /// Back to "no native half" (after such a test).
  @visibleForTesting
  void reset() {
    ownsTitleBar.value = false;
    fullScreen.value = false;
  }

  /// A press on the bar's empty space started a drag: the window follows.
  Future<void> startDrag() => _call('startDrag');

  /// A double-click on the bar's empty space: zoom, or what the user chose.
  Future<void> doubleClick() => _call('titleBarDoubleClick');

  Future<void> _call(String method) async {
    if (!ownsTitleBar.value) return;
    try {
      await _channel.invokeMethod<void>(method);
    } on MissingPluginException {
      // See [_ask].
    } on PlatformException {
      // See [_ask].
    }
  }
}
