// An on-screen stick for the lab's scenes. Flutter's Color and Transform
// here, so only Input and Vec2 come from Fynkel.

import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart' show Input, Vec2;

/// An on-screen stick: drag the knob, and [input]'s 'move' axis follows.
class Stick extends StatefulWidget {
  const Stick(this.input, {super.key});
  final Input input;

  @override
  State<Stick> createState() => _StickState();
}

class _StickState extends State<Stick> {
  static const _size = 120.0, _knob = 48.0;
  var _at = Offset.zero;

  void _drag(Offset local) {
    final from = local - const Offset(_size / 2, _size / 2);
    final reach = (_size - _knob) / 2;
    final at = from.distance > reach ? from / from.distance * reach : from;
    setState(() => _at = at);
    widget.input.setAxis2('move', Vec2(at.dx / reach, -at.dy / reach));
  }

  void _let() {
    setState(() => _at = Offset.zero);
    widget.input.setAxis2('move', Vec2.zero);
  }

  @override
  Widget build(BuildContext context) => GestureDetector(
    onPanStart: (d) => _drag(d.localPosition),
    onPanUpdate: (d) => _drag(d.localPosition),
    onPanEnd: (_) => _let(),
    onPanCancel: _let,
    child: Container(
      width: _size,
      height: _size,
      decoration: const BoxDecoration(color: Color(0x33FFFFFF), shape: BoxShape.circle),
      child: Transform.translate(
        offset: _at,
        child: Center(
          child: Container(
            width: _knob,
            height: _knob,
            decoration: const BoxDecoration(color: Color(0xAAFFFFFF), shape: BoxShape.circle),
          ),
        ),
      ),
    ),
  );
}
