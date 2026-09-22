// The Scene view's model (blueprint §19.2e item 6): one edit-preview world
// per open scene or prefab tab, the editor camera over it, picking, the
// gizmos and the editor-only hidden/locked flags. The picture is the
// document — the world never ticks (ADR-0016/0018): the model steers a
// camera, asks the preview what a ray hits, previews a drag's transforms
// in the world and, on release, writes exactly one transaction. The panel
// is a thin skin over this: it forwards pointers and keys and paints what
// the model projected.
//
// Item 9e: the Scene view can be open more than once. The copies draw the
// same preview world of a document, each through its own camera: a frame
// is extracted synchronously when the viewport renders and every viewport
// has its own renderer, so a view sets its camera on the world right
// before its frame whenever another view drew last ([_worldCamera]) — no
// engine change. A change one view makes to a world (a drag's preview
// transforms, the hidden set, a reload) repaints the others ([SceneViews]).
import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart' show FocusNode, KeyEventResult;
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../documents/document_tabs.dart';
import '../documents/editor_document.dart';
import '../documents/open_document.dart';
import '../hierarchy/hierarchy_model.dart';
import '../hierarchy/transform_math.dart';
import '../inspector/field_widgets.dart' show AssetDragPayload;
import '../inspector/prefab_actions.dart' as prefab;
import '../project/editor_project.dart';
import 'editor_state.dart';
import 'gizmo.dart';
import 'png.dart';
import 'scene_math.dart';

/// Which Scene view's camera each preview world holds, and at which
/// version: the copies share a world, so a view sets its own camera again
/// when another one drew last.
final _worldCamera = Expando<(SceneTab, int)>('scene.worldCamera');

/// The Scene view copies of one project (item 9e).
final class SceneViews {
  final _views = <SceneViewModel>{};
  int? _recookedAt;

  /// A view changed [preview]'s world (null: every world): the others
  /// showing it draw again. [reset] when the world was rebuilt, so its
  /// camera and hidden set must be applied again.
  void changed(SceneViewModel from, PreviewBackend? preview, {bool reset = false}) {
    for (final v in List.of(_views)) {
      if (v != from) v._worldChanged(preview, reset: reset);
    }
  }

  /// The first view to see a new assets revision rebuilds the cache for
  /// all of them; true for that one.
  bool claimRecook(int revision) {
    if (_recookedAt == revision) return false;
    _recookedAt = revision;
    return true;
  }
}

/// One tab's Scene view: its preview world and the editor's state for it.
final class SceneTab {
  SceneTab(this.tab, this.state, {this.view = SceneEditorState.firstView});

  final OpenDocument tab;
  final SceneEditorState state;

  /// The Scene view copy this belongs to: its camera is the copy's.
  final String view;

  PreviewBackend? preview;

  /// Why the preview did not open (a material, a cook that failed).
  String? openError;
  StreamSubscription<PreviewSyncReport>? _syncs;

  /// Bumped on every camera change; the preview is told on the next frame.
  int cameraVersion = 1;

  /// The hidden set must be re-applied (a sync respawned entities).
  bool hiddenDirty = true;

  /// The ring slot of the last frame rendered.
  int? lastSlot;

  /// Frames rendered for this tab.
  int frames = 0;

  /// Fallback reloads the session reported.
  int fallbackReloads = 0;

  /// Click cycling: the hits under the last click and which is selected.
  Offset? clickPx;
  List<String> clickHits = const [];
  int clickIndex = 0;

  EditorCamera get camera => state.cameraOf(view);
  bool get hasPreview => preview != null && !preview!.isClosed;

  /// This view's camera is the one the world holds.
  bool get cameraApplied {
    final p = preview;
    return p != null && _worldCamera[p] == (this, cameraVersion);
  }

  /// [closePreview] false for a Scene view copy closing while the document
  /// stays open: the world is the document's, and other views draw it.
  void dispose({bool closePreview = true}) {
    _syncs?.cancel();
    _syncs = null;
    final p = preview;
    if (closePreview && p != null && !p.isClosed) p.close();
    preview = null;
  }
}

/// One selected entity under the gizmo's drag: where it started (local,
/// world, its parent's world) and where the preview has it now.
final class GizmoTarget {
  GizmoTarget({
    required this.id,
    required this.transformId,
    required this.startLocal,
    required this.startWorld,
    required this.parentWorld,
    this.anchor,
    this.entityLocal,
    this.transformLocal,
  });

  final String id;
  final String transformId;

  /// A derived entity of a prefab instance: the anchor its override goes
  /// on, and the prefab-local ids the override addresses.
  final String? anchor;
  final String? entityLocal;
  final String? transformLocal;
  bool get isDerived => anchor != null;
  final Trs startLocal;
  final List<double> startWorld;
  final List<double> parentWorld;
  late Trs current = startLocal;
}

/// A gizmo drag in flight.
final class GizmoDrag {
  GizmoDrag({
    required this.tool,
    required this.handle,
    required this.startPx,
    required this.pivot,
    required this.axes,
    required this.size,
    required this.projected,
    required this.targets,
  });

  final SceneTool tool;
  final GizmoHandle handle;
  final Offset startPx;

  /// The gizmo's frame at the drag's start.
  final Vec3 pivot;
  final List<Vec3> axes;
  final double size;
  final ProjectedGizmo projected;
  final List<GizmoTarget> targets;

  /// The pivot now (moves with the lead).
  Vec3 currentPivot = const [0, 0, 0];

  /// The pointer moved far enough to count (a click on a handle that
  /// never moves changes nothing).
  bool moved = false;

  /// The last delta, for the panel's readout ("Δ 1.50 m", "30°", "×1.2").
  String readout = '';

  Iterable<String> get ids => targets.map((t) => t.id);
}

/// How a pointer button is being dragged.
enum _Mode { none, click, marquee, orbit, pan, dolly, look, gizmo }

/// The Scene view's model over the focused tab.
final class SceneViewModel extends ChangeNotifier {
  SceneViewModel({
    required this.tabs,
    required this.project,
    required this.hierarchy,
    required this.editorState,
    FynkelViewportController? controller,
    this.chooseSavePath,
    this.view = SceneEditorState.firstView,
    this.views,
  }) : controller = controller ?? FynkelViewportController(placeholders: false) {
    views?._views.add(this);
    tabs.addListener(_onTabs);
    project.addListener(_onProject);
    editorState.addListener(notifyListeners);
    this.controller.addListener(_onController);
    focusNode.addListener(_onFocus);
    _assetsRevision = project.assetsRevision;
    _onTabs();
  }

  final DocumentTabs tabs;
  final EditorProject project;
  final HierarchyModel hierarchy;
  final EditorState editorState;

  /// Which Scene view copy this is (item 9e): `scene`, `scene#2`, …
  final String view;

  /// The project's Scene view copies, when there can be more than one.
  final SceneViews? views;

  /// The one native viewport; it renders the focused tab's world.
  final FynkelViewportController controller;

  /// "Save Frame…": where to write; null cancels. The app's is a save
  /// dialog, the tests' a path.
  final Future<String?> Function(String suggestedName)? chooseSavePath;

  final focusNode = FocusNode(debugLabel: 'scene.view');

  final _sceneTabs = <OpenDocument, SceneTab>{};
  OpenDocument? _listening;
  int _assetsRevision = 0;
  bool _frameScheduled = false;
  bool _disposed = false;

  /// Physical pixels of the picture (the viewport's ring size once it is
  /// attached; the panel's layout before).
  double viewportWidth = 0, viewportHeight = 0;

  /// The pointer's last position (physical pixels) for hover and keys.
  Offset? pointer;

  /// The handle under the pointer.
  GizmoHandle? hoverHandle;

  /// The drag in flight.
  GizmoDrag? drag;

  /// The marquee in flight (physical pixels).
  Rect? marquee;

  /// The last message worth a line under the picture (a reload, a save,
  /// a refused drag); null when there is none.
  String? notice;

  /// Wall time of the last frame's render call, milliseconds.
  double lastFrameMs = 0;

  _Mode _mode = _Mode.none;
  Offset? _last;
  Offset? _downPx;
  final _flyKeys = <LogicalKeyboardKey>{};
  Timer? _flyTimer;
  bool _shift = false;

  // ── Tabs ─────────────────────────────────────────────────────────────

  /// The focused tab's Scene view, opening its preview on first use; null
  /// without a focused tab.
  SceneTab? get current {
    final f = tabs.focused;
    if (f == null) return null;
    return _sceneTabOf(f);
  }

  SceneTab _sceneTabOf(OpenDocument tab) {
    final existing = _sceneTabs[tab];
    if (existing != null) {
      if (!existing.hasPreview && existing.openError == null) _openPreview(existing);
      return existing;
    }
    final st = SceneTab(tab, editorState.scene(tab.document.id), view: view);
    _sceneTabs[tab] = st;
    _openPreview(st);
    return st;
  }

  void _openPreview(SceneTab st) {
    if (st.tab.document.isClosed) return;
    try {
      final p = st.tab.document.openPreview();
      st.preview = p;
      st.openError = null;
      st.hiddenDirty = true;
      st._syncs?.cancel();
      st._syncs = p.syncs.listen((r) => _onSync(st, r));
      final cook = p.cook;
      if (cook != null && !cook.ok) {
        notice = 'Cook: ${cook.failed} asset(s) failed — the picture shows what cooked';
      }
      if (p.diagnostics.any((d) => d.severity == Severity.error)) {
        notice = p.diagnostics
            .where((d) => d.severity == Severity.error)
            .map((d) => '${d.code}: ${d.message}')
            .join('\n');
      }
    } on AuthoringException catch (e) {
      st.preview = null;
      st.openError = e.diagnostics.isEmpty
          ? e.toString()
          : e.diagnostics.map((d) => '${d.code}: ${d.message}').join('\n');
    } on StateError catch (e) {
      st.preview = null;
      st.openError = '$e';
    }
  }

  void _onSync(SceneTab st, PreviewSyncReport r) {
    if (r.reloaded) {
      st.fallbackReloads++;
      notice = 'Preview reloaded: the incremental sync could not apply a change (${st.fallbackReloads}×)';
    }
    st.hiddenDirty = true;
    _repaint();
  }

  void _onTabs() {
    // Tabs that closed take their preview with them.
    for (final t in List.of(_sceneTabs.keys)) {
      if (!tabs.open.contains(t)) {
        _sceneTabs.remove(t)!.dispose();
        editorState.flush();
      }
    }
    final f = tabs.focused;
    if (f != _listening) {
      _listening?.removeListener(_onTab);
      _listening = f;
      f?.addListener(_onTab);
      _cancelInteraction();
    }
    _repaint();
  }

  void _onTab() {
    final t = current;
    if (t == null) return;
    final edit = t.tab.document.lastEdit;
    switch (edit?.kind) {
      case DocumentChangeKind.applied ||
          DocumentChangeKind.undone ||
          DocumentChangeKind.redone ||
          DocumentChangeKind.reloaded:
        // The session synced the world: hidden flags must be re-applied
        // (a respawned entity comes back visible), a drag in flight is
        // stale (an undo under it).
        t.hiddenDirty = true;
        if (drag != null && edit!.kind != DocumentChangeKind.applied) cancelDrag();
        t.clickHits = const [];
      case _:
        break;
    }
    _repaint();
  }

  void _onProject() {
    if (project.assetsRevision != _assetsRevision) {
      _assetsRevision = project.assetsRevision;
      // Assets changed under the open previews: the cache is rebuilt so
      // the picture shows the new mesh or texture — once, by the first
      // Scene view copy to hear of it.
      final t = current;
      if (t != null && t.hasPreview && (views?.claimRecook(_assetsRevision) ?? true)) _recook(t, announce: false);
    }
    _repaint();
  }

  /// Another Scene view copy changed [preview]'s world (null: all of them).
  void _worldChanged(PreviewBackend? preview, {required bool reset}) {
    if (_disposed) return;
    if (reset) {
      for (final st in _sceneTabs.values) {
        if (preview == null || st.preview == preview) st.hiddenDirty = true;
      }
    }
    final p = current?.preview;
    if (preview == null || p == preview) _repaint();
  }

  void _onController() => _repaint();

  void _onFocus() => notifyListeners();

  bool get hasFocus => focusNode.hasFocus;

  /// A preview is open for the focused tab.
  bool get hasView => current?.hasPreview ?? false;

  /// The focused tab can be edited and has a preview.
  bool get canEdit => hasView && (current!.tab.document.readOnly == null);

  // ── Toggles (saved in editor.json) ───────────────────────────────────

  SceneTool get tool => editorState.tool;
  set tool(SceneTool t) {
    if (drag != null) cancelDrag();
    editorState.tool = t;
    editorState.changed();
    _repaint();
  }

  GizmoSpace get space => editorState.space;
  void toggleSpace() {
    editorState.space = space == GizmoSpace.world ? GizmoSpace.local : GizmoSpace.world;
    editorState.changed();
    _repaint();
  }

  SnapSettings get snap => editorState.snap;
  set snap(SnapSettings s) {
    editorState.snap = s;
    editorState.changed();
    _repaint();
  }

  void toggleSnap() => snap = snap.copyWith(enabled: !snap.enabled);

  bool get showGrid => editorState.showGrid;
  bool get showOverlays => editorState.showOverlays;
  bool get showStats => editorState.showStats;
  void toggleGrid() {
    editorState.showGrid = !showGrid;
    editorState.changed();
    _repaint();
  }

  void toggleOverlays() {
    editorState.showOverlays = !showOverlays;
    editorState.changed();
    _repaint();
  }

  void toggleStats() {
    editorState.showStats = !showStats;
    editorState.changed();
    _repaint();
  }

  // ── Camera ───────────────────────────────────────────────────────────

  EditorCamera? get camera => current?.camera;

  void _cameraChanged() {
    final t = current;
    if (t == null) return;
    t.cameraVersion++;
    editorState.scheduleSave();
    _repaint();
  }

  ViewProjection? get viewProjection {
    final c = camera;
    if (c == null || viewportWidth <= 0 || viewportHeight <= 0) return null;
    return ViewProjection(c, viewportWidth, viewportHeight);
  }

  double get aspect => viewportHeight > 0 ? viewportWidth / viewportHeight : 16 / 9;

  void orbit(double dx, double dy) {
    camera?.orbit(dx, dy);
    _cameraChanged();
  }

  void pan(double dx, double dy) {
    camera?.pan(dx, dy, math.max(viewportHeight, 1));
    _cameraChanged();
  }

  void dolly(double steps) {
    camera?.dolly(steps);
    _cameraChanged();
  }

  void look(double dx, double dy) {
    camera?.look(dx, dy);
    _cameraChanged();
  }

  void toggleProjection() {
    camera?.toggleProjection();
    _cameraChanged();
  }

  /// The orientation gizmo's axis end (item 9e): look at the pivot from
  /// that side (a second click, from the opposite one).
  void lookFrom(int axis, {required bool positive}) {
    camera?.lookFrom(axis, positive: positive);
    _cameraChanged();
  }

  void resetView() {
    final t = current;
    if (t == null) return;
    t.state.setCamera(view, EditorCamera.initial());
    _cameraChanged();
  }

  /// The world-space bounds of [ids] from their outline lines; null when
  /// nothing has a position.
  (Vec3, Vec3)? boundsOf(Iterable<String> ids) {
    final t = current;
    if (t == null || !t.hasPreview) return null;
    final list = ids.toList();
    if (list.isEmpty) return null;
    final lines = t.preview!.lines(outline: list);
    if (lines.count == 0) return null;
    var min = [double.infinity, double.infinity, double.infinity];
    var max = [-double.infinity, -double.infinity, -double.infinity];
    final d = lines.data;
    for (var i = 0; i + 9 < d.length; i += 10) {
      for (final o in [0, 3]) {
        final p = [d[i + o], d[i + o + 1], d[i + o + 2]];
        min = vMin(min, p);
        max = vMax(max, p);
      }
    }
    return (min, max);
  }

  /// F: frame the selection (everything when nothing is selected).
  void frameSelected() {
    final t = current;
    if (t == null) return;
    final sel = t.tab.selection.where((id) => !t.state.hidden.contains(id)).toList();
    if (sel.isEmpty) {
      frameAll();
      return;
    }
    final b = boundsOf(sel);
    if (b == null) return;
    t.camera.frame(b.$1, b.$2);
    _cameraChanged();
  }

  /// ⇧F: frame every visible entity.
  void frameAll() {
    final t = current;
    if (t == null) return;
    final hidden = _hiddenAll(t);
    final ids = t.tab.tree.nodes.keys.where((id) => !hidden.contains(id));
    final b = boundsOf(ids);
    if (b == null) {
      t.camera.frame([-5, 0, -5], [5, 2, 5]);
    } else {
      t.camera.frame(b.$1, b.$2);
    }
    _cameraChanged();
  }

  /// The lead selected entity's `fynkel.Camera` component data and id.
  (String, Map<String, Object?>)? _selectedCamera(SceneTab t) {
    final lead = t.tab.lead ?? t.tab.selection.lastOrNull;
    if (lead == null) return null;
    final node = t.tab.tree[lead];
    if (node == null || !node.hasComponent('fynkel.Camera')) return null;
    final e = t.tab.document.entities([lead]).entities[lead];
    final comps = e?['components'];
    if (comps is! Map) return null;
    for (final entry in comps.entries) {
      final c = entry.value;
      if (c is Map && c['type'] == 'fynkel.Camera') {
        final data = c['data'];
        return (entry.key as String, data is Map<String, Object?> ? data : <String, Object?>{});
      }
    }
    return null;
  }

  bool get canAlignToSelectedCamera {
    final t = current;
    return t != null && t.hasPreview && _selectedCamera(t) != null;
  }

  /// ⌥⌘L: the view takes the selected camera's exact placement and lens
  /// (projection, field of view, near, far, exposure) — the picture is
  /// then what that camera renders.
  void alignViewToSelectedCamera() {
    final t = current;
    if (t == null || !t.hasPreview) return;
    final cam = _selectedCamera(t);
    if (cam == null) return;
    final lead = t.tab.lead ?? t.tab.selection.last;
    final world = t.preview!.worldMatrices([lead])[lead];
    if (world == null) return;
    final data = cam.$2;
    double? num_(String k) => data[k] is num ? (data[k] as num).toDouble() : null;
    t.camera.alignTo(
      world,
      orthographic: data['projection'] == 'orthographic',
      fovY: num_('fov_y'),
      orthoHeight: num_('ortho_height'),
      near: num_('near'),
      far: num_('far'),
      manualExposure: data['exposure_mode'] == 'manual' ? num_('exposure') : null,
    );
    _cameraChanged();
  }

  /// ⇧⌘L: the selected camera moves to where the view is and takes its
  /// lens — one transaction on its Transform and Camera.
  bool alignSelectedCameraToView() {
    final t = current;
    if (t == null || !t.hasPreview || !canEdit) return false;
    final cam = _selectedCamera(t);
    if (cam == null) return false;
    final lead = t.tab.lead ?? t.tab.selection.last;
    final node = t.tab.tree[lead];
    final transformId = node?.transformId;
    if (node == null || transformId == null) {
      notice = 'The camera entity has no Transform to move.';
      notifyListeners();
      return false;
    }
    final parentWorld = node.parent == null
        ? List.of(identityMatrix)
        : (t.preview!.worldMatrices([node.parent!])[node.parent!] ?? List.of(identityMatrix));
    final local = Trs.fromMatrix(matMul(matInverseAffine(parentWorld), t.camera.world));
    final c = t.camera;
    final data = local.toData();
    final commands = [
      Command.setField(lead, transformId, 'position', data['position']),
      Command.setField(lead, transformId, 'rotation', data['rotation']),
      Command.setField(lead, cam.$1, 'projection', c.orthographic ? 'orthographic' : 'perspective'),
      Command.setField(lead, cam.$1, 'fov_y', Trs.clean(c.fovY)),
      Command.setField(lead, cam.$1, 'ortho_height', Trs.clean(c.orthoHeight)),
      Command.setField(lead, cam.$1, 'near', Trs.clean(c.near)),
      Command.setField(lead, cam.$1, 'far', Trs.clean(c.far)),
    ];
    final a = t.tab.transaction(commands);
    if (a == null || !a.ok) return false;
    // The view now is that camera's view exactly.
    c.alignedWorld = c.world;
    _cameraChanged();
    return true;
  }

  // ── Hidden / locked (editor-only) ────────────────────────────────────

  Set<String> _hiddenAll(SceneTab t) => {for (final id in t.state.hidden) ...t.tab.tree.subtree(id)};

  bool isHidden(String id) => current?.state.hidden.contains(id) ?? false;
  bool isLocked(String id) => current?.state.locked.contains(id) ?? false;

  /// The selection (topmost) toggles between hidden and shown: all
  /// hidden → shown, else hidden.
  void toggleHiddenSelection() {
    final t = current;
    if (t == null || t.tab.selection.isEmpty) return;
    final ids = t.tab.tree.topmost(t.tab.selection);
    final allHidden = ids.every(t.state.hidden.contains);
    if (allHidden) {
      t.state.hidden.removeAll(ids);
    } else {
      t.state.hidden.addAll(ids);
    }
    t.hiddenDirty = true;
    applyHidden();
    editorState.changed();
    _repaint();
  }

  void showAll() {
    final t = current;
    if (t == null || t.state.hidden.isEmpty) return;
    t.state.hidden.clear();
    t.hiddenDirty = true;
    applyHidden();
    editorState.changed();
    _repaint();
  }

  void toggleLockedSelection() {
    final t = current;
    if (t == null || t.tab.selection.isEmpty) return;
    final ids = t.tab.selection.toList();
    final allLocked = ids.every(t.state.locked.contains);
    if (allLocked) {
      t.state.locked.removeAll(ids);
    } else {
      t.state.locked.addAll(ids);
    }
    if (drag != null) cancelDrag();
    editorState.changed();
    _repaint();
  }

  bool get hasHidden => current?.state.hidden.isNotEmpty ?? false;

  /// Pushes the hidden set (with its subtrees) into the focused tab's
  /// world when it changed — at once on a toggle so picking agrees, and
  /// again before a frame after every sync or reload (the world's filter
  /// is applied at set time only).
  void applyHidden() {
    final t = current;
    if (t != null) _applyHidden(t);
  }

  void _applyHidden(SceneTab t) {
    if (!t.hiddenDirty || !t.hasPreview) return;
    // The preview takes the subtrees; the world hides the instances of
    // any anchor among them itself.
    t.preview!.setHidden(_hiddenAll(t));
    t.hiddenDirty = false;
    views?.changed(this, t.preview);
  }

  // ── Reload / recook / save frame ─────────────────────────────────────

  /// ⌃⌘R: the world emptied and loaded fresh from the live document.
  void reload() {
    final t = current;
    if (t == null) return;
    if (!t.hasPreview) {
      _openPreview(t);
      _repaint();
      return;
    }
    if (drag != null) cancelDrag();
    try {
      final stats = t.preview!.reload();
      t.hiddenDirty = true;
      _worldCamera[t.preview!] = null;
      views?.changed(this, t.preview, reset: true);
      notice = 'Preview reloaded: ${stats.entities} entities';
    } on AuthoringException catch (e) {
      notice = 'Reload failed: $e';
    }
    _repaint();
  }

  /// ⌃⇧⌘R: `fynkel cook` in process, the cache reopened, every preview
  /// reloaded.
  void recook() {
    final t = current;
    if (t == null || !t.hasPreview) return;
    _recook(t, announce: true);
    _repaint();
  }

  void _recook(SceneTab t, {required bool announce}) {
    if (drag != null) cancelDrag();
    try {
      final r = t.preview!.recook();
      for (final st in _sceneTabs.values) {
        st.hiddenDirty = true;
        if (st.preview case final p?) _worldCamera[p] = null;
      }
      views?.changed(this, null, reset: true);
      if (announce || !r.ok) {
        notice = r.ok
            ? 'Cache rebuilt: ${r.cooked} cooked, ${r.cached} cached'
            : 'Cache rebuilt with ${r.failed} failure(s): ${r.cooked} cooked, ${r.cached} cached';
      }
    } on AuthoringException catch (e) {
      notice = 'Rebuild failed: $e';
    }
  }

  /// The pixels of a fresh frame of the focused tab (RGBA8, the viewport's
  /// physical size), or null when nothing renders.
  Uint8List? captureFrame() {
    final slot = renderFrame();
    if (slot == null || slot < 0) return null;
    return controller.readFrame(slot);
  }

  /// "Save Frame…": one frame as a PNG at the viewport's physical size —
  /// the pixel gate compares it with `fynkel render --size WxH` from the
  /// aligned scene camera. Returns the path written, null when cancelled
  /// or nothing rendered.
  Future<String?> saveFrame() async {
    final t = current;
    final choose = chooseSavePath;
    if (t == null || !t.hasPreview || choose == null) return null;
    if (!controller.isAttached) {
      notice = 'No frame to save: the viewport is not attached';
      notifyListeners();
      return null;
    }
    final w = controller.physicalWidth, h = controller.physicalHeight;
    final path = await choose('${t.tab.fileStem}-${w}x$h.png');
    if (path == null || _disposed) return null;
    final pixels = captureFrame();
    if (pixels == null) {
      notice = 'No frame to save: the viewport dropped the frame';
      notifyListeners();
      return null;
    }
    try {
      File(path).writeAsBytesSync(encodePng(w, h, pixels));
      final overlaid = showGrid || showOverlays || t.tab.selection.isNotEmpty;
      notice =
          'Saved $path ($w×$h) — compare with `fynkel render --size ${w}x$h`'
          '${overlaid ? '; the grid, icons or selection outline are in it (⌘G, ⌘I, Esc for a clean frame)' : ''}';
    } catch (e) {
      notice = 'Could not save the frame: $e';
    }
    notifyListeners();
    return path;
  }

  // ── Rendering ────────────────────────────────────────────────────────

  /// The panel's layout size in physical pixels (before the ring exists).
  void setViewportSize(double width, double height) {
    if (width == viewportWidth && height == viewportHeight) return;
    viewportWidth = width;
    viewportHeight = height;
    _repaint();
  }

  void _repaint() {
    if (_disposed) return;
    notifyListeners();
    requestFrame();
  }

  /// Draw a frame after this Flutter frame (coalesced).
  void requestFrame() {
    if (_frameScheduled || _disposed) return;
    _frameScheduled = true;
    SchedulerBinding.instance.addPostFrameCallback((_) {
      _frameScheduled = false;
      if (!_disposed) renderFrame();
    });
    SchedulerBinding.instance.ensureVisualUpdate();
  }

  /// Renders the focused tab's world now: camera, hidden set, debug lines
  /// (grid, axes, outlines, icons), then the frame. Returns the ring slot
  /// (see `FynkelViewportController.renderHandle`), null when nothing
  /// could be drawn.
  int? renderFrame() {
    final t = current;
    if (t == null || !t.hasPreview || !controller.isAttached) return null;
    if (controller.physicalWidth > 0) {
      viewportWidth = controller.physicalWidth.toDouble();
      viewportHeight = controller.physicalHeight.toDouble();
    }
    final p = t.preview!;
    final started = DateTime.now();
    try {
      if (!t.cameraApplied) {
        p.setCamera(t.camera.toPreview());
        _worldCamera[p] = (t, t.cameraVersion);
      }
      _applyHidden(t);
      controller.debugLines(debugLines(t));
      final slot = controller.renderHandle(p.worldAddress);
      // A resize in flight refuses the frame (the ring is being rebuilt).
      // The editor draws on demand, so without asking again this frame
      // would simply be lost and the panel would sit on a stale — or
      // blank — surface until the next thing to happen.
      if (slot == null && controller.state == ViewportState.resizing) {
        requestFrame();
        return null;
      }
      t.lastSlot = slot;
      t.frames++;
      lastFrameMs = DateTime.now().difference(started).inMicroseconds / 1000;
      return slot;
    } on AuthoringException catch (e) {
      notice = 'Preview: $e';
      notifyListeners();
      return null;
    }
  }

  /// The debug lines of a frame: the grid and axes (when shown), the
  /// selection outline, the icons (when shown).
  Float32List debugLines(SceneTab t) {
    final parts = <Float32List>[];
    if (showGrid) parts.add(gridLines(t.camera));
    final hidden = _hiddenAll(t);
    final outline = t.tab.selection.where((id) => !hidden.contains(id)).toList();
    if (outline.isNotEmpty || showOverlays) {
      final l = t.preview!.lines(outline: outline, iconsAspect: showOverlays ? aspect : null);
      if (l.count > 0) parts.add(l.data);
    }
    final total = parts.fold(0, (n, p) => n + p.length);
    final out = Float32List(total);
    var o = 0;
    for (final p in parts) {
      out.setAll(o, p);
      o += p.length;
    }
    return out;
  }

  /// The ground grid at an adaptive spacing (a power of ten near a
  /// twentieth of the camera's distance), 40 × 40 cells around the pivot,
  /// every tenth line brighter, plus the world axes through the origin:
  /// X red, Y green, Z blue.
  static Float32List gridLines(EditorCamera camera) {
    final d = math.max(camera.distance, 0.5);
    final step = math.pow(10, (math.log(d / 20) / math.ln10).floor()).toDouble();
    final n = 20;
    final cx = (camera.pivot[0] / (step * 10)).round() * step * 10;
    final cz = (camera.pivot[2] / (step * 10)).round() * step * 10;
    final lines = <double>[];
    void add(Vec3 a, Vec3 b, List<double> c) => lines.addAll([...a, ...b, ...c]);
    const minor = [0.35, 0.36, 0.4, 0.35];
    const major = [0.5, 0.52, 0.58, 0.7];
    for (var i = -n; i <= n; i++) {
      final x = cx + i * step, z = cz + i * step;
      final isMajor = ((cx / step).round() + i) % 10 == 0;
      final c = isMajor ? major : minor;
      if (x != 0) add([x, 0, cz - n * step], [x, 0, cz + n * step], c);
      final isMajorZ = ((cz / step).round() + i) % 10 == 0;
      if (z != 0) add([cx - n * step, 0, z], [cx + n * step, 0, z], isMajorZ ? major : minor);
    }
    final ext = n * step;
    add([-ext + cx, 0, 0], [ext + cx, 0, 0], const [0.9, 0.25, 0.25, 1]);
    add([0, 0, -ext + cz], [0, 0, ext + cz], const [0.3, 0.5, 1.0, 1]);
    add([0, 0, 0], [0, ext, 0], const [0.35, 0.85, 0.35, 1]);
    return Float32List.fromList(lines);
  }

  // ── Gizmo ────────────────────────────────────────────────────────────

  /// The selected, unlocked, Transform-bearing entities the gizmo moves
  /// (topmost of the selection), lead first.
  List<String> _dragTargets(SceneTab t) {
    final sel = t.tab.tree.topmost(t.tab.selection);
    final out = [
      for (final id in sel)
        if (!t.state.locked.contains(id) && t.tab.tree[id]?.transformId != null) id,
    ];
    final lead = t.tab.lead;
    if (lead != null && out.remove(lead)) out.insert(0, lead);
    return out;
  }

  /// Why a transform tool shows no gizmo for the selection, in a few
  /// words for the status line; null when there is a gizmo or nothing to
  /// explain (no selection, the select tool).
  String? get noGizmoReason {
    final t = current;
    if (t == null || !t.hasPreview || tool == SceneTool.select || t.tab.selection.isEmpty) return null;
    if (t.tab.document.readOnly != null) return 'read-only: no gizmo';
    if (_dragTargets(t).isNotEmpty) return null;
    final top = t.tab.tree.topmost(t.tab.selection);
    if (top.every(t.state.locked.contains)) return 'locked (L to unlock)';
    if (top.any((id) => t.tab.tree[id]?.isDerived ?? false)) {
      return 'no Transform component on the prefab\'s entity — add one in the Inspector (an override) to move it';
    }
    return 'no Transform component — add one in the Inspector to move it';
  }

  /// The gizmo's frame for the selection now: pivot at the lead's origin,
  /// world or local axes. Null when the tool is select, nothing draggable
  /// is selected or the preview is gone.
  GizmoGeometry? gizmoGeometry() {
    final t = current;
    final vp = viewProjection;
    if (t == null || !t.hasPreview || vp == null || tool == SceneTool.select) return null;
    if (t.tab.document.readOnly != null) return null; // nothing to drag into
    final d = drag;
    if (d != null) {
      return GizmoGeometry(tool: d.tool, pivot: d.currentPivot, axes: d.axes, size: d.size);
    }
    final targets = _dragTargets(t);
    if (targets.isEmpty) return null;
    final lead = targets.first;
    final world = t.preview!.worldMatrices([lead])[lead];
    if (world == null) return null;
    final pivot = matTranslation(world);
    final local = space == GizmoSpace.local || tool == SceneTool.scale;
    final axes = local
        ? matAxes(world)
        : const [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
          ];
    return GizmoGeometry(
      tool: tool,
      pivot: pivot,
      axes: axes,
      size: vp.metresPerPixel(pivot) * GizmoGeometry.screenSize,
    );
  }

  /// The gizmo on screen now.
  ProjectedGizmo? projectedGizmo() {
    final g = gizmoGeometry();
    final vp = viewProjection;
    if (g == null || vp == null) return null;
    return ProjectedGizmo.project(g, vp);
  }

  /// Starts a drag on [handle] from [px]; false when nothing is draggable.
  bool beginDrag(GizmoHandle handle, Offset px) {
    final t = current;
    final g = gizmoGeometry();
    final vp = viewProjection;
    if (t == null || g == null || vp == null || !canEdit) return false;
    final projected = ProjectedGizmo.project(g, vp);
    if (projected == null) return false;
    final ids = _dragTargets(t);
    final matrices = t.preview!.worldMatrices([...ids, for (final id in ids) ?t.tab.tree[id]?.parent]);
    final targets = <GizmoTarget>[];
    for (final id in ids) {
      final node = t.tab.tree[id]!;
      final world = matrices[id];
      if (world == null) continue;
      final parentWorld = node.parent == null
          ? List.of(identityMatrix)
          : (matrices[node.parent!] ?? List.of(identityMatrix));
      targets.add(
        GizmoTarget(
          id: id,
          transformId: node.transformId!,
          startLocal: node.transform ?? Trs.identity,
          startWorld: world,
          parentWorld: parentWorld,
          anchor: node.anchor,
          entityLocal: node.local,
          transformLocal: node.transformLocal,
        ),
      );
    }
    if (targets.isEmpty) return false;
    drag = GizmoDrag(
      tool: g.tool,
      handle: handle,
      startPx: px,
      pivot: g.pivot,
      axes: g.axes,
      size: g.size,
      projected: projected,
      targets: targets,
    )..currentPivot = List.of(g.pivot);
    _mode = _Mode.gizmo;
    notifyListeners();
    return true;
  }

  /// The pointer moved to [px] during a drag: the targets' transforms are
  /// previewed in the world (no document change). [invertSnap] (⌘ held)
  /// flips the snap setting for this move.
  void updateDrag(Offset px, {bool invertSnap = false}) {
    final d = drag;
    final t = current;
    final vp = viewProjection;
    if (d == null || t == null || vp == null) return;
    if ((px - d.startPx).distance < 2 && !d.moved) return;
    d.moved = true;
    final snapping = snap.enabled != invertSnap;
    final r0 = vp.rayAt(d.startPx.dx, d.startPx.dy);
    final r1 = vp.rayAt(px.dx, px.dy);
    final camForward = t.camera.forward;
    List<double>? delta; // the rigid/scale transform about the pivot, as a matrix
    switch (d.tool) {
      case SceneTool.select:
        return;
      case SceneTool.move:
        Vec3 move;
        if (d.handle.isAxis) {
          final axis = d.axes[d.handle.axisIndex];
          final s0 = r0.closestOnLine(d.pivot, axis);
          final s1 = r1.closestOnLine(d.pivot, axis);
          if (s0 == null || s1 == null) return;
          var s = s1 - s0;
          if (snapping) s = snapTo(s, snap.move);
          move = vScale(axis, s);
          d.readout = '${_fmt(s)} m';
        } else {
          final normal = d.handle.isPlane ? d.axes[d.handle.planeNormalIndex] : camForward;
          final h0 = r0.hitPlane(d.pivot, normal);
          final h1 = r1.hitPlane(d.pivot, normal);
          if (h0 == null || h1 == null) return;
          move = vSub(h1, h0);
          if (snapping) {
            if (d.handle.isPlane) {
              final (a, b) = d.handle.planeAxes;
              final sa = snapTo(vDot(move, d.axes[a]), snap.move);
              final sb = snapTo(vDot(move, d.axes[b]), snap.move);
              move = vAdd(vScale(d.axes[a], sa), vScale(d.axes[b], sb));
            } else {
              move = [for (final v in move) snapTo(v, snap.move)];
            }
          }
          d.readout = '${_fmt(vLength(move))} m';
        }
        delta = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, move[0], move[1], move[2], 1];
        d.currentPivot = vAdd(d.pivot, move);
      case SceneTool.rotate:
        final pivotPx = d.projected.pivot;
        final a0 = math.atan2(d.startPx.dy - pivotPx.dy, d.startPx.dx - pivotPx.dx);
        final a1 = math.atan2(px.dy - pivotPx.dy, px.dx - pivotPx.dx);
        var angle = a1 - a0;
        while (angle > math.pi) {
          angle -= 2 * math.pi;
        }
        while (angle < -math.pi) {
          angle += 2 * math.pi;
        }
        Vec3 axis;
        if (d.handle.isAxis) {
          axis = d.axes[d.handle.axisIndex];
          // Screen angles grow clockwise (y down); a positive rotation
          // about an axis pointing at the camera looks counter-clockwise.
          if (d.projected.axisDepth[d.handle.axisIndex]) angle = -angle;
        } else {
          axis = camForward;
        }
        if (snapping) angle = snapTo(angle, snap.angle * math.pi / 180);
        d.readout = '${_fmt(angle * 180 / math.pi)}°';
        final q = quatFromAxisAngle(axis, angle);
        final r = quatToMatrix(q);
        delta = matMul(_translation(d.pivot), matMul(r, _translation(vScale(d.pivot, -1))));
        d.currentPivot = List.of(d.pivot);
      case SceneTool.scale:
        List<double> f;
        if (d.handle.isAxis) {
          final axis = d.axes[d.handle.axisIndex];
          final s0 = r0.closestOnLine(d.pivot, axis);
          final s1 = r1.closestOnLine(d.pivot, axis);
          if (s0 == null || s1 == null) return;
          var factor = 1 + (s1 - s0) / d.size;
          if (snapping) factor = snapTo(factor, snap.scale);
          factor = math.max(factor, 0.001);
          f = [1, 1, 1];
          f[d.handle.axisIndex] = factor;
          d.readout = '×${_fmt(factor)}';
        } else {
          final pivotPx = d.projected.pivot;
          final l0 = (d.startPx - pivotPx).distance;
          final l1 = (px - pivotPx).distance;
          var factor = l0 < 1 ? 1.0 : l1 / l0;
          if (snapping) factor = snapTo(factor, snap.scale);
          factor = math.max(factor, 0.001);
          f = [factor, factor, factor];
          d.readout = '×${_fmt(factor)}';
        }
        // Scale along the gizmo axes about the pivot: A·diag(f)·Aᵀ.
        final a = d.axes;
        final s = List<double>.filled(16, 0);
        for (var r = 0; r < 3; r++) {
          for (var c = 0; c < 3; c++) {
            var v = 0.0;
            for (var k = 0; k < 3; k++) {
              v += a[k][r] * f[k] * a[k][c];
            }
            s[c * 4 + r] = v;
          }
        }
        s[15] = 1;
        delta = matMul(_translation(d.pivot), matMul(s, _translation(vScale(d.pivot, -1))));
        d.currentPivot = List.of(d.pivot);
    }
    final previews = <PreviewTransform>[];
    for (final target in d.targets) {
      final newWorld = matMul(delta, target.startWorld);
      final local = Trs.fromMatrix(matMul(matInverseAffine(target.parentWorld), newWorld));
      // A pure move keeps the rotation and scale bits exactly (no
      // decomposition noise on what did not change).
      final trs = switch (d.tool) {
        SceneTool.move => Trs(
          position: local.position,
          rotation: target.startLocal.rotation,
          scale: target.startLocal.scale,
        ),
        SceneTool.rotate => Trs(position: local.position, rotation: local.rotation, scale: target.startLocal.scale),
        _ => local,
      };
      target.current = trs;
      previews.add(PreviewTransform(id: target.id, position: trs.position, rotation: trs.rotation, scale: trs.scale));
    }
    try {
      t.preview!.setTransforms(previews);
    } on AuthoringException catch (e) {
      notice = 'Preview: $e';
    }
    views?.changed(this, t.preview);
    _repaint();
  }

  static List<double> _translation(Vec3 v) => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, v[0], v[1], v[2], 1];

  static String _fmt(double v) {
    final r = (v * 1000).roundToDouble() / 1000;
    return r == r.roundToDouble() ? r.toInt().toString() : r.toStringAsFixed(r.abs() < 0.1 ? 3 : 2);
  }

  /// The drag ends: the previewed transforms become one transaction of
  /// `setField`s (only the fields that changed) — for a derived entity of
  /// a prefab instance, `setField` overrides on its anchor (M4-07); a drag
  /// that moved nothing writes nothing. Returns the transaction's outcome
  /// (null when none was needed or the call failed).
  Applied? endDrag() {
    final d = drag;
    final t = current;
    drag = null;
    _mode = _Mode.none;
    if (d == null || t == null || !t.hasPreview) {
      notifyListeners();
      return null;
    }
    final commands = <Map<String, Object?>>[];
    for (final target in d.targets) {
      if (!d.moved || target.current.sameAs(target.startLocal)) continue;
      final before = target.startLocal.toData(), after = target.current.toData();
      for (final field in const ['position', 'rotation', 'scale']) {
        if ('${before[field]}' == '${after[field]}') continue;
        if (target.isDerived) {
          commands.add(
            Command.setPrefabOverrideOp(
              target.anchor!,
              prefab.setFieldOverride(
                entity: target.entityLocal ?? target.id,
                component: target.transformLocal ?? target.transformId,
                data: after,
                path: field,
                value: after[field],
              ),
            ),
          );
        } else {
          commands.add(Command.setField(target.id, target.transformId, field, after[field]));
        }
      }
    }
    // The world back to the document first: the transaction's sync then
    // moves it to the committed values (or, refused, leaves it as the
    // document says).
    try {
      t.preview!.restoreTransforms(d.ids);
    } on AuthoringException catch (e) {
      notice = 'Preview: $e';
    }
    views?.changed(this, t.preview);
    if (commands.isEmpty) {
      _repaint();
      return null;
    }
    final a = t.tab.transaction(commands);
    if (a == null || !a.ok) {
      notice = t.tab.lastError ?? 'The move was refused';
    } else {
      notice = null;
    }
    _repaint();
    return a;
  }

  /// Escape: the world back to the document, nothing written.
  void cancelDrag() {
    final d = drag;
    final t = current;
    drag = null;
    if (_mode == _Mode.gizmo) _mode = _Mode.none;
    if (d != null && t != null && t.hasPreview) {
      try {
        t.preview!.restoreTransforms(d.ids);
      } on AuthoringException catch (_) {}
      views?.changed(this, t.preview);
    }
    _repaint();
  }

  // ── Picking ──────────────────────────────────────────────────────────

  /// The pick hits under [px], nearest first, without hidden or locked
  /// entities (a locked entity is never picked in the view).
  List<String> hitsAt(Offset px) {
    final t = current;
    if (t == null || !t.hasPreview || viewportWidth <= 0) return const [];
    final ndc = [px.dx / viewportWidth * 2 - 1, 1 - px.dy / viewportHeight * 2];
    final pick = t.preview!.pick(t.camera.toPreview(), aspect, ndc);
    final hidden = _hiddenAll(t);
    return [
      for (final h in pick.hits)
        if (!t.state.locked.contains(h.id) && !hidden.contains(h.id) && t.tab.tree.nodes.containsKey(h.id)) h.id,
    ];
  }

  /// A click at [px]: selects the nearest hit; a repeated click on the
  /// same pixel cycles to the next stacked hit; ⌘ toggles, ⇧ adds; a click
  /// on nothing clears (unless a modifier is held).
  void click(Offset px, {bool toggle = false, bool add = false}) {
    final t = current;
    if (t == null || !t.hasPreview) return;
    final samePixel = t.clickPx != null && (t.clickPx! - px).distance < 3 && t.clickHits.isNotEmpty;
    List<String> hits;
    if (samePixel) {
      hits = t.clickHits;
      t.clickIndex = (t.clickIndex + 1) % hits.length;
    } else {
      hits = hitsAt(px);
      t.clickHits = hits;
      t.clickIndex = 0;
      t.clickPx = px;
    }
    if (hits.isEmpty) {
      if (!toggle && !add) t.tab.clearSelection();
      return;
    }
    final id = hits[t.clickIndex];
    if (toggle) {
      t.tab.select(id, toggle: true);
    } else if (add) {
      if (!t.tab.selection.contains(id)) t.tab.select(id, toggle: true);
    } else {
      t.tab.select(id);
    }
    t.tab.tree.reveal(id);
  }

  /// The marquee from [a] to [b] (physical pixels) selects what overlaps
  /// it; ⌘ toggles, ⇧ adds.
  void marqueeSelect(Offset a, Offset b, {bool toggle = false, bool add = false}) {
    final t = current;
    if (t == null || !t.hasPreview || viewportWidth <= 0) return;
    List<double> ndc(Offset p) => [p.dx / viewportWidth * 2 - 1, 1 - p.dy / viewportHeight * 2];
    final na = ndc(a), nb = ndc(b);
    final min = [math.min(na[0], nb[0]), math.min(na[1], nb[1])];
    final max = [math.max(na[0], nb[0]), math.max(na[1], nb[1])];
    final hidden = _hiddenAll(t);
    final ids = t.preview!
        .pickRect(t.camera.toPreview(), aspect, min, max)
        .where((id) => !t.state.locked.contains(id) && !hidden.contains(id) && t.tab.tree.nodes.containsKey(id));
    if (toggle) {
      final s = Set<String>.of(t.tab.selection);
      for (final id in ids) {
        if (!s.remove(id)) s.add(id);
      }
      t.tab.setSelection(s);
    } else if (add) {
      t.tab.setSelection({...t.tab.selection, ...ids});
    } else {
      t.tab.setSelection(ids);
    }
  }

  // ── Prefab drop ──────────────────────────────────────────────────────

  bool canDropAsset(AssetDragPayload d) =>
      hasView &&
      hierarchy.canDropAsset(d.toRef(), d.kind, DropTarget(parent: null, index: hierarchy.tree?.roots.length ?? 0));

  /// Where a drop at [px] lands: the nearest mesh hit, else the ground
  /// plane (y = 0), else 5 m along the ray; snapped when snapping is on.
  Vec3? dropPoint(Offset px) {
    final t = current;
    final vp = viewProjection;
    if (t == null || !t.hasPreview || vp == null) return null;
    final ray = vp.rayAt(px.dx, px.dy);
    final pick = t.preview!.pick(t.camera.toPreview(), aspect, vp.ndcOf(px.dx, px.dy));
    Vec3 p;
    final mesh = pick.hits.where((h) => h.mesh).firstOrNull;
    if (mesh != null) {
      p = mesh.point;
    } else {
      p = ray.hitPlane(const [0, 0, 0], const [0, 1, 0]) ?? ray.at(5);
    }
    if (snap.enabled) p = [for (final v in p) snapTo(v, snap.move)];
    return [for (final v in p) Trs.clean(v)];
  }

  /// Drops a prefab at [px]: one `instantiatePrefab` transaction with the
  /// instance's Transform at the hit point; the instance is selected.
  bool dropPrefab(AssetDragPayload d, Offset px) {
    if (!canDropAsset(d)) return false;
    final p = dropPoint(px);
    if (p == null) return false;
    final ok = hierarchy.instantiatePrefab(
      d.toRef(),
      name: prefabInstanceName(d.name),
      target: DropTarget(parent: null, index: hierarchy.tree?.roots.length ?? 0),
      transform: Trs(position: p, rotation: const [0, 0, 0, 1], scale: const [1, 1, 1]),
    );
    if (ok) notice = null;
    _repaint();
    return ok;
  }

  // ── Pointer input (physical pixels) ──────────────────────────────────

  bool get _alt => HardwareKeyboard.instance.isAltPressed;
  bool get _meta => HardwareKeyboard.instance.isMetaPressed;
  bool get _shiftHeld => HardwareKeyboard.instance.isShiftPressed;

  void pointerDown(PointerDownEvent e, Offset px) {
    if (!hasView) return;
    pointer = px;
    _last = px;
    _downPx = px;
    final alt = _alt;
    if (e.buttons == kMiddleMouseButton || (alt && e.buttons == kPrimaryMouseButton && _shiftHeld)) {
      _mode = _Mode.pan;
    } else if (alt && e.buttons == kPrimaryMouseButton) {
      _mode = _Mode.orbit;
    } else if (alt && e.buttons == kSecondaryMouseButton) {
      _mode = _Mode.dolly;
    } else if (e.buttons == kSecondaryMouseButton) {
      _mode = _Mode.look;
      _startFly();
    } else if (e.buttons == kPrimaryMouseButton) {
      final handle = projectedGizmo()?.hitTest(px);
      if (handle != null && canEdit && beginDrag(handle, px)) {
        _mode = _Mode.gizmo;
      } else {
        _mode = _Mode.click;
      }
    }
    notifyListeners();
  }

  void pointerMove(PointerMoveEvent e, Offset px) {
    pointer = px;
    final last = _last ?? px;
    final dx = px.dx - last.dx, dy = px.dy - last.dy;
    _last = px;
    switch (_mode) {
      case _Mode.none:
        break;
      case _Mode.orbit:
        orbit(dx, dy);
      case _Mode.pan:
        pan(dx, dy);
      case _Mode.dolly:
        dolly(-dy / 20);
      case _Mode.look:
        look(dx, dy);
      case _Mode.gizmo:
        updateDrag(px, invertSnap: _meta);
      case _Mode.click:
        if ((px - _downPx!).distance > 4) {
          _mode = _Mode.marquee;
          marquee = Rect.fromPoints(_downPx!, px);
          notifyListeners();
        }
      case _Mode.marquee:
        marquee = Rect.fromPoints(_downPx!, px);
        notifyListeners();
    }
  }

  void pointerUp(PointerEvent e, Offset px) {
    pointer = px;
    final mode = _mode;
    _mode = _Mode.none;
    final toggle = _meta, add = _shiftHeld;
    switch (mode) {
      case _Mode.click:
        click(px, toggle: toggle, add: add);
      case _Mode.marquee:
        final m = marquee;
        marquee = null;
        if (m != null) marqueeSelect(m.topLeft, m.bottomRight, toggle: toggle, add: add);
      case _Mode.gizmo:
        endDrag();
      case _Mode.look:
        _stopFly();
      case _:
        break;
    }
    _downPx = null;
    notifyListeners();
  }

  void pointerCancel(PointerEvent e, Offset px) {
    if (_mode == _Mode.gizmo) cancelDrag();
    if (_mode == _Mode.look) _stopFly();
    _mode = _Mode.none;
    marquee = null;
    notifyListeners();
  }

  void pointerHover(PointerHoverEvent e, Offset px) {
    pointer = px;
    final h = drag == null ? projectedGizmo()?.hitTest(px) : null;
    if (h != hoverHandle) {
      hoverHandle = h;
      notifyListeners();
    }
  }

  /// A mouse wheel: dolly; with ⇧ pan.
  void pointerSignal(PointerSignalEvent e, Offset px) {
    if (e is! PointerScrollEvent || !hasView) return;
    pointer = px;
    _scroll(e.scrollDelta);
  }

  /// A scroll of [delta] (a wheel's direction: +y is away from the user).
  void _scroll(Offset delta) {
    if (_shiftHeld) {
      pan(-delta.dx, -delta.dy);
    } else {
      dolly(-delta.dy / 40);
    }
  }

  // ── Trackpad ─────────────────────────────────────────────────────────
  //
  // On macOS a two-finger scroll and a pinch are not wheel events: they
  // arrive as one pan-zoom gesture whose pan and scale are totals since it
  // began. A scroll dollies (⇧ pans) exactly as the wheel does; a pinch
  // dollies by its own ratio — spread the fingers and the view comes closer.

  Offset _gesturePan = Offset.zero;
  double _gestureScale = 1;

  void panZoomStart(PointerPanZoomStartEvent e, Offset px) {
    pointer = px;
    _gesturePan = Offset.zero;
    _gestureScale = 1;
  }

  void panZoomUpdate(PointerPanZoomUpdateEvent e, Offset px) {
    if (!hasView) return;
    pointer = px;
    final pan = e.pan - _gesturePan;
    final ratio = e.scale / _gestureScale;
    _gesturePan = e.pan;
    _gestureScale = e.scale;
    if (ratio.isFinite && ratio > 0 && ratio != 1) dolly(math.log(ratio) * 10);
    // A finger movement is the opposite of the scroll it makes.
    if (pan != Offset.zero) _scroll(-pan);
  }

  void panZoomEnd(PointerPanZoomEndEvent e, Offset px) {
    _gesturePan = Offset.zero;
    _gestureScale = 1;
  }

  void _cancelInteraction() {
    if (drag != null) cancelDrag();
    _stopFly();
    _mode = _Mode.none;
    marquee = null;
    hoverHandle = null;
  }

  // ── Fly (RMB held: WASD, Q/E down/up, ⇧ faster) ──────────────────────

  bool get flying => _mode == _Mode.look;

  void _startFly() {
    _flyTimer ??= Timer.periodic(const Duration(milliseconds: 16), (_) => _flyTick());
  }

  void _stopFly() {
    _flyTimer?.cancel();
    _flyTimer = null;
    _flyKeys.clear();
  }

  void _flyTick() {
    if (_flyKeys.isEmpty || camera == null) return;
    final c = camera!;
    var speed = math.max(c.distance, 1) * 0.6; // m/s, scaled with the view
    if (_shift) speed *= 4;
    final step = speed * 0.016;
    double f = 0, r = 0, u = 0;
    if (_flyKeys.contains(LogicalKeyboardKey.keyW)) f += step;
    if (_flyKeys.contains(LogicalKeyboardKey.keyS)) f -= step;
    if (_flyKeys.contains(LogicalKeyboardKey.keyD)) r += step;
    if (_flyKeys.contains(LogicalKeyboardKey.keyA)) r -= step;
    if (_flyKeys.contains(LogicalKeyboardKey.keyE)) u += step;
    if (_flyKeys.contains(LogicalKeyboardKey.keyQ)) u -= step;
    c.fly(f, r, u);
    _cameraChanged();
  }

  static final _flySet = {
    LogicalKeyboardKey.keyW,
    LogicalKeyboardKey.keyA,
    LogicalKeyboardKey.keyS,
    LogicalKeyboardKey.keyD,
    LogicalKeyboardKey.keyQ,
    LogicalKeyboardKey.keyE,
  };

  // ── Keys (the panel's focus; bare hotkeys, no modifiers) ─────────────

  /// The Scene view's own keys while it has focus: Q/W/E/R tools, X
  /// local/world, F frame selected, ⇧F frame all, H hide/show, ⇧H show
  /// all, L lock, 5 projection, Esc cancel/deselect, ⌫/⌦ delete, WASDQE
  /// fly while the right button is held. Anything with ⌘ falls through to
  /// the registry's shortcuts.
  KeyEventResult handleKey(KeyEvent e) {
    if (!hasView) return KeyEventResult.ignored;
    final key = e.logicalKey;
    _shift = HardwareKeyboard.instance.isShiftPressed;
    if (e is KeyUpEvent) {
      if (_flySet.contains(key) && _flyKeys.remove(key)) return KeyEventResult.handled;
      return KeyEventResult.ignored;
    }
    if (HardwareKeyboard.instance.isMetaPressed ||
        HardwareKeyboard.instance.isControlPressed ||
        HardwareKeyboard.instance.isAltPressed) {
      return KeyEventResult.ignored;
    }
    if (flying && _flySet.contains(key)) {
      _flyKeys.add(key);
      return KeyEventResult.handled;
    }
    if (e is KeyRepeatEvent) return KeyEventResult.ignored;
    final shift = _shift;
    if (key == LogicalKeyboardKey.escape) {
      if (drag != null) {
        cancelDrag();
      } else if (marquee != null) {
        marquee = null;
        _mode = _Mode.none;
        notifyListeners();
      } else {
        current?.tab.clearSelection();
      }
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyQ) return _set(SceneTool.select);
    if (key == LogicalKeyboardKey.keyW) return _set(SceneTool.move);
    if (key == LogicalKeyboardKey.keyE) return _set(SceneTool.rotate);
    if (key == LogicalKeyboardKey.keyR) return _set(SceneTool.scale);
    if (key == LogicalKeyboardKey.keyX) {
      toggleSpace();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyF) {
      shift ? frameAll() : frameSelected();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyH) {
      shift ? showAll() : toggleHiddenSelection();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.keyL) {
      toggleLockedSelection();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.digit5) {
      toggleProjection();
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.backspace || key == LogicalKeyboardKey.delete) {
      if (current?.tab.selection.isNotEmpty ?? false) {
        hierarchy.delete();
        return KeyEventResult.handled;
      }
    }
    return KeyEventResult.ignored;
  }

  KeyEventResult _set(SceneTool t) {
    tool = t;
    return KeyEventResult.handled;
  }

  // ── Stats ────────────────────────────────────────────────────────────

  /// The preview's counts now (a `stats` call), null without a preview.
  PreviewStats? previewStats() {
    final t = current;
    if (t == null || !t.hasPreview) return null;
    try {
      return t.preview!.refreshStats();
    } on AuthoringException catch (_) {
      return t.preview!.stats;
    }
  }

  /// The renderer's counters since the last read.
  ViewportStatsSnapshot? viewportStats() => controller.takeStats();

  // ── Lifecycle ────────────────────────────────────────────────────────

  @override
  void dispose() {
    _disposed = true;
    views?._views.remove(this);
    _stopFly();
    tabs.removeListener(_onTabs);
    project.removeListener(_onProject);
    editorState.removeListener(notifyListeners);
    controller.removeListener(_onController);
    _listening?.removeListener(_onTab);
    focusNode.removeListener(_onFocus);
    // A copy closing leaves the worlds to the documents and the other views.
    final copy = view != SceneEditorState.firstView;
    for (final st in _sceneTabs.values) {
      st.dispose(closePreview: !copy);
    }
    _sceneTabs.clear();
    editorState.flush();
    controller.dispose();
    focusNode.dispose();
    super.dispose();
  }
}
