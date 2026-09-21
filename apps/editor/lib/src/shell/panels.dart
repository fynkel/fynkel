// The §22.2 panels that later items fill. Each says which item brings it,
// so an empty panel is never mistaken for a broken one.
import 'package:flutter/material.dart';

class ComingPanel extends StatelessWidget {
  const ComingPanel({super.key, required this.title, required this.item, required this.what, required this.icon});

  final String title;
  final int item;
  final String what;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    // A squeezed panel (a few dozen pixels tall) scrolls the sign instead of overflowing.
    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 40, color: theme.disabledColor),
            const SizedBox(height: 12),
            Text(title, style: theme.textTheme.titleMedium),
            const SizedBox(height: 4),
            Text(
              '$what\nArrives with editor item $item.',
              textAlign: TextAlign.center,
              style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
            ),
          ],
        ),
      ),
    );
  }
}
