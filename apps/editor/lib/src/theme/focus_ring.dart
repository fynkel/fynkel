// The keyboard focus ring: one accent outline around whatever holds focus,
// drawn by a layer over the whole app, so every focusable thing — a Material
// button, a docking tab, a Hierarchy row, a menu item, a dialog's control —
// shows it without each widget painting its own.
//
// It shows the way a browser's `:focus-visible` does: on a widget that focus
// reached by the keyboard (Tab, the arrows, a shortcut that moves focus), not
// on one that was clicked — and a key pressed where the mouse put focus (W in
// the Scene view, Delete in the Hierarchy) does not bring it up either. It
// appears and moves without animation. Text fields are skipped: their own
// accent border already says where the caret is. The ring is clipped to the
// scroll views around the focused widget, so a row scrolled half out of its
// panel is not outlined over the panel next to it.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';

import 'tokens.dart';

/// Keyboard-driven or not: true after a key press, false after a pointer
/// goes down anywhere. Read when focus moves, which Flutter applies in a
/// microtask after the key event that caused it.
final class FocusVisibility extends ChangeNotifier {
  FocusVisibility() {
    HardwareKeyboard.instance.addHandler(_onKey);
    GestureBinding.instance.pointerRouter.addGlobalRoute(_onPointer);
  }

  bool _keyboard = false;

  bool get keyboard => _keyboard;

  bool _onKey(KeyEvent event) {
    if (event is KeyDownEvent && !_modifiers.contains(event.logicalKey)) _set(true);
    return false; // observe, never consume
  }

  void _onPointer(PointerEvent event) {
    if (event is PointerDownEvent) _set(false);
  }

  void _set(bool v) {
    if (_keyboard == v) return;
    _keyboard = v;
    notifyListeners();
  }

  // A modifier alone is the start of a shortcut or a click, not navigation.
  static final _modifiers = {
    LogicalKeyboardKey.shiftLeft,
    LogicalKeyboardKey.shiftRight,
    LogicalKeyboardKey.controlLeft,
    LogicalKeyboardKey.controlRight,
    LogicalKeyboardKey.altLeft,
    LogicalKeyboardKey.altRight,
    LogicalKeyboardKey.metaLeft,
    LogicalKeyboardKey.metaRight,
    LogicalKeyboardKey.capsLock,
    LogicalKeyboardKey.fn,
  };

  @override
  void dispose() {
    HardwareKeyboard.instance.removeHandler(_onKey);
    GestureBinding.instance.pointerRouter.removeGlobalRoute(_onPointer);
    super.dispose();
  }
}

/// Paints the ring over [child]. Put it above the Navigator
/// (`MaterialApp.builder`) so dialogs and menus get it too.
class FocusRingLayer extends StatefulWidget {
  const FocusRingLayer({super.key, required this.child});

  final Widget child;

  @override
  State<FocusRingLayer> createState() => FocusRingLayerState();
}

class FocusRingLayerState extends State<FocusRingLayer> {
  final _visibility = FocusVisibility();
  final _self = GlobalKey();

  /// Where the ring is, in this layer's coordinates; null when none shows.
  Rect? get ring => _ring;
  Rect? _ring;
  bool _scheduled = false;

  /// The node focus last reached by the keyboard; the ring shows while it is
  /// still the primary focus.
  FocusNode? _visible;

  @override
  void initState() {
    super.initState();
    _visibility.addListener(_onVisibility);
    FocusManager.instance.addListener(_onFocus);
    _schedule();
  }

  @override
  void dispose() {
    FocusManager.instance.removeListener(_onFocus);
    _visibility.dispose();
    super.dispose();
  }

  void _onFocus() {
    _visible = _visibility.keyboard ? FocusManager.instance.primaryFocus : null;
    _changed();
  }

  void _onVisibility() {
    if (!_visibility.keyboard) _visible = null;
    _changed();
  }

  void _changed() {
    _schedule();
    SchedulerBinding.instance.ensureVisualUpdate();
  }

  // Measured after layout on every frame the app draws, so a focused row that
  // scrolls, a panel that resizes or a tab that moves takes its ring along. A
  // frame that changes nothing measures the same rect and schedules nothing.
  void _schedule() {
    if (_scheduled) return;
    _scheduled = true;
    SchedulerBinding.instance.addPostFrameCallback((_) {
      _scheduled = false;
      if (!mounted) return;
      final next = _measure();
      if (next != _ring) setState(() => _ring = next);
      _schedule();
    });
  }

  Rect? _measure() {
    final node = FocusManager.instance.primaryFocus;
    if (node == null || node != _visible || node is FocusScopeNode) return null;
    final ctx = node.context;
    if (ctx == null || !ctx.mounted) return null;
    if (ctx.findAncestorStateOfType<EditableTextState>() != null) return null;
    final self = _self.currentContext?.findRenderObject();
    final box = ctx.findRenderObject();
    if (self is! RenderBox || box is! RenderBox || !box.attached || !box.hasSize) return null;
    var rect = MatrixUtils.transformRect(box.getTransformTo(self), Offset.zero & box.size);
    // Clipped to every scroll view around it.
    for (var s = Scrollable.maybeOf(ctx); s != null; s = Scrollable.maybeOf(s.context)) {
      final viewport = s.context.findRenderObject();
      if (viewport is! RenderBox || !viewport.hasSize) continue;
      rect = rect.intersect(MatrixUtils.transformRect(viewport.getTransformTo(self), Offset.zero & viewport.size));
      if (rect.width <= 0 || rect.height <= 0) return null;
    }
    return rect;
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    return CustomPaint(key: _self, foregroundPainter: _RingPainter(_ring, t.accent, t.radius), child: widget.child);
  }
}

class _RingPainter extends CustomPainter {
  const _RingPainter(this.rect, this.color, this.radius);

  final Rect? rect;
  final Color color;
  final double radius;

  @override
  void paint(Canvas canvas, Size size) {
    final r = rect;
    if (r == null) return;
    // Just inside the widget's own bounds: panels sit edge to edge with 4 px
    // gutters, so a ring drawn outside would land on the neighbour.
    final paint = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1.5
      ..color = color;
    canvas.drawRRect(RRect.fromRectAndRadius(r.deflate(0.75), Radius.circular(radius)), paint);
  }

  @override
  bool shouldRepaint(_RingPainter old) => old.rect != rect || old.color != color || old.radius != radius;
}
