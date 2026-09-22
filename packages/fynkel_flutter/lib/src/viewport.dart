// The viewport widget: lays out a `Texture` sized from the constraints ×
// device pixel ratio, forwards focus/keys/pointers, and shows a placeholder
// until the ring is attached.

import 'dart:async';

import 'package:flutter/material.dart';

import 'viewport_controller.dart';

/// Pointer event plus the position in the viewport's physical pixels
/// (origin top-left, y down) — what a picking or camera-drag behaviour wants.
typedef ViewportPointerCallback = void Function(PointerEvent event, Offset physical);

class FynkelViewport extends StatefulWidget {
  const FynkelViewport({
    super.key,
    required this.controller,
    this.focusNode,
    this.autofocus = false,
    this.onKeyEvent,
    this.onPointerEvent,
    this.onFocusChange,
    this.filterQuality = FilterQuality.none,
    this.placeholder,
    this.resizeDebounce = Duration.zero,
  });

  final FynkelViewportController controller;
  final FocusNode? focusNode;
  final bool autofocus;
  final FocusOnKeyEventCallback? onKeyEvent;
  final ViewportPointerCallback? onPointerEvent;

  /// The viewport gained or lost keyboard focus (a focus loss releases
  /// every held key in the game view's input capture).
  final ValueChanged<bool>? onFocusChange;

  /// The ring is rendered at the widget's physical size, so nearest is the
  /// correct default; a resize in flight shows the old size scaled.
  final FilterQuality filterQuality;

  /// Shown until the texture is registered (default: the scaffold's
  /// background color) and, with the error, when creation failed.
  final Widget? placeholder;

  /// How long a new size must hold before the ring is resized. Zero (the
  /// default, and what a game wants) resizes as the constraints change.
  ///
  /// A window drag changes the constraints on every frame, and every
  /// resize reallocates the ring's surfaces and rebinds the texture —
  /// nothing renders while that is in flight, so the picture blinks. A
  /// short debounce holds the last good frame, scaled, until the drag
  /// settles, and then resizes once. The first size is never held: there
  /// is nothing on screen to scale yet.
  final Duration resizeDebounce;

  @override
  State<FynkelViewport> createState() => _FynkelViewportState();
}

class _FynkelViewportState extends State<FynkelViewport> {
  FocusNode? _ownedFocus;
  FocusNode get _focus => widget.focusNode ?? (_ownedFocus ??= FocusNode(debugLabel: 'FynkelViewport'));

  @override
  void didUpdateWidget(FynkelViewport oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.focusNode != widget.focusNode && widget.focusNode != null) {
      _ownedFocus?.dispose();
      _ownedFocus = null;
    }
  }

  @override
  void dispose() {
    _resize?.cancel();
    _ownedFocus?.dispose();
    super.dispose();
  }

  Timer? _resize;
  (int, int)? _wanted;

  /// The size the constraints ask for, held for [FynkelViewport.resizeDebounce]
  /// while it keeps changing (a window drag).
  void _wantSize(int w, int h) {
    final controller = widget.controller;
    if (widget.resizeDebounce == Duration.zero || !controller.isAttached) {
      _resize?.cancel();
      _resize = null;
      controller.requestSize(w, h);
      return;
    }
    if (w == controller.physicalWidth && h == controller.physicalHeight) {
      // Back to the size it already has: whatever was queued is moot.
      _resize?.cancel();
      _resize = null;
      _wanted = null;
      return;
    }
    _wanted = (w, h);
    _resize?.cancel();
    _resize = Timer(widget.resizeDebounce, () {
      _resize = null;
      final size = _wanted;
      _wanted = null;
      if (size != null && mounted) controller.requestSize(size.$1, size.$2);
    });
  }

  void _pointer(PointerEvent e, double dpr) {
    widget.onPointerEvent?.call(e, e.localPosition * dpr);
  }

  /// A click on the viewport claims the keyboard, like any game window:
  /// whatever took focus away (another widget, a window switch) hands it
  /// back on the next click instead of leaving every key unhandled — on
  /// macOS an unhandled key is the system beep.
  void _pointerDown(PointerDownEvent e, double dpr) {
    if (!_focus.hasFocus) _focus.requestFocus();
    _pointer(e, dpr);
  }

  @override
  Widget build(BuildContext context) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return LayoutBuilder(
      builder: (context, constraints) {
        final w = (constraints.maxWidth * dpr).round();
        final h = (constraints.maxHeight * dpr).round();
        // Layout may run during a build; the controller notifies listeners
        // when it attaches, so defer the request past this frame.
        scheduleMicrotask(() {
          if (mounted) _wantSize(w, h);
        });
        return Focus(
          focusNode: _focus,
          autofocus: widget.autofocus,
          onKeyEvent: widget.onKeyEvent,
          onFocusChange: widget.onFocusChange,
          child: Listener(
            behavior: HitTestBehavior.opaque,
            onPointerDown: (e) => _pointerDown(e, dpr),
            onPointerMove: (e) => _pointer(e, dpr),
            onPointerUp: (e) => _pointer(e, dpr),
            onPointerCancel: (e) => _pointer(e, dpr),
            onPointerHover: (e) => _pointer(e, dpr),
            onPointerSignal: (e) => _pointer(e, dpr),
            child: ListenableBuilder(
              listenable: widget.controller,
              builder: (context, _) {
                final id = widget.controller.textureId;
                if (id == null) {
                  final err = widget.controller.error;
                  return widget.placeholder ??
                      ColoredBox(
                        color: Theme.of(context).scaffoldBackgroundColor,
                        child: err == null
                            ? null
                            : Center(
                                child: Text('Viewport failed: $err', style: Theme.of(context).textTheme.bodySmall),
                              ),
                      );
                }
                return SizedBox.expand(
                  child: Texture(textureId: id, filterQuality: widget.filterQuality),
                );
              },
            ),
          ),
        );
      },
    );
  }
}
