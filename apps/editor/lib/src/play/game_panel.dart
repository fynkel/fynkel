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
import '../profiler/profiler_model.dart';
import '../theme/panel_focus.dart';
import '../theme/tokens.dart';
import 'editor_game.dart';
import 'game_preview.dart';
import 'game_toolbar.dart';

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

  /// The Pause toggle, as Unity's: it may be on before Play (the game then
  /// runs its first frame and pauses) and stays as it is across Stop.
  bool get paused => _pause;
  bool _pause = false;

  /// Which run is mounted: a new Play is a new widget subtree, so nothing
  /// of the previous run's widgets survives.
  int get runs => _runs;

  /// Mounts the game, playing [startScene] (the open scene) when given.
  void start({required String projectDir, String? startScene}) {
    if (running) return;
    final watch = Stopwatch()..start();
    _launch = FynkelLaunch(projectDir: projectDir, startScene: startScene, inEditor: true);
    _runs++;
    control.paused = _pause;
    // Paused before Play, Unity still loads the scene and runs its first
    // frame, then pauses: a game that switches scenes on its first frame
    // (the open scene over its menu) shows the scene, not the menu.
    if (_pause && control.pendingSteps == 0) control.step();
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
    _pause = !_pause;
    if (running) control.paused = _pause;
    notifyListeners();
  }

  /// One frame of the world's fixed step, while paused.
  void step() {
    if (!running) return;
    if (!control.paused) {
      _pause = control.paused = true;
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

/// The Game panel: the toolbar (camera, aspect, scale, Stats) over the
/// running game, or — between runs — what the scene's active camera sees.
class GamePanel extends StatefulWidget {
  const GamePanel({
    super.key,
    required this.run,
    this.preview,
    this.options,
    this.profiler,
    this.showViewport = true,
  });

  /// Null in the plain editor (no game compiled in).
  final GameRun? run;

  /// The camera's picture between runs; null before a project is open.
  final GamePreview? preview;

  /// The toolbar's settings; the panel keeps its own when null.
  final GameViewOptions? options;

  /// Where the Stats box reads the game's frame times.
  final ProfilerModel? profiler;

  /// False in widget tests (no Metal texture).
  final bool showViewport;

  @override
  State<GamePanel> createState() => _GamePanelState();
}

class _GamePanelState extends State<GamePanel> {
  GameViewOptions? _own;
  final _size = ValueNotifier(Size.zero);

  GameViewOptions get _options => widget.options ?? (_own ??= GameViewOptions());

  @override
  void dispose() {
    _own?.dispose();
    _size.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final run = widget.run;
    if (run == null) return _between(context, playsHere: false);
    PanelFocus.primary(context, run.focusNode);
    return ListenableBuilder(
      listenable: run,
      builder: (context, _) {
        final launch = run.launch;
        if (launch == null) return _between(context, playsHere: true);
        return _framed(
          playing: true,
          child: ColoredBox(
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
          ),
        );
      },
    );
  }

  /// The toolbar over [child] at the chosen aspect and scale, with the
  /// Stats box in the corner.
  Widget _framed({required bool playing, required Widget child}) {
    final options = _options;
    return Column(
      children: [
        GameToolbar(options: options, preview: widget.preview),
        Expanded(
          child: Stack(
            fit: StackFit.expand,
            children: [
              GameFrame(options: options, onSize: (s) => _size.value = s, child: child),
              ListenableBuilder(
                listenable: options,
                builder: (context, _) => options.stats
                    ? Align(
                        alignment: Alignment.topRight,
                        child: Padding(
                          padding: const EdgeInsets.all(8),
                          child: GameStatsBox(size: _size, profiler: widget.profiler, playing: playing),
                        ),
                      )
                    : const SizedBox.shrink(),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _between(BuildContext context, {required bool playsHere}) {
    final hint = playsHere
        ? ''
        : 'This editor was started without the project\'s game. Open the project with '
              '`fynkel editor <dir>` to play it here, with Pause, Step and hot reload.';
    final preview = widget.preview;
    if (preview == null) {
      return _Idle(title: playsHere ? 'Press Play (⌘P)' : 'The game runs in its own window', detail: hint);
    }
    // The camera's name is in the toolbar; the placeholder is one quiet
    // line saying why there is no picture (docs/ui-refresh).
    return _framed(
      playing: false,
      child: GamePreviewView(
        preview: preview,
        showViewport: widget.showViewport,
        placeholder: (message) => playsHere
            ? _Idle(title: message, detail: '')
            : _Idle(title: 'The game runs in its own window', detail: '$message\n\n$hint'),
      ),
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
              // One quiet line; the detail only when it explains something (docs/ui-refresh).
              Text(title, style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary)),
              if (detail.isNotEmpty) ...[
                const SizedBox(height: 6),
                Text(
                  detail,
                  textAlign: TextAlign.center,
                  style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}
