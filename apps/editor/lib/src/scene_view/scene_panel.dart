// The Scene panel (blueprint §19.2e item 6): the viewport over the focused
// tab's edit-preview world, the gizmo and marquee painted on top, the
// toolbar floating over the picture in translucent groups (item 9b) with the
// stats overlay under it and the orientation gizmo (item 9e) at its right,
// and at the bottom the last notice and a readout of
// the camera and tool — the picture takes the panel's full height. Every pointer and
// key goes to the model; the panel paints what the model projected.
import 'dart:ui' show ImageFilter;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../inspector/field_widgets.dart' show AssetDragPayload;
import '../theme/icons.dart';
import '../theme/panel_focus.dart';
import 'editor_state.dart';
import '../theme/tokens.dart';
import 'orientation_gizmo.dart';
import 'scene_view_model.dart';

class ScenePanel extends StatefulWidget {
  const ScenePanel({super.key, required this.model, this.showViewport = true});

  final SceneViewModel model;

  /// Tests without a native texture leave the viewport out and drive the
  /// pointer through the painted layer instead.
  final bool showViewport;

  @override
  State<ScenePanel> createState() => _ScenePanelState();
}

class _ScenePanelState extends State<ScenePanel> {
  SceneViewModel get model => widget.model;

  @override
  void initState() {
    super.initState();
    model.addListener(_changed);
  }

  @override
  void didUpdateWidget(ScenePanel old) {
    super.didUpdateWidget(old);
    if (old.model != model) {
      old.model.removeListener(_changed);
      model.addListener(_changed);
    }
  }

  @override
  void dispose() {
    model.removeListener(_changed);
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    PanelFocus.primary(context, model.focusNode);
    final theme = Theme.of(context);
    final tab = model.current;
    if (tab == null) {
      return _sign(theme, FIcons.viewInArOutlined, 'No document open', 'Open a scene or prefab (⇧⌘O) to see it here.');
    }
    if (!tab.hasPreview) {
      return Stack(
        children: [
          Positioned.fill(
            child: _sign(
              theme,
              FIcons.visibilityOffOutlined,
              tab.tab.kind == 'material' ? 'A material has no world' : 'No preview',
              tab.openError ?? 'The preview could not be opened.',
              action: tab.tab.kind == 'material'
                  ? null
                  : TextButton.icon(
                      key: const Key('scene.retry'),
                      onPressed: model.reload,
                      icon: const Icon(FIcons.refresh),
                      label: const Text('Try again'),
                    ),
            ),
          ),
          Positioned(left: _inset, top: _inset, right: _inset, child: _toolbar(context, theme, tab)),
        ],
      );
    }
    return Column(
      children: [
        Expanded(
          child: Stack(
            children: [
              Positioned.fill(child: _view(context, theme, tab)),
              // Over the picture: the toolbar, then what reads the picture.
              // Only the groups and chips take the pointer; the space
              // between them is the viewport's.
              Positioned(
                left: _inset,
                top: _inset,
                right: _inset,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    _toolbar(context, theme, tab),
                    const SizedBox(height: 6),
                    Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        if (model.showStats) Flexible(child: _stats(theme, tab)),
                        const Spacer(),
                        if (model.drag?.readout.isNotEmpty ?? false) ...[
                          _chip(theme, model.drag!.readout, key: const Key('scene.readout')),
                          const SizedBox(width: 6),
                        ],
                        _orientation(context, tab),
                      ],
                    ),
                  ],
                ),
              ),
              Positioned(
                left: _inset,
                right: _inset,
                bottom: _inset,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (model.notice != null) ...[_noticeBar(theme, model.notice!), const SizedBox(height: 6)],
                    Row(
                      crossAxisAlignment: CrossAxisAlignment.end,
                      children: [
                        Flexible(child: _status(theme, tab)),
                        const SizedBox(width: 6),
                        if (model.hasHidden)
                          _chip(
                            theme,
                            '${tab.state.hidden.length} hidden (⇧H shows all)',
                            key: const Key('scene.hiddenChip'),
                          ),
                      ],
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  /// The floating toolbar's distance from the picture's edges.
  static const _inset = 8.0;

  Widget _sign(ThemeData theme, IconData icon, String title, String body, {Widget? action}) => Center(
    child: SingleChildScrollView(
      padding: const EdgeInsets.all(24),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 32, color: theme.tokens.textDisabled),
          const SizedBox(height: 12),
          Text(title, style: theme.textTheme.titleMedium),
          const SizedBox(height: 4),
          Text(
            body,
            key: const Key('scene.sign'),
            textAlign: TextAlign.center,
            style: theme.textTheme.bodySmall?.copyWith(color: theme.disabledColor),
          ),
          if (action != null) ...[const SizedBox(height: 8), action],
        ],
      ),
    ),
  );

  // ── Toolbar ─────────────────────────────────────────────────────────

  Widget _toolbar(BuildContext context, ThemeData theme, SceneTab tab) {
    final live = tab.hasPreview;
    final cam = tab.camera;
    // Two sides: the tools and their space at the left; the camera, what
    // the picture shows, what is hidden or locked and the reload at the
    // right. A narrow panel wraps the right side under the left instead of
    // hiding any of it.
    return Wrap(
      key: const Key('scene.toolbar'),
      alignment: WrapAlignment.spaceBetween,
      spacing: 6,
      runSpacing: 6,
      children: [
        _pill(theme, [
          _toolGroup(theme, live),
          _pillGap(theme),
          _toggle(
            key: 'scene.space',
            icon: model.space == GizmoSpace.local ? FIcons.cropRotate : FIcons.public,
            tooltip: model.space == GizmoSpace.local ? 'Local axes (X → world)' : 'World axes (X → local)',
            on: model.space == GizmoSpace.local,
            enabled: live,
            onTap: model.toggleSpace,
          ),
          _toggle(
            key: 'scene.snap',
            icon: FIcons.gridGoldenratio,
            tooltip: model.snap.enabled
                ? 'Snapping on: ${_fmt(model.snap.move)} m, ${_fmt(model.snap.angle)}°, ×${_fmt(model.snap.scale)} (⌥⌘G; hold ⌘ to invert)'
                : 'Snapping off (⌥⌘G; hold ⌘ to snap)',
            on: model.snap.enabled,
            enabled: live,
            onTap: model.toggleSnap,
            onLongPress: () => _snapDialog(context),
          ),
        ]),
        Wrap(
          spacing: 6,
          runSpacing: 6,
          children: [
            _pill(theme, [
              _toggle(
                key: 'scene.projection',
                icon: cam.orthographic ? FIcons.gridOn : FIcons.videocamOutlined,
                tooltip: cam.orthographic ? 'Orthographic (5 → perspective)' : 'Perspective (5 → orthographic)',
                on: cam.orthographic,
                enabled: live,
                onTap: model.toggleProjection,
              ),
              _button(
                key: 'scene.frame',
                icon: FIcons.centerFocusStrong,
                tooltip: 'Frame selected (F) — everything with ⇧F',
                enabled: live,
                onTap: model.frameSelected,
              ),
              _button(
                key: 'scene.alignView',
                icon: FIcons.photoCameraFrontOutlined,
                tooltip: 'Look through the selected camera (⌥⌘L)',
                enabled: live && model.canAlignToSelectedCamera,
                onTap: model.alignViewToSelectedCamera,
              ),
              _pillGap(theme),
              _toggle(
                key: 'scene.grid',
                icon: FIcons.grid4x4,
                tooltip: 'Grid and axes (⌘G)',
                on: model.showGrid,
                enabled: live,
                onTap: model.toggleGrid,
              ),
              _toggle(
                key: 'scene.overlays',
                icon: FIcons.lightbulbOutline,
                tooltip: 'Light and camera icons (⌘I)',
                on: model.showOverlays,
                enabled: live,
                onTap: model.toggleOverlays,
              ),
              _toggle(
                key: 'scene.stats',
                icon: FIcons.queryStats,
                tooltip: 'Stats overlay (⌘T)',
                on: model.showStats,
                enabled: live,
                onTap: model.toggleStats,
              ),
            ]),
            _pill(theme, [
              _button(
                key: 'scene.hide',
                icon: FIcons.visibilityOffOutlined,
                tooltip: 'Hide / show selected (H) — editor only, never saved in the scene',
                enabled: live && tab.tab.selection.isNotEmpty,
                onTap: model.toggleHiddenSelection,
              ),
              _button(
                key: 'scene.showAll',
                icon: FIcons.visibilityOutlined,
                tooltip: 'Show all (⇧H)',
                enabled: live && model.hasHidden,
                onTap: model.showAll,
              ),
              _button(
                key: 'scene.lock',
                icon: FIcons.lockOutline,
                tooltip: 'Lock / unlock selected (L) — a locked entity is not picked or dragged here',
                enabled: live && tab.tab.selection.isNotEmpty,
                onTap: model.toggleLockedSelection,
              ),
              _pillGap(theme),
              _button(
                key: 'scene.reload',
                icon: FIcons.refresh,
                tooltip: 'Reload the preview world (⌃⌘R)',
                enabled: true,
                onTap: model.reload,
              ),
            ]),
          ],
        ),
      ],
    );
  }

  /// The orientation gizmo: its drags orbit like the viewport's (in
  /// physical pixels, as the viewport's pointer is).
  Widget _orientation(BuildContext context, SceneTab tab) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    return OrientationGizmo(
      camera: tab.camera,
      onLookFrom: (axis, positive) => model.lookFrom(axis, positive: positive),
      onToggleProjection: model.toggleProjection,
      onOrbit: (d) => model.orbit(d.dx * dpr, d.dy * dpr),
    );
  }

  /// One group of the floating toolbar: frosted over the picture.
  Widget _pill(ThemeData theme, List<Widget> children) {
    final t = theme.tokens;
    final radius = BorderRadius.circular(8);
    return ClipRRect(
      borderRadius: radius,
      child: BackdropFilter(
        filter: ImageFilter.blur(sigmaX: 12, sigmaY: 12),
        child: Container(
          padding: const EdgeInsets.all(2),
          decoration: BoxDecoration(
            color: t.overlay,
            borderRadius: radius,
            border: Border.all(color: t.overlayEdge),
          ),
          child: Row(mainAxisSize: MainAxisSize.min, children: children),
        ),
      ),
    );
  }

  /// A hairline between the kinds of control inside one group.
  Widget _pillGap(ThemeData theme) => Container(
    width: 1,
    height: 14,
    margin: const EdgeInsets.symmetric(horizontal: 3),
    color: theme.tokens.overlayEdge,
  );

  Widget _toolGroup(ThemeData theme, bool live) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (final tool in SceneTools.all)
          _toggle(
            key: 'scene.tool.${tool.id}',
            icon: tool.icon,
            tooltip: tool.key == null ? tool.label : '${tool.label} (${tool.key!.keyLabel})',
            on: model.tool == tool,
            enabled: live,
            onTap: () => model.tool = tool,
          ),
      ],
    );
  }

  Widget _toggle({
    required String key,
    required IconData icon,
    required String tooltip,
    required bool on,
    required bool enabled,
    required VoidCallback onTap,
    VoidCallback? onLongPress,
  }) {
    final theme = Theme.of(context);
    return Tooltip(
      message: tooltip,
      child: InkWell(
        key: Key(key),
        onTap: enabled ? onTap : null,
        onLongPress: enabled ? onLongPress : null,
        borderRadius: BorderRadius.circular(6),
        child: Container(
          width: theme.tokens.row + 4,
          height: theme.tokens.row + 2,
          decoration: BoxDecoration(
            color: on && enabled ? theme.tokens.accent.withValues(alpha: .22) : null,
            borderRadius: BorderRadius.circular(6),
          ),
          child: Icon(
            icon,
            size: 14,
            color: !enabled
                ? theme.tokens.textDisabled
                : on
                ? theme.tokens.accent
                : theme.tokens.textSecondary,
          ),
        ),
      ),
    );
  }

  Widget _button({
    required String key,
    required IconData icon,
    required String tooltip,
    required bool enabled,
    required VoidCallback onTap,
  }) => _toggle(key: key, icon: icon, tooltip: tooltip, on: false, enabled: enabled, onTap: onTap);

  Future<void> _snapDialog(BuildContext context) async {
    final s = model.snap;
    final move = TextEditingController(text: _fmt(s.move));
    final angle = TextEditingController(text: _fmt(s.angle));
    final scale = TextEditingController(text: _fmt(s.scale));
    final ok = await showDialog<bool>(
      animationStyle: AnimationStyle.noAnimation,
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Snap steps'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: move,
              decoration: const InputDecoration(labelText: 'Move (m)'),
              autofocus: true,
            ),
            TextField(
              controller: angle,
              decoration: const InputDecoration(labelText: 'Rotate (°)'),
            ),
            TextField(
              controller: scale,
              decoration: const InputDecoration(labelText: 'Scale (factor)'),
            ),
          ],
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Cancel')),
          FilledButton(onPressed: () => Navigator.pop(context, true), child: const Text('OK')),
        ],
      ),
    );
    if (ok == true) {
      double? parse(TextEditingController c) {
        final v = double.tryParse(c.text.trim());
        return v == null || v <= 0 ? null : v;
      }

      model.snap = s.copyWith(move: parse(move), angle: parse(angle), scale: parse(scale));
    }
    move.dispose();
    angle.dispose();
    scale.dispose();
  }

  static String _fmt(double v) => v == v.roundToDouble() ? v.toInt().toString() : v.toString();

  // ── The picture ─────────────────────────────────────────────────────

  /// The picture's own box: drops convert the pointer through it, not
  /// through the panel (whose toolbar would shift them down).
  final _pictureKey = GlobalKey(debugLabel: 'scene.picture');

  Widget _view(BuildContext context, ThemeData theme, SceneTab tab) {
    final dpr = MediaQuery.devicePixelRatioOf(context);
    // `offset` is the pointer: the browsers anchor their drag feedback at
    // the pointer (`pointerDragAnchorStrategy`).
    Offset? px(Offset offset) {
      final box = _pictureKey.currentContext?.findRenderObject() as RenderBox?;
      return box == null ? null : box.globalToLocal(offset) * dpr;
    }

    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) => model.canDropAsset(d.data),
      onMove: (d) {
        if (d.data.kind == 'material') model.hoverMaterial(d.data, px(d.offset));
      },
      onLeave: (_) => model.hoverMaterial(null, null),
      onAcceptWithDetails: (d) {
        final at = px(d.offset);
        if (at == null) return;
        if (d.data.kind == 'material') {
          model.dropMaterial(d.data, at);
        } else if (d.data.kind == 'script') {
          model.dropScript(d.data, at);
        } else {
          model.dropPrefab(d.data, at);
        }
      },
      builder: (context, candidates, _) => LayoutBuilder(
        key: _pictureKey,
        builder: (context, constraints) {
          // The layout size stands in for the ring's until the viewport
          // attaches (tests, the first frame).
          final w = constraints.maxWidth * dpr, h = constraints.maxHeight * dpr;
          model.pixelRatio = dpr;
          if (!model.controller.isAttached) {
            WidgetsBinding.instance.addPostFrameCallback((_) => model.setViewportSize(w, h));
          }
          final layers = <Widget>[
            if (widget.showViewport)
              FynkelViewport(
                controller: model.controller,
                focusNode: model.focusNode,
                onKeyEvent: (_, e) => model.handleKey(e),
                onPointerEvent: _pointer,
                placeholder: ColoredBox(color: theme.tokens.window),
                // A window or splitter drag changes the constraints on
                // every frame; resizing the ring that often blinks the
                // picture. The last frame is held, scaled, until the drag
                // settles (a game resizes as it goes: its viewport is the
                // window, and it is drawing every frame anyway).
                resizeDebounce: const Duration(milliseconds: 120),
              )
            else
              Focus(
                focusNode: model.focusNode,
                onKeyEvent: (_, e) => model.handleKey(e),
                child: Listener(
                  behavior: HitTestBehavior.opaque,
                  onPointerDown: (e) {
                    if (!model.focusNode.hasFocus) model.focusNode.requestFocus();
                    _pointer(e, e.localPosition * dpr);
                  },
                  onPointerMove: (e) => _pointer(e, e.localPosition * dpr),
                  onPointerUp: (e) => _pointer(e, e.localPosition * dpr),
                  onPointerCancel: (e) => _pointer(e, e.localPosition * dpr),
                  onPointerHover: (e) => _pointer(e, e.localPosition * dpr),
                  onPointerSignal: (e) => _pointer(e, e.localPosition * dpr),
                  onPointerPanZoomStart: (e) => _pointer(e, e.localPosition * dpr),
                  onPointerPanZoomUpdate: (e) => _pointer(e, e.localPosition * dpr),
                  onPointerPanZoomEnd: (e) => _pointer(e, e.localPosition * dpr),
                  child: ColoredBox(color: theme.tokens.window, child: const SizedBox.expand()),
                ),
              ),
            // The marquee and the focus ring (the handles are the
            // renderer's overlay, drawn in the picture).
            IgnorePointer(
              child: CustomPaint(
                key: const Key('scene.overlay'),
                painter: _OverlayPainter(
                  marquee: model.marquee,
                  dpr: dpr,
                  focused: model.hasFocus,
                  accent: theme.tokens.accent,
                ),
                child: const SizedBox.expand(),
              ),
            ),
            if (candidates.isNotEmpty)
              IgnorePointer(
                child: DecoratedBox(
                  decoration: BoxDecoration(border: Border.all(color: theme.tokens.accent, width: 2)),
                  child: const SizedBox.expand(),
                ),
              ),
          ];
          return MouseRegion(
            cursor: _cursor(),
            child: Stack(fit: StackFit.expand, children: layers),
          );
        },
      ),
    );
  }

  MouseCursor _cursor() {
    if (model.flying) return SystemMouseCursors.move;
    if (model.drag != null || model.hoverHandle != null) return SystemMouseCursors.grab;
    return SystemMouseCursors.basic;
  }

  void _pointer(PointerEvent e, Offset px) {
    switch (e) {
      case PointerDownEvent():
        model.pointerDown(e, px);
      case PointerMoveEvent():
        model.pointerMove(e, px);
      case PointerUpEvent():
        model.pointerUp(e, px);
      case PointerCancelEvent():
        model.pointerCancel(e, px);
      case PointerHoverEvent():
        model.pointerHover(e, px);
      case PointerSignalEvent():
        model.pointerSignal(e, px);
      case PointerPanZoomStartEvent():
        model.panZoomStart(e, px);
      case PointerPanZoomUpdateEvent():
        model.panZoomUpdate(e, px);
      case PointerPanZoomEndEvent():
        model.panZoomEnd(e, px);
      case _:
        break;
    }
  }

  Widget _chip(ThemeData theme, String text, {Key? key}) => Container(
    key: key,
    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
    decoration: BoxDecoration(
      color: theme.tokens.overlay,
      border: Border.all(color: theme.tokens.overlayEdge),
      borderRadius: BorderRadius.circular(6),
    ),
    child: Text(text, style: theme.textTheme.labelSmall?.copyWith(fontFeatures: const [FontFeature.tabularFigures()])),
  );

  Widget _stats(ThemeData theme, SceneTab tab) {
    final ps = model.previewStats();
    final vs = model.viewportStats();
    final mono = theme.textTheme.labelSmall?.copyWith(fontFeatures: const [FontFeature.tabularFigures()]);
    final lines = <String>[
      '${model.viewportWidth.toInt()}×${model.viewportHeight.toInt()}  frame ${model.lastFrameMs.toStringAsFixed(1)} ms',
      if (ps != null)
        'entities ${ps.entities} (live ${ps.live})  meshes ${ps.meshes}  lights ${ps.lights}  hidden ${ps.hidden}',
      if (ps != null) 'tick ${ps.tick}  loads ${ps.loads}  syncs ${ps.syncs}  fallback reloads ${tab.fallbackReloads}',
      if (vs != null)
        'draws ${vs.instances}  cpu ${vs.cpuP50Ms.toStringAsFixed(2)} ms  gpu ${vs.gpuP50Ms.toStringAsFixed(2)} ms  frames ${tab.frames}',
      if (vs != null && (vs.placeholders > 0 || vs.missing > 0 || vs.assetErrors > 0))
        'placeholders ${vs.placeholders}  missing ${vs.missing}  asset errors ${vs.assetErrors}',
    ];
    return Container(
      key: const Key('scene.statsBox'),
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
      decoration: BoxDecoration(
        color: theme.tokens.overlay,
        border: Border.all(color: theme.tokens.overlayEdge),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [for (final l in lines) Text(l, style: mono)],
      ),
    );
  }

  Widget _noticeBar(ThemeData theme, String text) => Container(
    key: const Key('scene.notice'),
    constraints: const BoxConstraints(maxWidth: 560),
    decoration: BoxDecoration(
      color: theme.tokens.overlay,
      border: Border.all(color: theme.tokens.overlayEdge),
      borderRadius: BorderRadius.circular(8),
    ),
    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
    child: Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(FIcons.infoOutline, size: 13, color: theme.tokens.info),
        const SizedBox(width: 6),
        Flexible(
          child: Text(text, style: theme.textTheme.bodySmall, maxLines: 3, overflow: TextOverflow.ellipsis),
        ),
        const SizedBox(width: 6),
        InkWell(
          key: const Key('scene.dismissNotice'),
          onTap: () => setState(() => model.notice = null),
          child: const Padding(padding: EdgeInsets.all(2), child: Icon(FIcons.close, size: 12)),
        ),
      ],
    ),
  );

  Widget _status(ThemeData theme, SceneTab tab) {
    final cam = tab.camera;
    final sel = tab.tab.selection.length;
    final text = [
      cam.orthographic ? 'ortho ${_fmt2(cam.orthoHeight)} m' : 'persp ${(cam.fovY * 180 / 3.141592653589793).round()}°',
      'pivot (${cam.pivot.map(_fmt2).join(', ')})  d ${_fmt2(cam.distance)}',
      if (sel > 0) '$sel selected',
      if (model.editingLabel case final editing?)
        'editing $editing · Esc to stop'
      else
        model.tool.name +
            (!model.tool.hasGizmo ? '' : ' · ${model.space.name}') +
            (model.snap.enabled ? ' · snap' : ''),
      ?model.noGizmoReason,
    ].join('   ');
    return Tooltip(
      message:
          'Navigate: ⌥-drag orbit · ⌥⇧-drag or middle-drag pan · ⌥-right-drag dolly · '
          'right-drag look + WASDQE · scroll or pinch dolly · ⇧-scroll pan',
      waitDuration: const Duration(milliseconds: 500),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
        decoration: BoxDecoration(
          color: theme.tokens.overlay,
          border: Border.all(color: theme.tokens.overlayEdge),
          borderRadius: BorderRadius.circular(6),
        ),
        child: Text(
          text,
          key: const Key('scene.status'),
          maxLines: 1,
          style: theme.textTheme.labelSmall?.copyWith(
            color: theme.tokens.textSecondary,
            fontFeatures: const [FontFeature.tabularFigures()],
          ),
          overflow: TextOverflow.ellipsis,
        ),
      ),
    );
  }

  static String _fmt2(double v) => v.abs() >= 100 ? v.toStringAsFixed(0) : v.toStringAsFixed(2);
}

/// The marquee and the focus ring, in logical pixels.
class _OverlayPainter extends CustomPainter {
  const _OverlayPainter({required this.marquee, required this.dpr, required this.focused, required this.accent});

  final Rect? marquee;
  final double dpr;
  final bool focused;
  final Color accent;

  @override
  void paint(Canvas canvas, Size size) {
    final m = marquee;
    if (m != null) {
      final r = Rect.fromLTRB(m.left / dpr, m.top / dpr, m.right / dpr, m.bottom / dpr);
      canvas.drawRect(r, Paint()..color = accent.withValues(alpha: 0.15));
      canvas.drawRect(
        r,
        Paint()
          ..color = accent
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1,
      );
    }
    if (focused) {
      canvas.drawRect(
        Offset.zero & size,
        Paint()
          ..color = accent.withValues(alpha: 0.6)
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1,
      );
    }
  }

  @override
  bool shouldRepaint(_OverlayPainter old) => true;
}
