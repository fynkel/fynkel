import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show FynkelLaunch;

/// A game compiled into this editor build (ADR-0021): its package, its
/// project and its entry point (`lib/fynkel_game.dart`'s `buildGame`). The
/// project editor `fynkel editor <dir>` generates passes one to
/// `runFynkelEditor`; the plain editor has none.
@immutable
class EditorGame {
  const EditorGame({required this.name, required this.projectDir, required this.build});

  /// The game package's name.
  final String name;

  /// The project it belongs to: Play runs it only there.
  final String projectDir;
  final Widget Function(FynkelLaunch launch) build;
}
