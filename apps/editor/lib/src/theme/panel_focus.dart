// Which panel holds focus (blueprint item 9b d): "the accent follows
// focus". A selected row is drawn in the accent only in the panel the
// keyboard is in; every other panel shows its selection as a quiet tint,
// so there is never more than one blue selection on screen.
//
// The workspace wraps every panel in a [PanelFocus]. A click anywhere in
// the panel brings focus into it (unless something inside already took
// it), and a row asks [PanelFocus.of] which colour to paint.
//
// Window → Go to <Panel> (⌘1…, Unity's keys) moves the keyboard into a
// panel through its [PanelFocusHandle]: to the node the panel named with
// [PanelFocus.primary] (the Hierarchy's list, the Scene view), else its
// first control. Each panel is a [FocusTraversalGroup], so Tab walks one
// panel's controls before the next panel's.
import 'dart:async';

import 'package:flutter/material.dart';

/// Moves the keyboard into one panel from outside it (a command).
final class PanelFocusHandle {
  _PanelFocusState? _state;

  /// Focuses the panel's primary node, else its first control; false when
  /// the panel is not built.
  bool focus() {
    final s = _state;
    if (s == null || !s.mounted) return false;
    s._focusPanel();
    return true;
  }
}

class PanelFocus extends StatefulWidget {
  const PanelFocus({super.key, required this.child, this.onChanged, this.debugLabel, this.handle});

  final Widget child;

  /// Called with true when focus enters the panel and false when it leaves.
  final ValueChanged<bool>? onChanged;
  final String? debugLabel;
  final PanelFocusHandle? handle;

  /// Names [node] as what focusing this panel from the keyboard focuses:
  /// its list or view, not the search field above it. Called from the
  /// panel's build.
  static void primary(BuildContext context, FocusNode node) {
    context.getInheritedWidgetOfExactType<_PanelFocusScope>()?.state._primary = node;
  }

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
  FocusNode? _primary;

  @override
  void initState() {
    super.initState();
    widget.handle?._state = this;
  }

  @override
  void didUpdateWidget(PanelFocus old) {
    super.didUpdateWidget(old);
    if (old.handle != widget.handle) {
      if (old.handle?._state == this) old.handle!._state = null;
      widget.handle?._state = this;
    }
  }

  @override
  void dispose() {
    if (widget.handle?._state == this) widget.handle!._state = null;
    _node.dispose();
    super.dispose();
  }

  void _focusPanel() {
    final primary = _primary;
    if (primary != null && primary.context != null && primary.canRequestFocus) {
      primary.requestFocus();
      return;
    }
    final first = _node.traversalDescendants.where((n) => n.canRequestFocus).firstOrNull;
    (first ?? _node).requestFocus();
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
        child: _PanelFocusScope(
          focused: _focused,
          state: this,
          child: FocusTraversalGroup(child: widget.child),
        ),
      ),
    );
  }
}

class _PanelFocusScope extends InheritedWidget {
  const _PanelFocusScope({required this.focused, required this.state, required super.child});

  final bool focused;
  final _PanelFocusState state;

  @override
  bool updateShouldNotify(_PanelFocusScope old) => old.focused != focused;
}
