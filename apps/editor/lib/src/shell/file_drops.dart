// Files dragged in from Finder (Unity imports what is dropped on its
// Project window). The macOS window takes the drag
// (macos/Runner/MainFlutterWindow.swift) and reports it on
// `fynkel.editor/drop` in the window's logical coordinates: `over {x, y}`
// while it moves, `exit`, `drop {paths, x, y}`. The panel under the
// point takes it; the Project browser imports the files.
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

/// Dropped [paths] (absolute) at [position] (global logical pixels).
final class FileDrop {
  const FileDrop(this.paths, this.position);

  final List<String> paths;
  final Offset position;
}

final class FileDrops {
  FileDrops._() {
    channel.setMethodCallHandler(_on);
  }

  static final instance = FileDrops._();

  static const channel = MethodChannel('fynkel.editor/drop');

  /// Where a drag from Finder is over the window; null when none is.
  final hover = ValueNotifier<Offset?>(null);

  final _takers = <bool Function(FileDrop drop)>[];

  /// [taker] answers true when the drop is its own (it took the files).
  void addTaker(bool Function(FileDrop drop) taker) => _takers.add(taker);
  void removeTaker(bool Function(FileDrop drop) taker) => _takers.remove(taker);

  Future<Object?> _on(MethodCall call) async {
    final a = call.arguments is Map ? call.arguments as Map : const {};
    final at = Offset((a['x'] as num? ?? 0).toDouble(), (a['y'] as num? ?? 0).toDouble());
    switch (call.method) {
      case 'over':
        hover.value = at;
        return null;
      case 'exit':
        hover.value = null;
        return null;
      case 'drop':
        hover.value = null;
        final drop = FileDrop([for (final p in a['paths'] as List? ?? const []) '$p'], at);
        for (final take in _takers.reversed.toList()) {
          if (take(drop)) return true;
        }
        return false;
    }
    return null;
  }
}
