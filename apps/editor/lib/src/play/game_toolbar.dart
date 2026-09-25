// The Game view's toolbar, as Unity's (docs/ui-refresh): the camera the
// picture is drawn through, the aspect ratio it keeps, its scale, and the
// Stats box. The same frame holds the picture between runs and the game
// while it plays, so an aspect or a zoom chosen before Play stays in Play.
import 'package:flutter/material.dart';

import '../profiler/profiler_model.dart';
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'game_preview.dart';

/// The aspect ratios the picture can keep; [free] fills the panel.
enum GameAspect {
  free('Free Aspect', null),
  wide('16:9', 16 / 9),
  wide10('16:10', 16 / 10),
  classic('4:3', 4 / 3),
  photo('3:2', 3 / 2),
  ultrawide('21:9', 21 / 9),
  portrait('9:16 Portrait', 9 / 16),
  square('1:1', 1);

  const GameAspect(this.label, this.ratio);
  final String label;

  /// Width over height; null is the panel's own.
  final double? ratio;
}

/// What the Game toolbar sets; one for the editor, so it outlives the
/// panel being moved in the dock.
final class GameViewOptions extends ChangeNotifier {
  static const minScale = 1.0;
  static const maxScale = 5.0;

  GameAspect aspect = GameAspect.free;
  double scale = 1;
  bool stats = false;

  void setAspect(GameAspect value) {
    if (value == aspect) return;
    aspect = value;
    notifyListeners();
  }

  void setScale(double value) {
    final v = value.clamp(minScale, maxScale).toDouble();
    if (v == scale) return;
    scale = v;
    notifyListeners();
  }

  void toggleStats() {
    stats = !stats;
    notifyListeners();
  }
}

/// The thin bar above the picture.
class GameToolbar extends StatelessWidget {
  const GameToolbar({super.key, required this.options, required this.preview});

  final GameViewOptions options;
  final GamePreview? preview;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final small = theme.textTheme.bodySmall;
    final preview = this.preview;
    return Container(
      key: const Key('game.toolbar'),
      height: t.row + 6,
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      padding: const EdgeInsets.symmetric(horizontal: 6),
      child: ListenableBuilder(
        listenable: Listenable.merge([options, ?preview]),
        builder: (context, _) {
          final cam = preview?.message == null ? preview?.camera : null;
          return Row(
            children: [
              Tooltip(
                message: 'The scene\'s active camera: the enabled Camera with the highest priority',
                waitDuration: const Duration(milliseconds: 500),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(FIcons.videocamOutlined, size: 13, color: t.textSecondary),
                    const SizedBox(width: 5),
                    ConstrainedBox(
                      constraints: const BoxConstraints(maxWidth: 160),
                      child: Text(
                        cam == null ? 'No camera' : (cam.name.isEmpty ? 'Camera' : cam.name),
                        key: const Key('game.camera'),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: small?.copyWith(color: cam == null ? t.textDisabled : t.text),
                      ),
                    ),
                  ],
                ),
              ),
              _divider(t),
              PopupMenuButton<GameAspect>(
                key: const Key('game.aspect'),
                popUpAnimationStyle: AnimationStyle.noAnimation,
                tooltip: 'The aspect ratio the picture keeps',
                onSelected: options.setAspect,
                itemBuilder: (_) => [
                  for (final a in GameAspect.values)
                    PopupMenuItem(
                      key: ValueKey('game.aspect.${a.name}'),
                      value: a,
                      height: 26,
                      child: Row(
                        children: [
                          SizedBox(width: 18, child: a == options.aspect ? const Icon(FIcons.check, size: 14) : null),
                          Text(a.label, style: small),
                        ],
                      ),
                    ),
                ],
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 3),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(options.aspect.label, style: small?.copyWith(color: t.text)),
                      const SizedBox(width: 3),
                      Icon(FIcons.chevronDown, size: 12, color: t.textDisabled),
                    ],
                  ),
                ),
              ),
              _divider(t),
              Tooltip(
                message: 'Zoom into the picture; click to go back to 1x',
                waitDuration: const Duration(milliseconds: 500),
                child: GestureDetector(
                  key: const Key('game.scale.reset'),
                  onTap: () => options.setScale(1),
                  child: Text('Scale', style: small?.copyWith(color: t.textSecondary)),
                ),
              ),
              const SizedBox(width: 8),
              SizedBox(
                width: 96,
                child: Slider(
                  key: const Key('game.scale'),
                  min: GameViewOptions.minScale,
                  max: GameViewOptions.maxScale,
                  value: options.scale,
                  onChanged: options.setScale,
                ),
              ),
              const SizedBox(width: 8),
              SizedBox(
                width: 30,
                child: Text(
                  '${options.scale.toStringAsFixed(1)}x',
                  style: small?.copyWith(color: t.textSecondary, fontFeatures: const [FontFeature.tabularFigures()]),
                ),
              ),
              const Spacer(),
              _Toggle(
                key: const Key('game.stats'),
                label: 'Stats',
                tooltip: 'Frame rate, frame times, draw calls and resolution over the picture',
                on: options.stats,
                onTap: options.toggleStats,
              ),
            ],
          );
        },
      ),
    );
  }

  Widget _divider(FynkelTokens t) =>
      Container(width: 1, height: 14, margin: const EdgeInsets.symmetric(horizontal: 8), color: t.line);
}

/// A flat text toggle: quiet grey fill while on (Unity's Stats, Gizmos).
class _Toggle extends StatelessWidget {
  const _Toggle({super.key, required this.label, required this.tooltip, required this.on, required this.onTap});

  final String label;
  final String tooltip;
  final bool on;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return Tooltip(
      message: tooltip,
      waitDuration: const Duration(milliseconds: 500),
      child: InkWell(
        onTap: onTap,
        borderRadius: t.cornerControl,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
          decoration: BoxDecoration(color: on ? t.selectionQuiet : null, borderRadius: t.cornerControl),
          child: Text(
            label,
            style: Theme.of(context).textTheme.bodySmall?.copyWith(color: on ? t.text : t.textSecondary),
          ),
        ),
      ),
    );
  }
}

/// [child] at the chosen aspect ratio (bars around it) and scale (zoomed
/// about the centre, clipped). The tree is the same whatever is chosen,
/// so the picture's texture survives a change.
class GameFrame extends StatelessWidget {
  const GameFrame({super.key, required this.options, required this.child, this.onSize});

  final GameViewOptions options;
  final Widget child;

  /// The picture's size in logical pixels, for the Stats box.
  final ValueChanged<Size>? onSize;

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: context.tokens.window,
      child: ClipRect(
        child: LayoutBuilder(
          builder: (context, box) => ListenableBuilder(
            listenable: options,
            builder: (context, _) {
              final size = _fit(box.biggest, options.aspect.ratio);
              if (onSize != null) WidgetsBinding.instance.addPostFrameCallback((_) => onSize!(size));
              return Center(
                child: Transform.scale(
                  scale: options.scale,
                  child: SizedBox.fromSize(size: size, child: child),
                ),
              );
            },
          ),
        ),
      ),
    );
  }

  static Size _fit(Size area, double? ratio) {
    if (ratio == null || area.isEmpty || !area.isFinite) return area;
    return area.width / area.height > ratio
        ? Size((area.height * ratio).floorToDouble(), area.height)
        : Size(area.width, (area.width / ratio).floorToDouble());
  }
}

/// Unity's Statistics box: the game's last stats window while it plays,
/// and the picture's resolution.
class GameStatsBox extends StatelessWidget {
  const GameStatsBox({super.key, required this.size, this.profiler, required this.playing});

  /// The picture's size in logical pixels.
  final ValueNotifier<Size> size;
  final ProfilerModel? profiler;
  final bool playing;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final style = Theme.of(context).textTheme.bodySmall?.copyWith(
      color: t.text,
      fontFeatures: const [FontFeature.tabularFigures()],
    );
    final dim = style?.copyWith(color: t.textSecondary);
    final profiler = this.profiler;
    return ListenableBuilder(
      listenable: Listenable.merge([size, ?profiler]),
      builder: (context, _) {
        final dpr = MediaQuery.devicePixelRatioOf(context);
        final px = size.value * dpr;
        final frame = playing && profiler != null && profiler.frames.isNotEmpty ? profiler.frames.last : null;
        String ms(double? v) => v == null ? '—' : '${v.toStringAsFixed(1)} ms';
        String count(double? v) => v == null ? '—' : v.round().toString();
        final rows = <(String, String)>[
          if (frame != null) ...[
            ('FPS', frame.values['fps']?.toStringAsFixed(1) ?? '—'),
            ('CPU', ms(frame.metric('work', pct: 'p50'))),
            ('GPU', ms(frame.metric('gpu_frame', pct: 'p50'))),
            ('Draw calls', count(frame.values['draws'])),
            ('Entities', count(frame.values['entities'])),
          ],
          ('Resolution', '${px.width.round()}×${px.height.round()}'),
        ];
        return Container(
          key: const Key('game.statsBox'),
          padding: const EdgeInsets.fromLTRB(10, 8, 10, 8),
          decoration: BoxDecoration(
            color: t.overlay,
            border: Border.all(color: t.overlayEdge),
            borderRadius: t.cornerControl,
          ),
          child: IntrinsicWidth(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text('Statistics', style: style?.copyWith(fontWeight: FontWeight.w600)),
                const SizedBox(height: 4),
                for (final (label, value) in rows)
                  Row(
                    children: [
                      Text(label, style: dim),
                      const SizedBox(width: 16),
                      const Spacer(),
                      Text(value, style: style),
                    ],
                  ),
                if (frame == null) ...[
                  const SizedBox(height: 4),
                  Text(
                    playing ? 'The first numbers come after a second.' : 'Frame times show while the game plays.',
                    style: dim?.copyWith(color: t.textDisabled),
                  ),
                ],
              ],
            ),
          ),
        );
      },
    );
  }
}
