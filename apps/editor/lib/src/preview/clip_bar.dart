// The play bar over a 3D preview's foot while a clip (or a blend space)
// poses the model, as Unity's animation preview: play/pause, a scrub bar,
// the time and frame, the speed (0.25× to 2×), loop, and root motion in
// place or moving. A blend space adds its parameter's slider.
import 'package:flutter/material.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../theme/icons.dart';
import '../theme/tokens.dart';

/// The speeds the bar offers.
const clipSpeeds = [0.25, 0.5, 1.0, 1.5, 2.0];

/// `0:01.20`.
String clipTime(double seconds) {
  final cs = (seconds * 100).round();
  final m = cs ~/ 6000, s = (cs ~/ 100) % 60, rest = cs % 100;
  return '$m:${'$s'.padLeft(2, '0')}.${'$rest'.padLeft(2, '0')}';
}

/// `0.25×`, `1×`.
String speedLabel(double speed) => '${speed == speed.roundToDouble() ? speed.round() : speed}×';

class ClipBar extends StatelessWidget {
  const ClipBar({
    super.key,
    required this.clip,
    required this.playing,
    required this.speed,
    required this.loop,
    required this.onPlay,
    required this.onScrub,
    required this.onSpeed,
    required this.onLoop,
    required this.onInPlace,
    this.onScrubEnd,
    this.onParameter,
  });

  final StageClip clip;
  final bool playing, loop;
  final double speed;
  final VoidCallback onPlay;
  final void Function(double time) onScrub;
  final VoidCallback? onScrubEnd;
  final void Function(double speed) onSpeed;
  final VoidCallback onLoop, onInPlace;
  final void Function(double value)? onParameter;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall?.copyWith(
      color: t.textSecondary,
      fontFeatures: const [FontFeature.tabularFigures()],
    );
    final frame = (clip.time * clip.fps).round();
    Widget toggle(String key, IconData icon, String tip, bool on, VoidCallback tap) => Tooltip(
      message: tip,
      child: InkWell(
        key: Key(key),
        onTap: tap,
        borderRadius: t.cornerControl,
        child: Container(
          padding: const EdgeInsets.all(3),
          decoration: BoxDecoration(color: on ? t.selectionQuiet : null, borderRadius: t.cornerControl),
          child: Icon(icon, size: 13, color: on ? t.text : t.textSecondary),
        ),
      ),
    );
    final slider = SliderTheme(
      data: SliderTheme.of(context).copyWith(
        trackHeight: 2,
        thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 5),
        overlayShape: SliderComponentShape.noOverlay,
      ),
      child: Slider(
        key: const Key('preview.clip.scrub'),
        value: clip.duration > 0 ? clip.time.clamp(0.0, clip.duration) : 0,
        max: clip.duration > 0 ? clip.duration : 1,
        onChanged: clip.duration > 0 ? onScrub : null,
        onChangeEnd: (_) => onScrubEnd?.call(),
      ),
    );
    final blend = clip.blend;
    return Container(
      key: const Key('preview.clip'),
      color: t.panel.withValues(alpha: 0.85),
      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (blend != null && blend.points.length > 1 && onParameter != null)
            Row(
              children: [
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 4),
                  child: Text(blend.parameter, style: small),
                ),
                Expanded(
                  child: SliderTheme(
                    data: SliderTheme.of(context).copyWith(
                      trackHeight: 2,
                      thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 5),
                      overlayShape: SliderComponentShape.noOverlay,
                    ),
                    child: Slider(
                      key: const Key('preview.clip.parameter'),
                      value: blend.value.clamp(blend.points.first.at, blend.points.last.at),
                      min: blend.points.first.at,
                      max: blend.points.last.at,
                      onChanged: onParameter,
                    ),
                  ),
                ),
                SizedBox(width: 40, child: Text(blend.value.toStringAsFixed(2), style: small)),
              ],
            ),
          Row(
            children: [
              toggle(
                'preview.clip.play',
                playing ? FIcons.pause : FIcons.playArrow,
                playing ? 'Pause' : 'Play',
                playing,
                onPlay,
              ),
              const SizedBox(width: 4),
              Expanded(child: slider),
              const SizedBox(width: 6),
              Text(
                '${clipTime(clip.time)}  ($frame)',
                key: const Key('preview.clip.time'),
                style: small,
              ),
              const SizedBox(width: 4),
              PopupMenuButton<double>(
                key: const Key('preview.clip.speed'),
                tooltip: 'Playback speed',
                popUpAnimationStyle: AnimationStyle.noAnimation,
                initialValue: speed,
                onSelected: onSpeed,
                itemBuilder: (_) => [
                  for (final s in clipSpeeds)
                    PopupMenuItem(
                      key: Key('preview.clip.speed.$s'),
                      value: s,
                      height: 28,
                      child: Text(speedLabel(s), style: small),
                    ),
                ],
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
                  child: Text(speedLabel(speed), style: small),
                ),
              ),
              toggle('preview.clip.loop', FIcons.repeat, 'Loop', loop, onLoop),
              const SizedBox(width: 2),
              toggle(
                'preview.clip.move',
                FIcons.directionsRun,
                clip.inPlace ? 'Root motion: in place (click to let it move)' : 'Root motion: moving (click to hold in place)',
                !clip.inPlace,
                onInPlace,
              ),
            ],
          ),
        ],
      ),
    );
  }
}
