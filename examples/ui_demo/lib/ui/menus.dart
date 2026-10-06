// The game's screens: a title screen, a pause menu and a settings panel.
//
// Ordinary Flutter widgets (ADR-0062: game UI is Flutter, written in
// Dart). Which one shows is the `HudModel.screen` field — lib/main.dart
// switches on it with a `ModelBuilder` — and each button calls back into
// the page, which owns the game loop: the menus never touch the world.
// Restyle them, replace them, add screens (an inventory, a results screen)
// the same way: a value in the model, a widget per value.

import 'package:flutter/material.dart';
import 'package:fynkel/gameplay.dart' show SettingsDocument, SettingsService;

/// The project's quality profiles (fynkel.project.json `quality`), in order.
const qualityProfiles = ['low', 'medium', 'high'];

/// The title screen, over the scene's first frame (the world waits).
class TitleScreen extends StatelessWidget {
  const TitleScreen({super.key, required this.title, required this.onPlay, required this.onSettings, this.onQuit});

  final String title;
  final VoidCallback onPlay;
  final VoidCallback onSettings;

  /// Null hides Quit (phones: the system closes apps, not the game).
  final VoidCallback? onQuit;

  @override
  Widget build(BuildContext context) {
    return MenuBackdrop(
      child: MenuColumn(
        heading: title,
        children: [
          MenuButton('Play', onPressed: onPlay, autofocus: true),
          MenuButton('Settings', onPressed: onSettings),
          if (onQuit case final quit?) MenuButton('Quit', onPressed: quit),
        ],
      ),
    );
  }
}

/// The pause menu over the paused game.
class PauseMenu extends StatelessWidget {
  const PauseMenu({super.key, required this.onResume, required this.onSettings, required this.onMainMenu, this.onQuit});

  final VoidCallback onResume;
  final VoidCallback onSettings;
  final VoidCallback onMainMenu;

  /// Null hides Quit, as on the title screen.
  final VoidCallback? onQuit;

  @override
  Widget build(BuildContext context) {
    return MenuBackdrop(
      child: MenuColumn(
        heading: 'Paused',
        children: [
          MenuButton('Resume', onPressed: onResume, autofocus: true),
          MenuButton('Settings', onPressed: onSettings),
          MenuButton('Main menu', onPressed: onMainMenu),
          if (onQuit case final quit?) MenuButton('Quit', onPressed: quit),
        ],
      ),
    );
  }
}

/// The user's settings (docs/settings.md): every control edits the
/// `SettingsDocument` through [SettingsService.update], which saves
/// settings.json and applies the volumes before it returns; the panel
/// rebuilds from the store's listener, so the document is the one source.
/// The quality profile also goes to [onQuality] (the renderer reads it from
/// the viewport controller).
class SettingsPanel extends StatefulWidget {
  const SettingsPanel({super.key, required this.settings, required this.onBack, this.onQuality});

  final SettingsService settings;
  final VoidCallback onBack;
  final ValueChanged<String>? onQuality;

  @override
  State<SettingsPanel> createState() => _SettingsPanelState();
}

class _SettingsPanelState extends State<SettingsPanel> {
  void _changed(SettingsDocument _) => setState(() {});

  @override
  void initState() {
    super.initState();
    widget.settings.addListener(_changed);
  }

  @override
  void dispose() {
    widget.settings.removeListener(_changed);
    super.dispose();
  }

  void _update(SettingsDocument Function(SettingsDocument) change) =>
      widget.settings.update(change(widget.settings.current));

  @override
  Widget build(BuildContext context) {
    final s = widget.settings.current;
    final quality = qualityProfiles.contains(s.quality) ? s.quality : 'medium';
    return MenuBackdrop(
      child: MenuColumn(
        heading: 'Settings',
        children: [
          _Row(
            'Quality',
            SegmentedButton<String>(
              segments: [for (final q in qualityProfiles) ButtonSegment(value: q, label: Text(q))],
              selected: {quality},
              onSelectionChanged: (picked) {
                _update((d) => d.copyWith(quality: picked.single));
                widget.onQuality?.call(picked.single);
              },
            ),
          ),
          _volume('Master volume', s.masterVolume, (v) => _update((d) => d.copyWith(masterVolume: v))),
          _volume('Music', s.musicVolume, (v) => _update((d) => d.copyWith(musicVolume: v))),
          _volume('Effects', s.sfxVolume, (v) => _update((d) => d.copyWith(sfxVolume: v))),
          _Row(
            'Mute',
            Switch(
              value: s.muted,
              onChanged: (v) => _update((d) => d.copyWith(muted: v)),
            ),
          ),
          const SizedBox(height: 8),
          MenuButton('Back', onPressed: widget.onBack, autofocus: true),
        ],
      ),
    );
  }

  Widget _volume(String label, double value, ValueChanged<double> onChanged) =>
      _Row(label, Slider(value: value.clamp(0, 1), onChanged: onChanged, semanticFormatterCallback: _percent));

  static String _percent(double v) => '${(v * 100).round()}%';
}

class _Row extends StatelessWidget {
  const _Row(this.label, this.control);

  final String label;
  final Widget control;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        SizedBox(width: 140, child: Text(label)),
        Expanded(
          child: Align(alignment: Alignment.centerLeft, child: control),
        ),
      ],
    );
  }
}

/// Dims the game behind a menu and swallows the pointer, so a click on the
/// menu never reaches the game.
class MenuBackdrop extends StatelessWidget {
  const MenuBackdrop({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: Colors.black.withValues(alpha: 0.6),
      child: Center(child: child),
    );
  }
}

/// A heading over a column of controls, at most 420 wide.
class MenuColumn extends StatelessWidget {
  const MenuColumn({super.key, required this.heading, required this.children});

  final String heading;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 420),
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(heading, textAlign: TextAlign.center, style: Theme.of(context).textTheme.headlineMedium),
            const SizedBox(height: 24),
            for (final c in children) Padding(padding: const EdgeInsets.symmetric(vertical: 6), child: c),
          ],
        ),
      ),
    );
  }
}

/// A full-width menu button.
class MenuButton extends StatelessWidget {
  const MenuButton(this.label, {super.key, required this.onPressed, this.autofocus = false});

  final String label;
  final VoidCallback onPressed;
  final bool autofocus;

  @override
  Widget build(BuildContext context) {
    return FilledButton.tonal(
      onPressed: onPressed,
      autofocus: autofocus,
      style: FilledButton.styleFrom(minimumSize: const Size.fromHeight(48)),
      child: Text(label),
    );
  }
}
