// Which panel holds focus (blueprint item 9b d): "the accent follows
// focus". A selected row is drawn in the accent only in the panel the
// keyboard is in; every other panel shows its selection as a quiet tint,
// so there is never more than one blue selection on screen.
//
// The workspace wraps every panel in a [PanelFocus]. A click anywhere in
// the panel brings focus into it (unless something inside already took
// it), and a row asks [PanelFocus.of] which colour to paint.
import 'dart:async';

import 'package:flutter/material.dart';

class PanelFocus extends StatefulWidget {
  const PanelFocus({super.key, required this.child, this.onChanged, this.debugLabel});

  final Widget child;

  /// Called with true when focus enters the panel and false when it leaves.
  final ValueChanged<bool>? onChanged;
  final String? debugLabel;

  /// True when focus is somewhere inside the nearest panel. Outside any
  /// panel (a dialog, a widget test that pumps one panel alone) it is true:
  /// there is nothing else the selection could be competing with.
  static bool of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<_PanelFocusScope>()?.focused ?? true;

  @override
  State<PanelFocus> createState() => _PanelFocusState();
}

class _PanelFocusState extends State<PanelFocus> {
  late final _node = FocusNode(debugLabel: widget.debugLabel ?? 'panel', skipTraversal: true);
  bool _focused = false;

  @override
  void dispose() {
    _node.dispose();
    super.dispose();
  }

  void _changed(bool focused) {
    if (focused == _focused) return;
    setState(() => _focused = focused);
    widget.onChanged?.call(focused);
  }

  @override
  Widget build(BuildContext context) {
    return Listener(
      behavior: HitTestBehavior.translucent,
      // A click in the panel that lands on nothing focusable still moves
      // the keyboard here — a row, the empty space under a list.
      // Checked a microtask later: a widget under the pointer that asked
      // for focus on the same event (the Scene view does) has it by then,
      // and the panel does not take it back.
      onPointerDown: (_) => scheduleMicrotask(() {
        if (mounted && !_node.hasFocus) _node.requestFocus();
      }),
      child: Focus(
        focusNode: _node,
        onFocusChange: _changed,
        child: _PanelFocusScope(focused: _focused, child: widget.child),
      ),
    );
  }
}

class _PanelFocusScope extends InheritedWidget {
  const _PanelFocusScope({required this.focused, required super.child});

  final bool focused;

  @override
  bool updateShouldNotify(_PanelFocusScope old) => old.focused != focused;
}
