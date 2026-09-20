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
    _ownedFocus?.dispose();
    super.dispose();
  }

  void _pointer(PointerEvent e, double dpr) {
    widget.onPointerEvent?.call(e, e.localPosition * dpr);
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
          if (mounted) widget.controller.requestSize(w, h);
        });
        return Focus(
          focusNode: _focus,
          autofocus: widget.autofocus,
          onKeyEvent: widget.onKeyEvent,
          onFocusChange: widget.onFocusChange,
          child: Listener(
            behavior: HitTestBehavior.opaque,
            onPointerDown: (e) => _pointer(e, dpr),
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
