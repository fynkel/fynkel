// The Scene panel (blueprint §19.2e item 6): a toolbar, the viewport over
// the focused tab's edit-preview world, the gizmo and marquee painted on
// top, a stats overlay, and a line for the last notice. Every pointer and
// key goes to the model; the panel paints what the model projected.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../inspector/field_widgets.dart' show AssetDragPayload;
import 'editor_state.dart';
import 'gizmo.dart';
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
    final theme = Theme.of(context);
    final tab = model.current;
    if (tab == null) {
      return _sign(
        theme,
        Icons.view_in_ar_outlined,
        'No document open',
        'Open a scene or prefab (⇧⌘O) to see it here.',
      );
    }
    if (!tab.hasPreview) {
      return Column(
        children: [
          _toolbar(context, theme, tab),
          const Divider(height: 1),
          Expanded(
            child: _sign(
              theme,
              Icons.visibility_off_outlined,
              tab.tab.kind == 'material' ? 'A material has no world' : 'No preview',
              tab.openError ?? 'The preview could not be opened.',
              action: tab.tab.kind == 'material'
                  ? null
                  : TextButton.icon(
                      key: const Key('scene.retry'),
                      onPressed: model.reload,
                      icon: const Icon(Icons.refresh, size: 16),
                      label: const Text('Try again'),
                    ),
            ),
          ),
        ],
      );
    }
    return Column(
      children: [
        _toolbar(context, theme, tab),
        const Divider(height: 1),
        Expanded(child: _view(context, theme, tab)),
        if (model.notice != null) _noticeBar(theme, model.notice!),
        const Divider(height: 1),
        _status(theme, tab),
      ],
    );
  }

  Widget _sign(ThemeData theme, IconData icon, String title, String body, {Widget? action}) => Center(
    child: SingleChildScrollView(
      padding: const EdgeInsets.all(24),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 40, color: theme.disabledColor),
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
    return SizedBox(
      height: 34,
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            const SizedBox(width: 4),
            _toolGroup(theme, live),
            const SizedBox(width: 6),
            _toggle(
              key: 'scene.space',
              icon: model.space == GizmoSpace.local ? Icons.crop_rotate : Icons.public,
              tooltip: model.space == GizmoSpace.local ? 'Local axes (X → world)' : 'World axes (X → local)',
              on: model.space == GizmoSpace.local,
              enabled: live,
              onTap: model.toggleSpace,
            ),
            _toggle(
              key: 'scene.snap',
              icon: Icons.grid_goldenratio,
              tooltip: model.snap.enabled
                  ? 'Snapping on: ${_fmt(model.snap.move)} m, ${_fmt(model.snap.angle)}°, ×${_fmt(model.snap.scale)} (⌥⌘G; hold ⌘ to invert)'
                  : 'Snapping off (⌥⌘G; hold ⌘ to snap)',
              on: model.snap.enabled,
              enabled: live,
              onTap: model.toggleSnap,
              onLongPress: () => _snapDialog(context),
            ),
            const SizedBox(width: 6),
            _toggle(
              key: 'scene.projection',
              icon: cam.orthographic ? Icons.grid_on : Icons.videocam_outlined,
              tooltip: cam.orthographic ? 'Orthographic (5 → perspective)' : 'Perspective (5 → orthographic)',
              on: cam.orthographic,
              enabled: live,
              onTap: model.toggleProjection,
            ),
            _button(
              key: 'scene.frame',
              icon: Icons.center_focus_strong,
              tooltip: 'Frame selected (F) — everything with ⇧F',
              enabled: live,
              onTap: model.frameSelected,
            ),
            _button(
              key: 'scene.alignView',
              icon: Icons.photo_camera_front_outlined,
              tooltip: 'Look through the selected camera (⌥⌘L)',
              enabled: live && model.canAlignToSelectedCamera,
              onTap: model.alignViewToSelectedCamera,
            ),
            const SizedBox(width: 6),
            _toggle(
              key: 'scene.grid',
              icon: Icons.grid_4x4,
              tooltip: 'Grid and axes (⌘G)',
              on: model.showGrid,
              enabled: live,
              onTap: model.toggleGrid,
            ),
            _toggle(
              key: 'scene.overlays',
              icon: Icons.lightbulb_outline,
              tooltip: 'Light and camera icons (⌘I)',
              on: model.showOverlays,
              enabled: live,
              onTap: model.toggleOverlays,
            ),
            _toggle(
              key: 'scene.stats',
              icon: Icons.query_stats,
              tooltip: 'Stats overlay (⌘T)',
              on: model.showStats,
              enabled: live,
              onTap: model.toggleStats,
            ),
            const SizedBox(width: 6),
            _button(
              key: 'scene.hide',
              icon: Icons.visibility_off_outlined,
              tooltip: 'Hide / show selected (H) — editor only, never saved in the scene',
              enabled: live && tab.tab.selection.isNotEmpty,
              onTap: model.toggleHiddenSelection,
            ),
            _button(
              key: 'scene.showAll',
              icon: Icons.visibility_outlined,
              tooltip: 'Show all (⇧H)',
              enabled: live && model.hasHidden,
              onTap: model.showAll,
            ),
            _button(
              key: 'scene.lock',
              icon: Icons.lock_outline,
              tooltip: 'Lock / unlock selected (L) — a locked entity is not picked or dragged here',
              enabled: live && tab.tab.selection.isNotEmpty,
              onTap: model.toggleLockedSelection,
            ),
            const SizedBox(width: 6),
            _button(
              key: 'scene.reload',
              icon: Icons.refresh,
              tooltip: 'Reload the preview world (⌃⌘R)',
              enabled: true,
              onTap: model.reload,
            ),
            const SizedBox(width: 4),
          ],
        ),
      ),
    );
  }

  Widget _toolGroup(ThemeData theme, bool live) {
    const tools = [
      (SceneTool.select, Icons.near_me_outlined, 'Select (Q)'),
      (SceneTool.move, Icons.open_with, 'Move (W)'),
      (SceneTool.rotate, Icons.rotate_right, 'Rotate (E)'),
      (SceneTool.scale, Icons.aspect_ratio, 'Scale (R)'),
    ];
    return Row(
      children: [
        for (final (tool, icon, tip) in tools)
          _toggle(
            key: 'scene.tool.${tool.name}',
            icon: icon,
            tooltip: tip,
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
        borderRadius: BorderRadius.circular(4),
        child: Container(
          width: 28,
          height: 26,
          margin: const EdgeInsets.symmetric(horizontal: 1),
          decoration: BoxDecoration(
            color: on && enabled ? theme.colorScheme.primary.withValues(alpha: 0.18) : null,
            borderRadius: BorderRadius.circular(4),
          ),
          child: Icon(
            icon,
            size: 16,
            color: !enabled
                ? theme.disabledColor
                : on
                ? theme.colorScheme.primary
                : theme.iconTheme.color,
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
    return DragTarget<AssetDragPayload>(
      onWillAcceptWithDetails: (d) => model.canDropAsset(d.data),
      onAcceptWithDetails: (d) {
        // `d.offset` is the pointer: the browsers anchor their drag
        // feedback at the pointer (`pointerDragAnchorStrategy`).
        final box = _pictureKey.currentContext?.findRenderObject() as RenderBox?;
        if (box == null) return;
        final local = box.globalToLocal(d.offset);
        model.dropPrefab(d.data, local * dpr);
      },
      builder: (context, candidates, _) => LayoutBuilder(
        key: _pictureKey,
        builder: (context, constraints) {
          // The layout size stands in for the ring's until the viewport
          // attaches (tests, the first frame).
          final w = constraints.maxWidth * dpr, h = constraints.maxHeight * dpr;
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
                placeholder: ColoredBox(color: theme.colorScheme.surfaceContainerLowest),
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
                  child: ColoredBox(color: theme.colorScheme.surfaceContainerLowest, child: const SizedBox.expand()),
                ),
              ),
            // The gizmo and the marquee: 2D, on top, in logical pixels.
            IgnorePointer(
              child: CustomPaint(
                key: const Key('scene.overlay'),
                painter: _OverlayPainter(
                  gizmo: model.projectedGizmo(),
                  hover: model.hoverHandle,
                  drag: model.drag,
                  marquee: model.marquee,
                  dpr: dpr,
                  focused: model.hasFocus,
                  accent: theme.colorScheme.primary,
                ),
                child: const SizedBox.expand(),
              ),
            ),
            if (model.showStats) Positioned(left: 8, top: 8, child: _stats(theme, tab)),
            if (model.drag?.readout.isNotEmpty ?? false)
              Positioned(right: 8, top: 8, child: _chip(theme, model.drag!.readout, key: const Key('scene.readout'))),
            if (model.hasHidden)
              Positioned(
                right: 8,
                bottom: 8,
                child: _chip(
                  theme,
                  '${tab.state.hidden.length} hidden (⇧H shows all)',
                  key: const Key('scene.hiddenChip'),
                ),
              ),
            if (candidates.isNotEmpty)
              IgnorePointer(
                child: DecoratedBox(
                  decoration: BoxDecoration(border: Border.all(color: theme.colorScheme.primary, width: 2)),
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
      case _:
        break;
    }
  }

  Widget _chip(ThemeData theme, String text, {Key? key}) => Container(
    key: key,
    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
    decoration: BoxDecoration(
      color: theme.colorScheme.surface.withValues(alpha: 0.85),
      borderRadius: BorderRadius.circular(4),
    ),
    child: Text(text, style: theme.textTheme.labelSmall),
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
        color: theme.colorScheme.surface.withValues(alpha: 0.85),
        borderRadius: BorderRadius.circular(4),
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
    width: double.infinity,
    color: theme.colorScheme.surfaceContainerHighest,
    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
    child: Row(
      children: [
        Icon(Icons.info_outline, size: 14, color: theme.colorScheme.onSurfaceVariant),
        const SizedBox(width: 6),
        Expanded(
          child: Text(text, style: theme.textTheme.bodySmall, maxLines: 3, overflow: TextOverflow.ellipsis),
        ),
        InkWell(
          key: const Key('scene.dismissNotice'),
          onTap: () => setState(() => model.notice = null),
          child: const Padding(padding: EdgeInsets.all(2), child: Icon(Icons.close, size: 14)),
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
      model.tool.name +
          (model.tool == SceneTool.select ? '' : ' · ${model.space.name}') +
          (model.snap.enabled ? ' · snap' : ''),
      ?model.noGizmoReason,
    ].join('   ');
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
      child: Row(
        children: [
          Expanded(
            child: Text(
              text,
              key: const Key('scene.status'),
              style: theme.textTheme.labelSmall?.copyWith(color: theme.disabledColor),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          Flexible(
            child: Text(
              '⌥-drag orbit · ⌥⇧-drag / middle pan · ⌥-right dolly · right-drag look + WASDQE · scroll dolly',
              style: theme.textTheme.labelSmall?.copyWith(color: theme.disabledColor),
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }

  static String _fmt2(double v) => v.abs() >= 100 ? v.toStringAsFixed(0) : v.toStringAsFixed(2);
}

/// The gizmo (axes, planes, rings, the centre) and the marquee, from the
/// model's projection, in logical pixels.
class _OverlayPainter extends CustomPainter {
  const _OverlayPainter({
    required this.gizmo,
    required this.hover,
    required this.drag,
    required this.marquee,
    required this.dpr,
    required this.focused,
    required this.accent,
  });

  final ProjectedGizmo? gizmo;
  final GizmoHandle? hover;
  final GizmoDrag? drag;
  final Rect? marquee;
  final double dpr;
  final bool focused;
  final Color accent;

  static const axisColors = [Color(0xFFE5484D), Color(0xFF46A758), Color(0xFF3E63DD)];
  static const highlight = Color(0xFFFFD60A);

  Color _colorOf(GizmoHandle h) {
    final active = drag?.handle ?? hover;
    if (active == h) return highlight;
    if (h.isAxis) return axisColors[h.axisIndex];
    if (h.isPlane) return axisColors[h.planeNormalIndex];
    return const Color(0xFFE0E0E0);
  }

  @override
  void paint(Canvas canvas, Size size) {
    final g = gizmo;
    if (g != null) {
      for (final h in g.handles) {
        if (h.points.length < 2) continue;
        final color = _colorOf(h.handle);
        final path = Path()..moveTo(h.points.first.dx / dpr, h.points.first.dy / dpr);
        for (final p in h.points.skip(1)) {
          path.lineTo(p.dx / dpr, p.dy / dpr);
        }
        if (h.closed) {
          path.close();
          canvas.drawPath(path, Paint()..color = color.withValues(alpha: h.handle == GizmoHandle.center ? 0.35 : 0.3));
          canvas.drawPath(
            path,
            Paint()
              ..color = color
              ..style = PaintingStyle.stroke
              ..strokeWidth = 1.5,
          );
        } else {
          canvas.drawPath(
            path,
            Paint()
              ..color = color
              ..style = PaintingStyle.stroke
              ..strokeWidth = 2.5
              ..strokeCap = StrokeCap.round,
          );
          final tip = h.tip;
          if (tip != null) {
            final t = tip / dpr;
            if (g.tool == SceneTool.scale) {
              canvas.drawRect(Rect.fromCenter(center: t, width: 8, height: 8), Paint()..color = color);
            } else {
              canvas.drawCircle(t, 4.5, Paint()..color = color);
            }
          }
        }
      }
      canvas.drawCircle(g.pivot / dpr, 2.5, Paint()..color = const Color(0xFFFFFFFF));
    }
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
