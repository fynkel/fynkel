// The touch overlay (`touch:overlay/<control>`, docs/input.md): a virtual
// stick and buttons drawn over the viewport that feed the same actions the
// keys and gamepad do — an `axis2d` action bound to
// `touch:overlay/stick` reads the stick, a `button` bound to
// `touch:overlay/jump` reads the button. Above the texture, below the HUD.

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:fynkel/fynkel.dart';

import 'input_capture.dart';

/// One overlay control: a stick (an `axis2d` binding) or a button.
sealed class OverlayControl {
  const OverlayControl(this.name);

  /// The control's name after `touch:overlay/`.
  final String name;
}

class OverlayStick extends OverlayControl {
  const OverlayStick(super.name, {this.radius = 56, this.knobRadius = 24});

  /// Logical pixels: the ring the knob moves in, and the knob.
  final double radius;
  final double knobRadius;
}

class OverlayButton extends OverlayControl {
  const OverlayButton(super.name, {this.label, this.radius = 32});

  /// Shown on the button (default: the name).
  final String? label;
  final double radius;
}

/// Virtual controls over the viewport. Sticks sit bottom-left, buttons
/// bottom-right; a pointer down on a control captures it until up. Shown
/// on touch platforms unless [visible] says otherwise.
class TouchOverlay extends StatelessWidget {
  const TouchOverlay({
    super.key,
    required this.input,
    required this.controls,
    this.visible,
    this.margin = const EdgeInsets.all(32),
    this.color = const Color(0x66FFFFFF),
    this.activeColor = const Color(0xAAFFFFFF),
  });

  /// From a load report: every `touch:overlay/<name>` binding of an
  /// `axis2d` action becomes a stick, of a `button` action a button.
  factory TouchOverlay.fromActions(
    List<BoundAction> actions, {
    Key? key,
    required FynkelInputCapture input,
    bool? visible,
  }) {
    final controls = <OverlayControl>[];
    final seen = <String>{};
    for (final a in actions) {
      for (final name in a.overlayControls) {
        if (!seen.add(name)) continue;
        controls.add(a.type == 'axis2d' ? OverlayStick(name) : OverlayButton(name));
      }
    }
    return TouchOverlay(key: key, input: input, controls: controls, visible: visible);
  }

  final FynkelInputCapture input;
  final List<OverlayControl> controls;

  /// Default: iOS and Android only.
  final bool? visible;
  final EdgeInsets margin;
  final Color color;
  final Color activeColor;

  static bool get isTouchPlatform =>
      !kIsWeb && (defaultTargetPlatform == TargetPlatform.iOS || defaultTargetPlatform == TargetPlatform.android);

  @override
  Widget build(BuildContext context) {
    if (!(visible ?? isTouchPlatform) || controls.isEmpty) return const SizedBox.shrink();
    final sticks = controls.whereType<OverlayStick>().toList();
    final buttons = controls.whereType<OverlayButton>().toList();
    return Stack(
      fit: StackFit.expand,
      children: [
        if (sticks.isNotEmpty)
          Positioned(
            left: margin.left,
            bottom: margin.bottom,
            child: Row(
              children: [
                for (final s in sticks)
                  Padding(
                    padding: const EdgeInsets.only(right: 24),
                    child: _Stick(control: s, input: input, color: color, activeColor: activeColor),
                  ),
              ],
            ),
          ),
        if (buttons.isNotEmpty)
          Positioned(
            right: margin.right,
            bottom: margin.bottom,
            child: Row(
              children: [
                for (final b in buttons)
                  Padding(
                    padding: const EdgeInsets.only(left: 16),
                    child: _Button(control: b, input: input, color: color, activeColor: activeColor),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}

class _Stick extends StatefulWidget {
  const _Stick({required this.control, required this.input, required this.color, required this.activeColor});
  final OverlayStick control;
  final FynkelInputCapture input;
  final Color color, activeColor;

  @override
  State<_Stick> createState() => _StickState();
}

class _StickState extends State<_Stick> {
  Offset _knob = Offset.zero; // -1..1 each, y down (screen space)
  int? _pointer;

  String get _name => 'overlay/${widget.control.name}';

  void _set(Offset local) {
    final r = widget.control.radius;
    var v = (local - Offset(r, r)) / r;
    if (v.distance > 1) v = v / v.distance;
    setState(() => _knob = v);
    // The action's y is up.
    widget.input.add(InputEvent.axis(InputDevice.touch, _name, v.dx, -v.dy));
  }

  void _release() {
    if (_pointer == null) return;
    _pointer = null;
    setState(() => _knob = Offset.zero);
    widget.input.add(InputEvent.axis(InputDevice.touch, _name, 0, 0));
  }

  @override
  Widget build(BuildContext context) {
    final r = widget.control.radius;
    final k = widget.control.knobRadius;
    final active = _pointer != null;
    return Listener(
      behavior: HitTestBehavior.opaque,
      onPointerDown: (e) {
        if (_pointer != null) return;
        _pointer = e.pointer;
        _set(e.localPosition);
      },
      onPointerMove: (e) {
        if (e.pointer == _pointer) _set(e.localPosition);
      },
      onPointerUp: (e) {
        if (e.pointer == _pointer) _release();
      },
      onPointerCancel: (e) {
        if (e.pointer == _pointer) _release();
      },
      child: SizedBox(
        width: r * 2,
        height: r * 2,
        child: Stack(
          children: [
            Positioned.fill(
              child: DecoratedBox(
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  border: Border.all(color: active ? widget.activeColor : widget.color, width: 2),
                ),
              ),
            ),
            Positioned(
              left: r - k + _knob.dx * (r - k),
              top: r - k + _knob.dy * (r - k),
              child: Container(
                width: k * 2,
                height: k * 2,
                decoration: BoxDecoration(shape: BoxShape.circle, color: active ? widget.activeColor : widget.color),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Button extends StatefulWidget {
  const _Button({required this.control, required this.input, required this.color, required this.activeColor});
  final OverlayButton control;
  final FynkelInputCapture input;
  final Color color, activeColor;

  @override
  State<_Button> createState() => _ButtonState();
}

class _ButtonState extends State<_Button> {
  int? _pointer;

  String get _name => 'overlay/${widget.control.name}';

  void _up() {
    if (_pointer == null) return;
    setState(() => _pointer = null);
    widget.input.add(InputEvent.up(InputDevice.touch, _name));
  }

  @override
  Widget build(BuildContext context) {
    final r = widget.control.radius;
    final down = _pointer != null;
    return Listener(
      behavior: HitTestBehavior.opaque,
      onPointerDown: (e) {
        if (_pointer != null) return;
        setState(() => _pointer = e.pointer);
        widget.input.add(InputEvent.down(InputDevice.touch, _name));
      },
      onPointerUp: (e) {
        if (e.pointer == _pointer) _up();
      },
      onPointerCancel: (e) {
        if (e.pointer == _pointer) _up();
      },
      child: Container(
        width: r * 2,
        height: r * 2,
        alignment: Alignment.center,
        decoration: BoxDecoration(shape: BoxShape.circle, color: down ? widget.activeColor : widget.color),
        child: Text(
          widget.control.label ?? widget.control.name,
          style: const TextStyle(color: Color(0xDD000000), fontWeight: FontWeight.w600),
        ),
      ),
    );
  }
}
