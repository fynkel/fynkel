// A panel's padlock (items 9c and 9e): open and quiet while the panel
// follows the selection or the focused tab, shut and in the accent while it
// holds on. The Inspector, the Project browser and the Hierarchy use it.
import 'package:flutter/material.dart';

import 'icons.dart';
import 'tokens.dart';

class LockButton extends StatelessWidget {
  const LockButton({
    super.key,
    required this.locked,
    required this.onChanged,
    required this.lockTip,
    required this.unlockTip,
  });

  final bool locked;

  /// Null disables the padlock (nothing to hold on to yet).
  final ValueChanged<bool>? onChanged;
  final String lockTip;
  final String unlockTip;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final change = onChanged;
    return IconButton(
      tooltip: locked ? unlockTip : lockTip,
      isSelected: locked,
      icon: Icon(locked ? FIcons.lock : FIcons.lockOpen, size: 13),
      color: locked ? t.accent : t.textDisabled,
      padding: EdgeInsets.zero,
      constraints: const BoxConstraints.tightFor(width: 24, height: 24),
      onPressed: change == null ? null : () => change(!locked),
    );
  }
}
