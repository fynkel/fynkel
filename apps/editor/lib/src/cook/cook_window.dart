// Unity's "Importing Assets" window (ADR-0040 §4): over the whole editor
// while a cook runs longer than a moment — the file, the count, a bar and
// Cancel. The editor behind it keeps drawing but takes no clicks, and the
// command registry takes no keys ([CommandRegistry.blocked]).
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'cook_coordinator.dart';

/// [child] with the cook window over it while [cooks] runs a cook.
class CookWindow extends StatelessWidget {
  const CookWindow({super.key, required this.cooks, required this.child});

  final CookCoordinator cooks;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    // The child is built once, outside the builder: a poll redraws the
    // window, never the editor under it.
    return Stack(
      children: [
        // A one-child slot: when the editor's root changes type (Play's
        // tint) the old docking goes before the new one is built.
        Positioned.fill(child: child),
        Positioned.fill(
          child: ListenableBuilder(
            listenable: cooks,
            builder: (context, _) {
              if (!cooks.isRunning) return const SizedBox.shrink();
              // Invisible until the window shows: a short cook only holds
              // the clicks for its moment.
              return Stack(
                children: [
                  ModalBarrier(
                    key: const Key('cook.barrier'),
                    dismissible: false,
                    color: cooks.isShowing ? Colors.black26 : Colors.transparent,
                  ),
                  if (cooks.isShowing) Center(child: _Window(cooks: cooks)),
                ],
              );
            },
          ),
        ),
      ],
    );
  }
}

class _Window extends StatelessWidget {
  const _Window({required this.cooks});

  final CookCoordinator cooks;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final total = cooks.total;
    return Focus(
      autofocus: true,
      // The editor's keys wait; Escape is Cancel.
      onKeyEvent: (_, event) {
        if (event is KeyDownEvent && event.logicalKey == LogicalKeyboardKey.escape) cooks.cancel();
        return KeyEventResult.handled;
      },
      child: Card(
        key: const Key('cook.window'),
        child: SizedBox(
          width: 380,
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('Importing Assets', style: theme.textTheme.titleMedium),
                const SizedBox(height: 8),
                Text(
                  cooks.current.isEmpty ? 'Reading the project…' : cooks.current,
                  key: const Key('cook.current'),
                  style: theme.textTheme.bodySmall,
                  overflow: TextOverflow.ellipsis,
                ),
                const SizedBox(height: 8),
                LinearProgressIndicator(value: total == 0 ? null : cooks.progress),
                const SizedBox(height: 8),
                Row(
                  children: [
                    Text(
                      total == 0 ? '' : '${cooks.done} of $total',
                      key: const Key('cook.count'),
                      style: theme.textTheme.bodySmall,
                    ),
                    const Spacer(),
                    TextButton(key: const Key('cook.cancel'), onPressed: cooks.cancel, child: const Text('Cancel')),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
