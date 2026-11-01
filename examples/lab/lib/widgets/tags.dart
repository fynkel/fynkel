// The world widgets of the lab's "Widgets in the world" scene: plain
// Flutter, which the scene puts over its cubes.

import 'package:flutter/material.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show WorldWidget;

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

/// A name while its point is on the screen; an arrow toward it at the
/// screen's edge while it isn't (`WorldWidget(keepOnScreen: true)`).
class EdgeArrow extends StatelessWidget {
  const EdgeArrow(this.name, {super.key});

  final String name;

  @override
  Widget build(BuildContext context) {
    final place = WorldWidget.of(context);
    return ValueListenableBuilder(
      valueListenable: place.offScreen,
      builder: (context, off, _) => off
          ? ValueListenableBuilder(
              valueListenable: place.angle,
              builder: (context, angle, _) => DecoratedBox(
                decoration: const BoxDecoration(color: Color(0xCCFFD040), shape: BoxShape.circle),
                child: Padding(
                  padding: const EdgeInsets.all(6),
                  child: Transform.rotate(angle: angle, child: const Icon(Icons.arrow_forward, size: 20)),
                ),
              ),
            )
          : NameTag(name),
    );
  }
}

/// A red cross, to see whether a widget stays on what it marks.
class Crosshair extends StatelessWidget {
  const Crosshair({super.key});

  @override
  Widget build(BuildContext context) => const SizedBox(
    width: 40,
    height: 40,
    child: Stack(
      children: [
        Positioned(
          left: 19,
          top: 0,
          bottom: 0,
          child: ColoredBox(color: Colors.red, child: SizedBox(width: 2)),
        ),
        Positioned(
          top: 19,
          left: 0,
          right: 0,
          child: ColoredBox(color: Colors.red, child: SizedBox(height: 2)),
        ),
      ],
    ),
  );
}
