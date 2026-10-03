// A game's uncaught errors on the editor channel (roadmap Phase 2, IDE
// navigation): while an editor listens (`EditorChannel.enabled` — Build
// and Run's process, or Play in the editor), an error Flutter reports (a
// behaviour that threw inside the game view's frame) or an async error the
// platform dispatcher catches is also sent as an `error` log record with
// its Dart stack, so the Console's Game tab shows it and opens the game's
// own frames in the IDE. The handlers that were there still run: nothing
// is swallowed, and with no editor listening this does nothing.

import 'dart:ui';

import 'package:flutter/widgets.dart';
import 'package:fynkel/gameplay.dart';

abstract final class ChannelErrorReporting {
  static bool _installed = false;

  /// Whether [install] ran in this process.
  static bool get installed => _installed;

  /// Chains the reporters in front of the current handlers, once per
  /// process (every `FynkelGameView` calls it).
  static void install() {
    if (_installed) return;
    _installed = true;
    final flutter = FlutterError.onError;
    FlutterError.onError = (details) {
      if (EditorChannel.enabled) {
        EditorChannel.error(details.exceptionAsString(), stack: details.stack?.toString());
      }
      (flutter ?? FlutterError.presentError)(details);
    };
    final dispatcher = PlatformDispatcher.instance;
    final platform = dispatcher.onError;
    dispatcher.onError = (error, stack) {
      if (EditorChannel.enabled) EditorChannel.error('$error', stack: stack.toString());
      return platform?.call(error, stack) ?? false;
    };
  }
}
