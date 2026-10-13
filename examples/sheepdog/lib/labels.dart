// The widgets in the world, over the things they're about: each sheep's
// name, and each wolf's health. The game gives them to WorldWidgets; they
// are plain Flutter, in the app's theme.

import 'package:flutter/material.dart';

import 'game.dart' show Health;

/// A sheep's name, over it.
class NameTag extends StatelessWidget {
  const NameTag(this.name, {super.key});

  final String name;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(color: scheme.surface.withValues(alpha: 0.8), borderRadius: BorderRadius.circular(6)),
      child: Text(name, style: TextStyle(fontSize: 11, color: scheme.onSurface)),
    );
  }
}

/// A wolf's health, over it: a bar that empties as it's hit.
class HealthBar extends StatelessWidget {
  const HealthBar(this.health, {super.key});

  final Health health;

  @override
  Widget build(BuildContext context) => StreamBuilder(
    stream: health.hurt.stream,
    initialData: health.left,
    builder: (context, left) => Container(
      width: 40,
      height: 7,
      padding: const EdgeInsets.all(1),
      decoration: BoxDecoration(color: const Color(0xAA000000), borderRadius: BorderRadius.circular(3)),
      alignment: Alignment.centerLeft,
      child: FractionallySizedBox(
        widthFactor: left.requireData / health.max,
        heightFactor: 1,
        child: DecoratedBox(
          decoration: BoxDecoration(color: Theme.of(context).colorScheme.error, borderRadius: BorderRadius.circular(2)),
        ),
      ),
    ),
  );
}
