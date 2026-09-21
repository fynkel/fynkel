// The in-game HUD (docs/hud.md): every widget binds to one `GameState`
// field through `ModelBuilder` and rebuilds when that field changes —
// `collected` on a coin, the clock every tenth, `deaths` on a death — never
// on a frame where nothing changed, and nothing here reads the engine.

import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../game/game.dart';
import '../game/game_state.dart';

/// The HUD counter: `Coins n / 10`, the clock, the deaths and the
/// footsteps, one widget per model field.
class Counter extends StatelessWidget {
  const Counter({super.key});

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    final theme = Theme.of(context).textTheme;
    final big = theme.headlineSmall?.copyWith(fontWeight: FontWeight.bold, shadows: const [Shadow(blurRadius: 6)]);
    final small = theme.bodyLarge?.copyWith(shadows: const [Shadow(blurRadius: 6)]);
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('Coins ', style: big),
            ModelBuilder<int>(
              value: state.collected,
              builder: (_, n, _) => Text('$n', style: big),
            ),
            Text(' / ', style: big),
            ModelBuilder<int>(
              value: state.total,
              builder: (_, n, _) => Text('$n', style: big),
            ),
          ],
        ),
        ModelBuilder<int>(
          value: state.elapsedTenths,
          builder: (_, t, _) => Text(formatTenths(t), style: small),
        ),
        ModelBuilder<int>(
          value: state.deaths,
          builder: (_, n, _) => Text(n == 0 ? '' : 'deaths $n', style: small),
        ),
        ModelBuilder<int>(
          value: state.footsteps,
          builder: (_, n, _) => Text(n == 0 ? '' : 'steps $n', style: small),
        ),
      ],
    );
  }
}

/// The prompt over the switch (`E — open the gate`), visible while the
/// player stands on it; pinned to the pad by a `ProjectedAnchor`.
class SwitchPrompt extends StatelessWidget {
  const SwitchPrompt({super.key});

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    return ModelBuilder<String>(
      value: state.hint,
      builder: (context, hint, _) => hint.isEmpty
          ? const SizedBox.shrink()
          : DecoratedBox(
              decoration: BoxDecoration(
                color: Colors.cyan.withValues(alpha: 0.8),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                child: Text(hint, style: Theme.of(context).textTheme.bodyMedium?.copyWith(color: Colors.black)),
              ),
            ),
    );
  }
}

/// The bottom line: the controls until the first coin, then nothing. The
/// keys are the bindings in force (a rebind shows here).
class Hint extends StatelessWidget {
  const Hint({super.key, required this.game});

  final Game game;

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    return ModelBuilder<int>(
      value: state.collected,
      builder: (context, n, _) {
        if (n > 0) return const SizedBox.shrink();
        String key(String action) => keyboardKey(game.bindingsOf(action)) ?? '—';
        return Text(
          'WASD / left stick: walk   mouse / right stick: look   ${key('jump')} / A: jump   '
          '${key('sprint')} / LB: sprint   ${key('interact')} / X: use   ${key('wave')} / B: wave   ${key('restart')} / Y: restart',
          style: Theme.of(context).textTheme.bodyMedium?.copyWith(shadows: const [Shadow(blurRadius: 6)]),
          textAlign: TextAlign.center,
        );
      },
    );
  }
}

/// The first keyboard key among [bindings] (`keyboard:Space` → `Space`),
/// null when the action has none.
String? keyboardKey(List<String> bindings) {
  for (final b in bindings) {
    if (b.startsWith('keyboard:')) return b.substring('keyboard:'.length);
  }
  return null;
}

/// The moment of the win: shown while `GameState.won` is set, until the
/// Results scene takes over (`winDelay`).
class WinFlash extends StatelessWidget {
  const WinFlash({super.key});

  @override
  Widget build(BuildContext context) {
    final state = ModelBinding.of<GameState>(context);
    return ModelBuilder<bool>(
      value: state.won,
      builder: (context, won, _) => won
          ? Center(
              child: Text(
                'All coins collected!',
                style: Theme.of(context).textTheme.headlineMedium?.copyWith(shadows: const [Shadow(blurRadius: 12)]),
              ),
            )
          : const SizedBox.shrink(),
    );
  }
}

/// The engine's numbers (F1): once a second from `onStats`.
class StatsBlock extends StatelessWidget {
  const StatsBlock({super.key, required this.game, required this.stats, required this.viewport});

  final Game game;
  final FynkelFrameStats? stats;
  final FynkelViewportController viewport;

  @override
  Widget build(BuildContext context) {
    final style = Theme.of(context).textTheme.bodyMedium?.copyWith(fontFamily: 'monospace');
    final s = stats;
    final report = game.scenes.lastReport ?? game.report;
    final lines = <String>[
      'scene      ${report.sceneName} (${report.entityCount} entities)',
      'tick       ${game.world.tick}',
      'behaviours ${game.world.behaviours.length} attached',
      'quality    ${viewport.quality ?? 'medium (project default)'}  [Q cycles]',
      if (s != null) ...[
        'fps        ${s.fps.toStringAsFixed(0)}  frame ${s.tickInterval.p50.toStringAsFixed(2)} ms '
            '(p99 ${s.tickInterval.p99.toStringAsFixed(2)})  render ${s.renderCall.p50.toStringAsFixed(2)} ms',
        'simulate   ${s.simulate.p50.toStringAsFixed(2)} ms (p99 ${s.simulate.p99.toStringAsFixed(2)})  '
            '${s.fixedTicks} ticks/s  ${s.missedDeadlines} missed, ${s.dropped} dropped',
        if (s.viewport case final v?)
          'meshes     ${v.instances} drawn in ${v.draws} draws, ${v.culled} culled, ${v.missing} missing',
        if (game.world.audio.stats case final a)
          'audio      ${a.voices} voices, ${a.streams} streams, ${a.underruns} underruns, ${a.starved} starved',
      ],
      for (final d in report.diagnostics) '$d',
    ];
    return DecoratedBox(
      decoration: BoxDecoration(color: Colors.black.withValues(alpha: 0.55), borderRadius: BorderRadius.circular(8)),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [for (final l in lines) Text(l, style: style)],
        ),
      ),
    );
  }
}
