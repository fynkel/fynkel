// The settings panel (M3-15): every control edits the user's
// `SettingsDocument` through `Game.settings.update` (docs/settings.md):
// the store writes settings.json, sets the audio buses and lays the
// rebinds over the project's actions before the call returns, and this
// panel rebuilds from the store's listener — the document is the one
// source, nothing is mirrored here. The quality profile goes to the
// viewport controller as well (the renderer reads it from there).
//
// Rebinding is the page's business (lib/main.dart): the next key comes
// through the game view's `onKeyEvent`, so the panel only says which
// action waits ([rebinding]) and asks the page to start or stop waiting
// ([onRebind]).

import 'package:flutter/material.dart';
import 'package:fynkel/gameplay.dart' show SettingsDocument;
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../game/game.dart';
import 'hud.dart' show keyboardKey;
import 'menu.dart';

/// The quality profiles of fynkel.project.json, in order.
const qualityProfiles = ['low', 'medium', 'high'];

class SettingsPanel extends StatefulWidget {
  const SettingsPanel({
    super.key,
    required this.game,
    required this.viewport,
    required this.rebinding,
    required this.onRebind,
    required this.onBack,
  });

  final Game game;
  final FynkelViewportController viewport;

  /// The button action waiting for its key, null when none.
  final String? rebinding;

  /// Start waiting for a key for [action] (null: stop waiting).
  final void Function(String? action) onRebind;
  final VoidCallback onBack;

  @override
  State<SettingsPanel> createState() => _SettingsPanelState();
}

class _SettingsPanelState extends State<SettingsPanel> {
  void _changed(SettingsDocument _) => setState(() {});

  @override
  void initState() {
    super.initState();
    widget.game.settings.addListener(_changed);
  }

  @override
  void dispose() {
    widget.game.settings.removeListener(_changed);
    super.dispose();
  }

  void _update(SettingsDocument next) => widget.game.settings.update(next);

  void _quality(String profile) {
    widget.viewport.quality = profile;
    _update(widget.game.settings.current.copyWith(quality: profile));
  }

  @override
  Widget build(BuildContext context) {
    final game = widget.game;
    final current = game.settings.current;
    final theme = Theme.of(context).textTheme;
    // The button actions, rebindable; the axes (move, look) are not.
    final actions = game.report.inputActions.where((a) => a.type == 'button').toList();
    return MenuPanel(
      game: game,
      children: [
        Text('Settings', style: theme.headlineLarge?.copyWith(fontWeight: FontWeight.bold)),
        const SizedBox(height: 16),
        SizedBox(
          width: 420,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('Audio', style: theme.titleMedium),
              _VolumeRow(
                label: 'Master',
                value: current.masterVolume,
                onChanged: (v) => _update(current.copyWith(masterVolume: v)),
              ),
              _VolumeRow(
                label: 'Music',
                value: current.musicVolume,
                onChanged: (v) => _update(current.copyWith(musicVolume: v)),
              ),
              _VolumeRow(
                label: 'Effects',
                value: current.sfxVolume,
                onChanged: (v) => _update(current.copyWith(sfxVolume: v)),
              ),
              SwitchListTile(
                title: const Text('Mute'),
                value: current.muted,
                onChanged: (v) => _update(current.copyWith(muted: v)),
                contentPadding: EdgeInsets.zero,
              ),
              const SizedBox(height: 12),
              Text('Quality', style: theme.titleMedium),
              const SizedBox(height: 4),
              SegmentedButton<String>(
                segments: [for (final p in qualityProfiles) ButtonSegment(value: p, label: Text(p))],
                selected: {qualityProfiles.contains(current.quality) ? current.quality : 'medium'},
                onSelectionChanged: (s) => _quality(s.first),
              ),
              const SizedBox(height: 12),
              Text('Keys', style: theme.titleMedium),
              for (final a in actions)
                _BindingRow(
                  action: a.name,
                  bindings: game.bindingsOf(a.name),
                  rebound: current.rebinds.containsKey(a.name),
                  waiting: widget.rebinding == a.name,
                  onRebind: () => widget.onRebind(widget.rebinding == a.name ? null : a.name),
                  onReset: () => _update(current.rebind(a.name, const [])),
                ),
              if (widget.rebinding != null)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: Text(
                    'Press a key for "${widget.rebinding}" (Esc cancels)',
                    style: theme.bodyMedium?.copyWith(color: Colors.amber),
                  ),
                ),
            ],
          ),
        ),
        const SizedBox(height: 16),
        MenuButton(
          game: game,
          label: 'Reset to defaults',
          onPressed: () {
            widget.onRebind(null);
            widget.viewport.quality = null;
            _update(SettingsDocument.defaults);
          },
        ),
        MenuButton(game: game, label: 'Back (Esc)', primary: true, onPressed: widget.onBack),
      ],
    );
  }
}

class _VolumeRow extends StatelessWidget {
  const _VolumeRow({required this.label, required this.value, required this.onChanged});

  final String label;
  final double value;
  final ValueChanged<double> onChanged;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        SizedBox(width: 72, child: Text(label)),
        Expanded(
          child: Slider(value: value.clamp(0, 1), onChanged: onChanged),
        ),
        SizedBox(width: 40, child: Text('${(value * 100).round()}%', textAlign: TextAlign.right)),
      ],
    );
  }
}

/// One action: its keyboard key (and the other devices, dimmed), Rebind,
/// Reset when the user changed it.
class _BindingRow extends StatelessWidget {
  const _BindingRow({
    required this.action,
    required this.bindings,
    required this.rebound,
    required this.waiting,
    required this.onRebind,
    required this.onReset,
  });

  final String action;
  final List<String> bindings;
  final bool rebound;
  final bool waiting;
  final VoidCallback onRebind;
  final VoidCallback onReset;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context).textTheme;
    final others = bindings.where((b) => !b.startsWith('keyboard:')).map((b) => b.split(':').last).join(', ');
    return Row(
      children: [
        SizedBox(width: 72, child: Text(action)),
        Expanded(
          child: Text.rich(
            TextSpan(
              children: [
                TextSpan(
                  text: waiting ? '…' : (keyboardKey(bindings) ?? '—'),
                  style: theme.bodyLarge?.copyWith(fontWeight: FontWeight.bold),
                ),
                if (others.isNotEmpty)
                  TextSpan(
                    text: '   $others',
                    style: theme.bodySmall?.copyWith(color: Colors.white54),
                  ),
              ],
            ),
          ),
        ),
        TextButton(onPressed: onRebind, child: Text(waiting ? 'Cancel' : 'Rebind')),
        if (rebound) TextButton(onPressed: onReset, child: const Text('Reset')),
      ],
    );
  }
}
