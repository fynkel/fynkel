import 'package:flutter/gestures.dart' show DragStartBehavior;
import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show Input, Vec2;

/// An on-screen stick, Unity's `OnScreenStick`: drag its knob and
/// [input]'s [axis] follows, -1 to 1 each way, up the screen being +y;
/// let go and it's zero. `Stick(game.world.input)`, placed where it goes
/// (`Positioned(left: 24, bottom: 24, child: ...)`), walks a player that
/// reads `input.axis2('move')`; the keys' and a pad's move it too.
///
/// [floating], it isn't drawn till a thumb comes down anywhere in its
/// area, and appears under it (Unity's dynamic origin): give it the area
/// (the screen's bottom half, say) rather than a spot. A tap there without
/// a drag still reaches what's under it, a game's object or a button.
class Stick extends StatefulWidget {
  /// A stick that sets [input]'s [axis]: 'move' unless given.
  const Stick(this.input, {super.key, this.axis = 'move', this.floating = false, this.size = 120});

  /// The input it sets: a game's is `world.input`.
  final Input input;

  /// The axis it sets, by name ([Input.setAxis2]).
  final String axis;

  /// Whether it appears where a thumb comes down in its area, rather than
  /// staying in one place.
  final bool floating;

  /// Across its ring, in logical pixels; the knob is 13/30 of it.
  final double size;

  @override
  State<Stick> createState() => _StickState();
}

class _StickState extends State<Stick> {
  Offset? _origin;
  var _at = Offset.zero;

  double get _knob => widget.size * 13 / 30;
  double get _reach => (widget.size - _knob) / 2;

  void _start(Offset local) {
    setState(() {
      _origin = widget.floating ? local : Offset(widget.size / 2, widget.size / 2);
      _at = Offset.zero;
    });
    if (!widget.floating) _drag(local);
  }

  void _drag(Offset local) {
    final origin = _origin;
    if (origin == null) return;
    var from = local - origin;
    // Past the rim: the knob stays on it, pointing the thumb's way.
    if (from.distance > _reach) from = from / from.distance * _reach;
    setState(() => _at = from);
    widget.input.setAxis2(widget.axis, Vec2(from.dx / _reach, -from.dy / _reach));
  }

  void _let() {
    if (_origin == null) return;
    setState(() {
      _origin = null;
      _at = Offset.zero;
    });
    widget.input.setAxis2(widget.axis, Vec2.zero);
  }

  @override
  void didUpdateWidget(Stick old) {
    super.didUpdateWidget(old);
    if ((old.input != widget.input || old.axis != widget.axis) && _origin != null) {
      old.input.setAxis2(old.axis, Vec2.zero);
      _origin = null;
      _at = Offset.zero;
    }
  }

  @override
  void dispose() {
    if (_origin != null) widget.input.setAxis2(widget.axis, Vec2.zero);
    super.dispose();
  }

  Widget _ring() => IgnorePointer(
    child: Container(
      key: const ValueKey('fynkel.stick'),
      width: widget.size,
      height: widget.size,
      decoration: BoxDecoration(
        color: const Color(0x332B2D42),
        shape: BoxShape.circle,
        border: Border.all(color: const Color(0x99FFFFFF), width: 2),
      ),
      child: Transform.translate(
        offset: _at,
        child: Center(
          child: Container(
            width: _knob,
            height: _knob,
            decoration: const BoxDecoration(
              color: Color(0xFFFFFFFF),
              shape: BoxShape.circle,
              boxShadow: [BoxShadow(color: Color(0x552B2D42), blurRadius: 8, offset: Offset(0, 3))],
            ),
          ),
        ),
      ),
    ),
  );

  @override
  Widget build(BuildContext context) {
    final origin = _origin;
    return GestureDetector(
      // Only drags: taps go on to what's under it.
      behavior: widget.floating ? HitTestBehavior.translucent : HitTestBehavior.opaque,
      // From where the thumb first came down, not where the drag was
      // noticed a few pixels on.
      dragStartBehavior: DragStartBehavior.down,
      onPanStart: (d) => _start(d.localPosition),
      onPanUpdate: (d) => _drag(d.localPosition),
      onPanEnd: (_) => _let(),
      onPanCancel: _let,
      child: widget.floating
          ? Stack(
              children: [
                const Positioned.fill(child: SizedBox()),
                if (origin != null)
                  Positioned(left: origin.dx - widget.size / 2, top: origin.dy - widget.size / 2, child: _ring()),
              ],
            )
          : SizedBox(width: widget.size, height: widget.size, child: _ring()),
    );
  }
}
