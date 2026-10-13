// The world widgets of the lab's "Widgets in the world" scene: plain
// Flutter, which the scene puts over its cubes.

import 'package:flutter/material.dart';

/// A name over a cube.
class NameTag extends StatelessWidget {
  const NameTag(this.name, {super.key});

  final String name;

  @override
  Widget build(BuildContext context) => DecoratedBox(
    decoration: BoxDecoration(color: const Color(0xCC000000), borderRadius: BorderRadius.circular(6)),
    child: Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      child: Text(name, style: const TextStyle(color: Colors.white, fontSize: 14)),
    ),
  );
}

/// A name with a button.
class ButtonTag extends StatelessWidget {
  const ButtonTag(this.name, {super.key, required this.onPressed});

  final String name;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) => Material(
    color: const Color(0xCC000000),
    borderRadius: BorderRadius.circular(8),
    child: Padding(
      padding: const EdgeInsets.all(4),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 6),
            child: Text(name, style: const TextStyle(color: Colors.white, fontSize: 14)),
          ),
          FilledButton.tonal(onPressed: onPressed, child: const Text('press')),
        ],
      ),
    ),
  );
}

/// A number that pops up, as a hit's.
class PopText extends StatelessWidget {
  const PopText(this.text, {super.key});

  final String text;

  @override
  Widget build(BuildContext context) => Text(
    text,
    style: const TextStyle(
      color: Color(0xFFFFD040),
      fontSize: 22,
      fontWeight: FontWeight.bold,
      shadows: [Shadow(blurRadius: 4)],
    ),
  );
}
