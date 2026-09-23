// The Game view (ADR-0021 P4): Play runs the project's game inside the
// editor, in its own panel beside the Scene view, as in Unity. The game is
// compiled into this editor build (`fynkel editor <dir>`), so Play is a
// mount, not a build: [GameRun.start] gives the panel a launch, the panel
// builds `buildGame(launch)` under a [FynkelPlayScope] (Pause and Step
// reach every `FynkelGameView` of the game, and Pause silences its audio),
// and [GameRun.stop] unmounts it — the game disposes its world and nothing
// of the run survives. The game's Quit ends Play, never the editor.
//
// While the game has the keyboard, the editor's bare keys (Q W E R, F, …)
// stay out of it ([KeyboardOwner]); ⌘-shortcuts (⌘P, ⌘S) still work.

import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show EditorChannel;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../commands/commands.dart' show KeyboardOwner;
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'editor_game.dart';

/// One project's in-editor Play: whether the game is mounted, paused, and
/// what it was launched with.
final class GameRun extends ChangeNotifier {
  GameRun({required this.game, this.onRecord, this.onQuit});

  /// The game compiled into this editor.
  final EditorGame game;

  /// Where the game's editor-channel records go (the Console's Game tab).
  final void Function(String line)? onRecord;

  /// What the game's Quit does while it plays (the shell's Stop).
  final VoidCallback? onQuit;

  /// Pause and Step, for every `FynkelGameView` of the game.
  final control = FynkelPlayControl();

  /// The Game view's keyboard: Play gives it the focus.
  final focusNode = FocusNode(debugLabel: 'game.view');

  FynkelLaunch? _launch;
  int _runs = 0;
  Duration startedIn = Duration.zero;
  void Function()? _quitOutside;

  FynkelLaunch? get launch => _launch;
  bool get running => _launch != null;
  bool get paused => running && control.paused;

  /// Which run is mounted: a new Play is a new widget subtree, so nothing
  /// of the previous run's widgets survives.
  int get runs => _runs;

  /// Mounts the game, playing [startScene] (the open scene) when given.
  void start({required String projectDir, String? startScene}) {
    if (running) return;
    final watch = Stopwatch()..start();
    _launch = FynkelLaunch(projectDir: projectDir, startScene: startScene, inEditor: true);
    _runs++;
    control.paused = false;
    FynkelApplication.inEditor = true;
    _quitOutside = FynkelApplication.onQuit;
    // Unity ignores Application.Quit in the editor; here it ends Play.
    FynkelApplication.onQuit = () => onQuit?.call();
    final sink = onRecord;
    if (sink != null) EditorChannel.debugUse(enabled: true, sink: sink);
    startedIn = watch.elapsed;
    notifyListeners();
  }

  /// Unmounts the game; its world is disposed with its widgets.
  void stop() {
    if (!running) return;
    _launch = null;
    control.paused = false;
    FynkelApplication.onQuit = _quitOutside ?? FynkelApplication.onQuit;
    _quitOutside = null;
    FynkelApplication.inEditor = false;
    if (onRecord != null) EditorChannel.debugUse();
    notifyListeners();
  }

  void togglePause() {
    if (!running) return;
    control.paused = !control.paused;
    notifyListeners();
  }

  /// One frame of the world's fixed step, while paused.
  void step() {
    if (!running) return;
    if (!control.paused) {
      control.paused = true;
      notifyListeners();
    }
    control.step();
  }

  @override
  void dispose() {
    stop();
    control.dispose();
    focusNode.dispose();
    super.dispose();
  }
}

/// The Game panel: the running game, or what Play will do.
class GamePanel extends StatelessWidget {
  const GamePanel({super.key, required this.run});

  /// Null in the plain editor (no game compiled in).
  final GameRun? run;

  @override
  Widget build(BuildContext context) {
    final run = this.run;
    if (run == null) {
      return const _Idle(
        title: 'The game runs in its own window',
        detail:
            'This editor was started without the project\'s game. Open the project with '
            '`fynkel editor <dir>` to play it here, with Pause, Step and hot reload.',
      );
    }
    return ListenableBuilder(
      listenable: run,
      builder: (context, _) {
        final launch = run.launch;
        if (launch == null) {
          return const _Idle(
            title: 'Press Play (⌘P)',
            detail: 'The game runs here, starting in the scene open in the editor.',
          );
        }
        return ColoredBox(
          color: Colors.black,
          child: KeyboardOwner(
            child: Focus(
              focusNode: run.focusNode,
              autofocus: true,
              child: FynkelPlayScope(
                control: run.control,
                child: KeyedSubtree(key: ValueKey(run.runs), child: run.game.build(launch)),
              ),
            ),
          ),
        );
      },
    );
  }
}

class _Idle extends StatelessWidget {
  const _Idle({required this.title, required this.detail});

  final String title;
  final String detail;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    return ColoredBox(
      color: t.panel,
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 380),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(FIcons.playArrow, size: 28, color: t.textSecondary),
              const SizedBox(height: 10),
              Text(title, style: theme.textTheme.titleSmall?.copyWith(color: t.text)),
              const SizedBox(height: 6),
              Text(
                detail,
                textAlign: TextAlign.center,
                style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
