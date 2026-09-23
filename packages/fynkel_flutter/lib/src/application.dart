// What a game asks of the app it runs in, and what that app tells it
// (ADR-0021): [FynkelLaunch] — where the project is and which scene to
// start in; [FynkelApplication] — Unity's `Application`: quit, and whether
// this is the editor; [FynkelPlayControl] — the editor's Pause and Step,
// which every `FynkelGameView` below a [FynkelPlayScope] obeys.
//
// One game entry point serves both hosts: `lib/fynkel_game.dart` exports
// `Widget buildGame(FynkelLaunch launch)`; the game's `main.dart` runs it
// with [FynkelLaunch.fromEnvironment], the editor's Play with the open
// project and scene.

import 'dart:io';

import 'package:flutter/widgets.dart';

/// Where the game starts: given by its `main` in a build, by the editor's
/// Play inside the editor.
@immutable
class FynkelLaunch {
  const FynkelLaunch({this.projectDir, this.cacheDir, this.startScene, this.inEditor = false});

  /// The directory holding `fynkel.project.json`, when this process can
  /// see it (null on a device: the game unpacks the cache it bundles).
  final String? projectDir;

  /// The cooked cache to load from; null: `<projectDir>/.fynkel/cache`.
  final String? cacheDir;

  /// A scene (asset id) to play instead of the project's startup scene —
  /// the scene open in the editor when Play was pressed.
  final String? startScene;

  /// The editor's Play (see [FynkelApplication.inEditor]).
  final bool inEditor;

  /// A build's launch: the project from `FYNKEL_PROJECT` (`--dart-define`
  /// or the environment; `fynkel run` sets it), else the working directory
  /// when it holds `fynkel.project.json`; the start scene from
  /// `FYNKEL_SCENE` the same way.
  factory FynkelLaunch.fromEnvironment({Map<String, String>? environment, String? workingDirectory}) {
    final env = environment ?? Platform.environment;
    String? read(String defined, String name) {
      if (defined.isNotEmpty) return defined;
      final v = env[name];
      return v == null || v.isEmpty ? null : v;
    }

    var project = read(const String.fromEnvironment('FYNKEL_PROJECT'), 'FYNKEL_PROJECT');
    if (project == null) {
      final cwd = workingDirectory ?? Directory.current.path;
      if (File('$cwd/fynkel.project.json').existsSync()) project = cwd;
    }
    return FynkelLaunch(
      projectDir: project,
      startScene: read(const String.fromEnvironment('FYNKEL_SCENE'), 'FYNKEL_SCENE'),
    );
  }

  @override
  String toString() =>
      'FynkelLaunch(projectDir: $projectDir, cacheDir: $cacheDir, startScene: $startScene, inEditor: $inEditor)';
}

/// What a game asks of the app it runs in — Unity's `Application`.
abstract final class FynkelApplication {
  /// What [quit] does. A build exits the process; the editor replaces it
  /// while it plays the game, so a Quit button ends Play instead.
  static void Function() onQuit = _exit;

  /// Whether the game runs inside the editor's Play (set by the editor).
  static bool inEditor = false;

  static void _exit() => exit(0);

  /// Ends the game: the process in a build, Play in the editor. Games call
  /// this, never `exit()`.
  static void quit() => onQuit();
}

/// The editor's Pause and Step for the games below a [FynkelPlayScope]. A
/// paused `FynkelGameView` runs no frames and suspends its audio; [step]
/// runs one frame of the world's fixed step while paused.
class FynkelPlayControl extends ChangeNotifier {
  bool _paused = false;
  int _steps = 0;

  bool get paused => _paused;
  set paused(bool v) {
    if (v == _paused) return;
    _paused = v;
    notifyListeners();
  }

  /// Steps requested and not yet run (a view takes them, [takeStep]).
  int get pendingSteps => _steps;

  /// One frame, while paused (ignored while running).
  void step() {
    if (!_paused) return;
    _steps++;
    notifyListeners();
  }

  /// A view runs one requested step: true when there was one.
  bool takeStep() {
    if (_steps == 0) return false;
    _steps--;
    return true;
  }
}

/// Puts a [FynkelPlayControl] over a game (the editor's Game view).
class FynkelPlayScope extends InheritedNotifier<FynkelPlayControl> {
  const FynkelPlayScope({super.key, required FynkelPlayControl control, required super.child})
    : super(notifier: control);

  static FynkelPlayControl? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<FynkelPlayScope>()?.notifier;
}
