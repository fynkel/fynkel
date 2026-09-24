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
import '../inspector/component_specs.dart';
import '../inspector/field_widgets.dart' show AssetDragPayload;
import '../inspector/prefab_actions.dart' as prefab;
import '../play/live_preview.dart';
import '../play/play_document.dart';
import '../project/editor_project.dart';
import '../theme/tokens.dart' show GizmoTokens;
import 'component_handles.dart';
import 'declared_gizmos.dart';
import 'editor_state.dart';
import 'gizmo.dart';
import 'scene_tools.dart';
import 'png.dart';
import 'scene_math.dart';

/// Which Scene view's camera each preview world holds, and at which
/// version: the copies share a world, so a view sets its own camera again
/// when another one drew last.
final _worldCamera = Expando<(SceneTab, int)>('scene.worldCamera');

/// Someone other than a Scene view (the Game view's camera preview,
/// ADR-0021) set [preview]'s camera: every view sets its own again before
/// its next frame.
void releaseWorldCamera(PreviewBackend preview) => _worldCamera[preview] = null;

/// The Scene view copies of one project (item 9e).
final class SceneViews {
  final _views = <SceneViewModel>{};
  int? _recookedAt;

  /// Told of every change a view makes to a world (null: every world) —
  /// the Game view's camera preview draws again.
  final worldListeners = <void Function(PreviewBackend? preview)>{};

  /// A view changed [preview]'s world (null: every world): the others
  /// showing it draw again. [reset] when the world was rebuilt, so its
  /// camera and hidden set must be applied again.
  void changed(SceneViewModel from, PreviewBackend? preview, {bool reset = false}) {
    for (final v in List.of(_views)) {
      if (v != from) v._worldChanged(preview, reset: reset);
    }
    for (final l in List.of(worldListeners)) {
      l(preview);
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

/// The component the Scene view's handles edit (the Inspector's Edit
/// toggle, item 9g): where it is, its data as the document holds it, the
/// entity's world matrix and its handler.
final class ComponentTarget {
  const ComponentTarget({
    required this.entityId,
    required this.componentId,
    required this.type,
    required this.data,
    required this.world,
    required this.handler,
    this.anchor,
    this.entityLocal,
    this.local,
  });

  final String entityId, componentId, type;
  final Map<String, Object?> data;
  final List<double> world;
  final ComponentHandles handler;

  /// A derived component of a prefab instance: the anchor its overrides
  /// go on and the prefab-local ids they name.
  final String? anchor, entityLocal, local;
  bool get isDerived => anchor != null;
}

/// A handle drag in flight.
final class GizmoDrag {
  GizmoDrag({
    required this.tool,
    required this.handle,
    required this.startPx,
    required this.frame,
    required this.pivotPx,
    required this.targets,
  });

  final SceneTool tool;
  final Handle handle;
  final Offset startPx;

  /// The gizmo's frame at the drag's start, and its pivot's pixel.
  final GizmoFrame frame;
  final Offset pivotPx;
  final List<GizmoTarget> targets;

  /// The pivot now (moves with the lead).
  Vec3 currentPivot = const [0, 0, 0];

  /// The pointer moved far enough to count (a click on a handle that
  /// never moves changes nothing).
  bool moved = false;

  /// The last delta, for the panel's readout ("Δ 1.50 m", "30°", "×1.2").
  String readout = '';

  Iterable<String> get ids => targets.map((t) => t.id);

  /// A component handle's drag: the component, the context at the start
  /// and the fields the drag sets now.
  ComponentTarget? component;
  ComponentContext? componentContext;
  Map<String, Object?> fields = const {};
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
    bool Function()? zoomToPointer,
  }) : controller = controller ?? FynkelViewportController(placeholders: false),
       zoomToPointer = zoomToPointer ?? _on {
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

  /// Whether a scroll or a pinch zooms about the point under the pointer
  /// (the preference) rather than the view's centre.
  final bool Function() zoomToPointer;
  static bool _on() => true;

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

  /// The id of the handle under the pointer.
  String? hoverHandle;

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

  /// The focused tab's Scene view — the running game's during Play
  /// (ADR-0021 P7 L5), as the Hierarchy shows it — opening its preview on
  /// first use; null without a focused tab.
  SceneTab? get current {
    final f = tabs.shown;
    if (f == null) return null;
    return _sceneTabOf(f);
  }

  /// The Scene view draws the running game (Play): continuously, through
  /// its own camera on the viewport, and nothing in it can be dragged.
  bool get isLive => current?.preview is LivePreview;

  SceneTab _sceneTabOf(OpenDocument tab) {
    final existing = _sceneTabs[tab];
    if (existing != null) {
      if (!existing.hasPreview && existing.openError == null) _openPreview(existing);
      return existing;
    }
    // The running game keeps the scene's camera and flags (Unity's Scene
    // view does not move when Play starts).
    final doc = tab.document;
    final stateId = doc is PlayDocument ? (doc.scene?.id ?? doc.id) : doc.id;
    final st = SceneTab(tab, editorState.scene(stateId), view: view);
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
      if (!tabs.open.contains(t) && t != tabs.live) {
        _sceneTabs.remove(t)!.dispose();
        editorState.flush();
      }
    }
    final f = tabs.shown;
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
    // Schemas or behaviour metadata may have changed (codegen ran).
    _specs.clear();
    _declared.clear();
    _declaredCache = null;
    _componentCache = null;
    if (project.assetsRevision != _assetsRevision) {
      _assetsRevision = project.assetsRevision;
      // Assets changed under the open previews: the cache is rebuilt so
      // the picture shows the new mesh or texture — once, by the first
      // Scene view copy to hear of it.
      final t = current;
      if (t != null && t.hasPreview && !isLive && (views?.claimRecook(_assetsRevision) ?? true)) {
        _recook(t, announce: false);
      }
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
    // Picking a tool ends a component's editing, as in Unity.
    current?.tab.editComponent(null);
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

  /// A scroll's or a pinch's dolly: about the point under the pointer, on
  /// the pivot's plane, when the preference says so.
  void _dollyAtPointer(double steps) {
    final c = camera, vp = viewProjection, px = pointer;
    final toward = c == null || vp == null || px == null || !zoomToPointer()
        ? null
        : vp.rayAt(px.dx, px.dy).hitPlane(c.pivot, c.forward);
    camera?.dolly(steps, toward: toward);
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
    _stopCoast();
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
    _stopCoast();
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
  /// reloaded. Play passes [live] (the unsaved documents cook as held,
  /// ADR-0021 P5) and no [announce]. Null when nothing could cook (no
  /// preview, or the cook threw — [notice] says why).
  CookReport? recook({bool live = false, bool announce = true}) {
    final t = current;
    if (t == null || !t.hasPreview) return null;
    final r = _recook(t, announce: announce, live: live);
    _repaint();
    return r;
  }

  CookReport? _recook(SceneTab t, {required bool announce, bool live = false}) {
    if (drag != null) cancelDrag();
    try {
      final r = t.preview!.recook(live: live);
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
      return r;
    } on AuthoringException catch (e) {
      notice = 'Rebuild failed: $e';
      return null;
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
    final live = p is LivePreview;
    try {
      if (live) {
        // The game's world keeps its camera; the viewport has its own.
        final c = t.camera.toPreview();
        controller.view = ViewportView(
          world: c.world,
          orthographic: c.orthographic,
          fovY: c.fovY,
          orthoHeight: c.orthoHeight,
          near: c.near,
          far: c.far,
          manualExposure: c.manualExposure != null,
          exposure: c.manualExposure ?? 12,
        );
      } else {
        if (controller.view != null) controller.view = null;
        if (!t.cameraApplied) {
          p.setCamera(t.camera.toPreview());
          _worldCamera[p] = (t, t.cameraVersion);
        }
      }
      _applyHidden(t);
      controller.overlay(overlay(t));
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
      // The game moves on its own: draw it again next frame.
      if (live) requestFrame();
      return slot;
    } on AuthoringException catch (e) {
      notice = 'Preview: $e';
      notifyListeners();
      return null;
    }
  }

  /// Logical-to-physical pixels of the panel the picture is in (its
  /// panel sets it on layout): overlay widths are the design tokens'
  /// logical pixels times this.
  double pixelRatio = 1;

  final _overlay = ViewportOverlay();

  /// The overlay of a frame (ADR-0019): the renderer's ground grid (when
  /// shown), the selection outline, the icons (when shown; colliders only
  /// for the selection, an unselected camera faint).
  /// Rebuilt into one reused buffer every frame.
  ViewportOverlay overlay(SceneTab t) {
    final o = _overlay..clear();
    final width = GizmoTokens.lineWidth * pixelRatio;
    if (showGrid) o.grid = const OverlayGrid(xAxis: GizmoTokens.xAxis, zAxis: GizmoTokens.zAxis);
    final hidden = _hiddenAll(t);
    final outline = t.tab.selection.where((id) => !hidden.contains(id)).toList();
    if (outline.isNotEmpty || showOverlays) {
      final l = t.preview!.lines(outline: outline, iconsAspect: showOverlays ? aspect : null);
      if (l.count > 0) o.segments(l.data, width: width);
    }
    _drawDeclared(o, t);
    _drawHandles(o);
    return o;
  }

  // ── Declared gizmos (item 9h) ───────────────────────────────────────

  late final _specs = ComponentSpecs(project);
  final _declared = <String, DeclaredGizmos?>{};

  /// [type]'s declared gizmos (`@Gizmo`, or the native hints'), parsed
  /// once per project scan; null when it declares none.
  DeclaredGizmos? declaredGizmos(String type) => _declared.putIfAbsent(type, () => DeclaredGizmos.of(_specs.of(type)));

  /// What the Edit toggle drags on [type]: its registered handles, else
  /// its declared gizmos.
  ComponentHandles? handlersFor(String type) => ComponentHandlers.of(type) ?? declaredGizmos(type);

  ({String key, List<({String id, String type, Map<String, Object?> data})> items})? _declaredCache;

  /// The components whose declared gizmos show: the selection's, and every
  /// shown carrier's of a type with an always-on one. Read once per
  /// document revision, selection and visibility.
  List<({String id, String type, Map<String, Object?> data})> _declaredItems(SceneTab t, Set<String> hidden) {
    final selection = t.tab.selection.toSet();
    final key = '${t.tab.document.revision}|${selection.join(',')}|${hidden.join(',')}';
    final cached = _declaredCache;
    if (cached != null && cached.key == key) return cached.items;
    final wanted = <String>[];
    for (final node in t.tab.tree.nodes.values) {
      if (hidden.contains(node.id)) continue;
      final selected = selection.contains(node.id);
      if (node.componentTypes.any((type) => declaredGizmos(type) != null && (selected || declaredGizmos(type)!.anyAlways))) {
        wanted.add(node.id);
      }
    }
    final items = <({String id, String type, Map<String, Object?> data})>[];
    final own = <String>[];
    for (final id in wanted) {
      final node = t.tab.tree[id]!;
      final anchor = node.overrideAnchor ?? (node.isPrefabInstance ? id : null);
      if (anchor == null) {
        own.add(id);
        continue;
      }
      final de = t.tab.expansionOf(anchor)?.entity(id);
      for (final c in de?.components.values ?? const <Never>[]) {
        if (declaredGizmos(c.type) != null) items.add((id: id, type: c.type, data: Map<String, Object?>.from(c.data)));
      }
    }
    if (own.isNotEmpty) {
      try {
        final entities = t.tab.document.entities(own).entities;
        for (final id in own) {
          final components = entities[id]?['components'];
          if (components is! Map) continue;
          for (final c in components.values) {
            if (c is! Map || c['type'] is! String || declaredGizmos(c['type'] as String) == null) continue;
            final data = c['data'] is Map ? Map<String, Object?>.from(c['data'] as Map) : <String, Object?>{};
            items.add((id: id, type: c['type'] as String, data: data));
          }
        }
      } on AuthoringException catch (_) {}
    }
    _declaredCache = (key: key, items: items);
    return items;
  }

  /// The declared gizmos of the selection and of `when: always` carriers,
  /// depth-faded like every gizmo; a component being dragged draws its
  /// dragged values.
  void _drawDeclared(ViewportOverlay o, SceneTab t) {
    final vp = viewProjection;
    if (vp == null) return;
    final items = _declaredItems(t, _hiddenAll(t));
    if (items.isEmpty) return;
    final byName = <String, String>{};
    for (final n in t.tab.tree.nodes.values) {
      byName.putIfAbsent(n.name, () => n.id);
    }
    final targets = <String>{};
    for (final item in items) {
      for (final f in declaredGizmos(item.type)!.fields) {
        final name = item.data[f.name];
        if (f.gizmo!.shape == 'lineTo' && name is String) targets.addAll([?byName[name]]);
      }
    }
    final matrices = t.preview!.worldMatrices([...{for (final i in items) i.id}, ...targets]);
    final selection = t.tab.selection.toSet();
    final d = drag;
    final width = GizmoTokens.lineWidth * pixelRatio;
    for (final item in items) {
      final world = matrices[item.id];
      if (world == null) continue;
      final dragged = d?.component;
      final data = dragged != null && dragged.entityId == item.id && dragged.type == item.type
          ? {...item.data, ...d!.fields}
          : item.data;
      final c = ComponentContext(world: world, data: data, vp: vp, pixelRatio: pixelRatio, aspect: aspect);
      final lines = declaredGizmos(item.type)!.lines(
        c,
        selected: selection.contains(item.id),
        find: (name) {
          final m = matrices[byName[name]];
          return m == null ? null : matTranslation(m);
        },
      );
      for (final l in lines) {
        for (var i = 0; i + 1 < l.points.length; i++) {
          o.line(l.points[i], l.points[i + 1], l.color, width: width);
        }
      }
    }
  }

  /// The tool's handles, on top of everything: the dragged or hovered one
  /// in the highlight colour, the others dimmed while a drag is on.
  void _drawHandles(ViewportOverlay o) {
    final active = drag?.handle.id ?? hoverHandle;
    final highlight = GizmoTokens.linear(GizmoTokens.highlight);
    for (final h in handles()) {
      final base = h.id == active ? highlight : h.color;
      final dim = drag != null && h.id != active ? 0.35 : 1.0;
      List<double> alpha(double a) => [base[0], base[1], base[2], base[3] * a * dim];
      final width =
          switch (h.width) {
            HandleWidth.handle => GizmoTokens.handleWidth,
            HandleWidth.line => GizmoTokens.lineWidth,
            HandleWidth.dot => GizmoTokens.dotSize,
          } *
          pixelRatio;
      final line = alpha(1);
      for (final l in h.shape.lines) {
        for (var i = 0; i + 1 < l.length; i++) {
          o.line(l[i], l[i + 1], line, width: width, onTop: true);
        }
      }
      final back = alpha(0.3);
      for (final l in h.shape.back) {
        for (var i = 0; i + 1 < l.length; i++) {
          o.line(l[i], l[i + 1], back, width: GizmoTokens.lineWidth * pixelRatio, onTop: true);
        }
      }
      if (drag?.component != null) {
        final guide = [h.color[0], h.color[1], h.color[2], h.color[3]];
        for (final l in h.shape.guides) {
          for (var i = 0; i + 1 < l.length; i++) {
            o.line(l[i], l[i + 1], guide, width: GizmoTokens.lineWidth * pixelRatio, onTop: true);
          }
        }
      }
      final fill = alpha(h.shape.fill);
      for (final t in h.shape.triangles) {
        o.triangle(t[0], t[1], t[2], fill, onTop: true);
      }
    }
  }

  // ── Gizmo ────────────────────────────────────────────────────────────

  /// The selected, unlocked, Transform-bearing entities the gizmo moves
  /// (topmost of the selection), lead first.
  List<String> _dragTargets(SceneTab t) {
    final sel = t.tab.tree.topmost(t.tab.selection);
    // An instance's Transform is its prefab root's: read from its expansion.
    for (final id in sel) {
      if (t.tab.tree[id]?.isPrefabInstance ?? false) t.tab.expansionOf(id);
    }
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
    if (t == null || !t.hasPreview || !tool.hasGizmo || t.tab.selection.isEmpty) return null;
    if (t.tab.document.readOnly != null) return 'read-only: no gizmo';
    if (componentTarget() != null) return null;
    if (_dragTargets(t).isNotEmpty) return null;
    final top = t.tab.tree.topmost(t.tab.selection);
    if (top.every(t.state.locked.contains)) return 'locked (L to unlock)';
    if (top.any((id) => t.tab.tree[id]?.isDerived ?? false)) {
      return 'no Transform component on the prefab\'s entity — add one in the Inspector (an override) to move it';
    }
    return 'no Transform component — add one in the Inspector to move it';
  }

  /// The gizmo's frame for the selection now: pivot at the lead's origin,
  /// world or local axes. Null when the tool has no gizmo, nothing
  /// draggable is selected or the preview is gone.
  GizmoFrame? gizmoFrame() {
    final t = current;
    final vp = viewProjection;
    if (t == null || !t.hasPreview || vp == null || !tool.hasGizmo) return null;
    if (t.tab.document.readOnly != null) return null; // nothing to drag into
    final d = drag;
    if (d != null) return GizmoFrame(pivot: d.currentPivot, axes: d.frame.axes, size: d.frame.size);
    final targets = _dragTargets(t);
    if (targets.isEmpty) return null;
    final lead = targets.first;
    final world = t.preview!.worldMatrices([lead])[lead];
    if (world == null) return null;
    final pivot = matTranslation(world);
    final local = space == GizmoSpace.local || tool.alwaysLocal;
    return GizmoFrame(
      pivot: pivot,
      axes: local ? matAxes(world) : GizmoFrame.worldAxes,
      size: vp.metresPerPixel(pivot) * GizmoFrame.screenSize * pixelRatio,
    );
  }

  ToolContext? _context(GizmoFrame? frame, {bool snapping = false}) {
    final vp = viewProjection;
    if (frame == null || vp == null) return null;
    return ToolContext(frame: frame, vp: vp, snap: snap, snapping: snapping);
  }

  /// The tool's handles on the selection now (during a drag, the dragged
  /// tool's, following the pivot).
  List<Handle> handles() {
    final ct = componentTarget();
    if (ct != null) {
      final d = drag;
      final c = d?.componentContext ?? _componentContext(ct);
      if (c == null) return const [];
      return ct.handler.handles(d?.component == null ? c : c.withData({...ct.data, ...d!.fields}));
    }
    final c = _context(gizmoFrame());
    if (c == null) return const [];
    return (drag?.tool ?? tool).handles(c);
  }

  /// The pivot's pixel, when there is a gizmo.
  Offset? get gizmoPivotPx {
    final f = gizmoFrame();
    final r = f == null ? null : viewProjection?.project(f.pivot);
    return r == null ? null : Offset(r[0], r[1]);
  }

  /// The handle under [px] (physical pixels).
  Handle? handleAt(Offset px) {
    final vp = viewProjection;
    if (vp == null) return null;
    return hitTestHandles([for (final h in handles()) ProjectedHandle(h, vp)], px, tolerance: 8 * pixelRatio);
  }

  /// The pixel where handle [id] is grabbed; null when it is not on screen.
  Offset? grabPoint(String id) {
    final vp = viewProjection;
    for (final h in handles()) {
      if (h.id != id) continue;
      final r = vp?.project(h.grab);
      return r == null ? null : Offset(r[0], r[1]);
    }
    return null;
  }

  /// Starts a drag on handle [id] from [px]; false when nothing is
  /// draggable or the tool has no such handle.
  bool beginDrag(String id, Offset px) {
    final ct = componentTarget();
    if (ct != null) return _beginComponentDrag(ct, id, px);
    final t = current;
    final frame = gizmoFrame();
    final c = _context(frame);
    if (t == null || frame == null || c == null || !canEdit) return false;
    final handle = tool.handles(c).where((h) => h.id == id).firstOrNull;
    final pivot = c.vp.project(frame.pivot);
    if (handle == null || pivot == null) return false;
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
          anchor: node.overrideAnchor,
          entityLocal: node.overrideLocal,
          transformLocal: node.transformLocal,
        ),
      );
    }
    if (targets.isEmpty) return false;
    drag = GizmoDrag(
      tool: tool,
      handle: handle,
      startPx: px,
      frame: frame,
      pivotPx: Offset(pivot[0], pivot[1]),
      targets: targets,
    )..currentPivot = List.of(frame.pivot);
    _mode = _Mode.gizmo;
    notifyListeners();
    return true;
  }

  /// The pointer moved to [px] during a drag: the handle's constraint
  /// reads the motion, the tool turns it into an edit, and the targets'
  /// transforms are previewed in the world (no document change).
  /// [invertSnap] (⌘ held) flips the snap setting for this move.
  void updateDrag(Offset px, {bool invertSnap = false}) {
    final d = drag;
    final t = current;
    final vp = viewProjection;
    if (d == null || t == null || vp == null) return;
    if ((px - d.startPx).distance < 2 && !d.moved) return;
    d.moved = true;
    final value = d.handle.constraint.solve(
      DragInput(
        startPx: d.startPx,
        px: px,
        startRay: vp.rayAt(d.startPx.dx, d.startPx.dy),
        ray: vp.rayAt(px.dx, px.dy),
        pivotPx: d.pivotPx,
        cameraForward: t.camera.forward,
      ),
    );
    final component = d.component;
    if (component != null) {
      final e = value == null ? null : component.handler.edit(d.handle, value, d.componentContext!);
      if (e == null) return;
      d.fields = e.fields;
      d.readout = e.readout;
      _repaint();
      return;
    }
    final c = ToolContext(frame: d.frame, vp: vp, snap: snap, snapping: snap.enabled != invertSnap);
    final edit = value == null ? null : d.tool.edit(d.handle, value, c);
    if (edit == null) return;
    d.readout = edit.readout;
    d.currentPivot = edit.pivot;
    final delta = edit.delta;
    final previews = <PreviewTransform>[];
    for (final target in d.targets) {
      final newWorld = matMul(delta, target.startWorld);
      final local = Trs.fromMatrix(matMul(matInverseAffine(target.parentWorld), newWorld));
      // A pure move keeps the rotation and scale bits exactly (no
      // decomposition noise on what did not change).
      final trs = switch (edit.channel) {
        TransformChannel.move => Trs(
          position: local.position,
          rotation: target.startLocal.rotation,
          scale: target.startLocal.scale,
        ),
        TransformChannel.rotate => Trs(
          position: local.position,
          rotation: local.rotation,
          scale: target.startLocal.scale,
        ),
        TransformChannel.scale => local,
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

  /// The drag ends: the previewed transforms become one transaction of
  /// `setField`s (only the fields that changed) — for a derived entity of
  /// a prefab instance or the instance itself, `setField` overrides on the
  /// instance (M4-07, ADR-0022); a drag
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
    if (d.component != null) return _endComponentDrag(t, d);
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

  /// The Inspector's label drag (Unity's live scrub): [locals] shown in
  /// [tab]'s world with no transaction; null puts [ids] back to the
  /// document (the release's transaction then syncs the world). Only a
  /// tab whose world is open follows.
  void previewLocals(OpenDocument tab, Map<String, Trs>? locals, Iterable<String> ids) {
    final t = _sceneTabs[tab];
    if (t == null || !t.hasPreview || isLive) return;
    try {
      if (locals == null) {
        t.preview!.restoreTransforms(ids);
      } else {
        t.preview!.setTransforms([
          for (final e in locals.entries)
            PreviewTransform(id: e.key, position: e.value.position, rotation: e.value.rotation, scale: e.value.scale),
        ]);
      }
    } on AuthoringException catch (e) {
      notice = 'Preview: $e';
    }
    views?.changed(this, t.preview);
    _repaint();
  }

  /// Escape: the world back to the document, nothing written.
  void cancelDrag() {
    final d = drag;
    final t = current;
    drag = null;
    if (_mode == _Mode.gizmo) _mode = _Mode.none;
    if (d != null && d.component == null && t != null && t.hasPreview) {
      try {
        t.preview!.restoreTransforms(d.ids);
      } on AuthoringException catch (_) {}
      views?.changed(this, t.preview);
    }
    _repaint();
  }

  // ── Component handles (item 9g) ─────────────────────────────────────

  ({String key, ComponentTarget? target})? _componentCache;

  /// The component the Inspector's Edit toggle is on, when the Scene view
  /// can edit it: on the lead entity, unlocked and shown, in a writable
  /// document, with handles for its data (not a mesh collider). Its data
  /// is read once per document revision.
  ComponentTarget? componentTarget() {
    final t = current;
    final type = t?.tab.editingType;
    if (t == null || type == null || !t.hasPreview || t.tab.document.readOnly != null) return null;
    final id = t.tab.lead!;
    final handler = handlersFor(type);
    if (handler == null || t.state.locked.contains(id) || _hiddenAll(t).contains(id)) return null;
    final key = '${t.tab.document.revision}|$id|$type';
    var cached = _componentCache;
    if (cached == null || cached.key != key) {
      cached = _componentCache = (key: key, target: _readComponent(t, id, type, handler));
    }
    final target = cached.target;
    if (target == null) return null;
    final world = t.preview!.worldMatrices([id])[id];
    if (world == null) return null;
    return ComponentTarget(
      entityId: target.entityId,
      componentId: target.componentId,
      type: type,
      data: target.data,
      world: world,
      handler: handler,
      anchor: target.anchor,
      entityLocal: target.entityLocal,
      local: target.local,
    );
  }

  ComponentTarget? _readComponent(SceneTab t, String id, String type, ComponentHandles handler) {
    final node = t.tab.tree[id];
    if (node == null) return null;
    final anchor = node.overrideAnchor ?? (node.isPrefabInstance ? id : null);
    if (anchor != null) {
      final de = t.tab.expansionOf(anchor)?.entity(id);
      if (de == null) return null;
      for (final c in de.components.entries) {
        if (c.value.type != type) continue;
        final data = Map<String, Object?>.from(c.value.data);
        if (!handler.supports(data)) return null;
        return ComponentTarget(
          entityId: id,
          componentId: c.key,
          type: type,
          data: data,
          world: identityMatrix,
          handler: handler,
          anchor: anchor,
          entityLocal: de.local,
          local: c.value.local,
        );
      }
      return null;
    }
    try {
      final components = t.tab.document.entities([id]).entities[id]?['components'];
      if (components is! Map) return null;
      for (final e in components.entries) {
        final c = e.value;
        if (c is! Map || c['type'] != type) continue;
        final data = c['data'] is Map ? Map<String, Object?>.from(c['data'] as Map) : <String, Object?>{};
        if (!handler.supports(data)) return null;
        return ComponentTarget(
          entityId: id,
          componentId: e.key as String,
          type: type,
          data: data,
          world: identityMatrix,
          handler: handler,
        );
      }
    } on AuthoringException catch (_) {}
    return null;
  }

  ComponentContext? _componentContext(ComponentTarget ct) {
    final vp = viewProjection;
    if (vp == null) return null;
    return ComponentContext(world: ct.world, data: ct.data, vp: vp, pixelRatio: pixelRatio, aspect: aspect);
  }

  bool _beginComponentDrag(ComponentTarget ct, String id, Offset px) {
    final c = _componentContext(ct);
    if (c == null || !canEdit) return false;
    final handle = ct.handler.handles(c).where((h) => h.id == id).firstOrNull;
    if (handle == null) return false;
    final origin = c.vp.project(c.origin);
    drag =
        GizmoDrag(
            tool: tool,
            handle: handle,
            startPx: px,
            frame: GizmoFrame(pivot: c.origin, axes: GizmoFrame.worldAxes, size: 1),
            pivotPx: origin == null ? px : Offset(origin[0], origin[1]),
            targets: const [],
          )
          ..currentPivot = c.origin
          ..component = ct
          ..componentContext = c;
    _mode = _Mode.gizmo;
    notifyListeners();
    return true;
  }

  /// A component handle's drag ends: the fields it changed, one
  /// transaction (`setField`s, or `setField` overrides on a derived
  /// component's anchor).
  Applied? _endComponentDrag(SceneTab t, GizmoDrag d) {
    final ct = d.component!;
    final commands = <Map<String, Object?>>[
      for (final e in d.fields.entries)
        if (d.moved && '${ct.data[e.key]}' != '${e.value}')
          ct.isDerived
              ? Command.setPrefabOverrideOp(
                  ct.anchor!,
                  prefab.setFieldOverride(
                    entity: ct.entityLocal ?? ct.entityId,
                    component: ct.local ?? ct.componentId,
                    data: ct.data,
                    path: e.key,
                    value: e.value,
                  ),
                )
              : Command.setField(ct.entityId, ct.componentId, e.key, e.value),
    ];
    if (commands.isEmpty) {
      _repaint();
      return null;
    }
    final a = t.tab.transaction(commands);
    notice = a == null || !a.ok ? (t.tab.lastError ?? 'The edit was refused') : null;
    _repaint();
    return a;
  }

  /// What the handles edit, for the status line ("Editing Collider").
  String? get editingLabel {
    final ct = componentTarget();
    if (ct == null) return null;
    final dot = ct.type.lastIndexOf('.');
    return ct.type.substring(dot + 1);
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
    _stopCoast();
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
      final handle = handleAt(px);
      if (handle != null && canEdit && beginDrag(handle.id, px)) {
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
    final h = drag == null ? handleAt(px)?.id : null;
    if (h != hoverHandle) {
      hoverHandle = h;
      notifyListeners();
    }
  }

  /// A mouse wheel: dolly; with ⇧ pan.
  void pointerSignal(PointerSignalEvent e, Offset px) {
    if (e is! PointerScrollEvent || !hasView) return;
    pointer = px;
    _stopCoast();
    _scroll(e.scrollDelta);
  }

  /// A scroll of [delta] (a wheel's direction: +y is away from the user);
  /// [panning] defaults to ⇧ held now.
  void _scroll(Offset delta, {bool? panning}) {
    if (panning ?? _shiftHeld) {
      pan(-delta.dx, -delta.dy);
    } else {
      _dollyAtPointer(-delta.dy / 40);
    }
  }

  // ── Trackpad ─────────────────────────────────────────────────────────
  //
  // On macOS a two-finger scroll and a pinch are not wheel events: they
  // arrive as one pan-zoom gesture whose pan and scale are totals since it
  // began. A scroll dollies (⇧ pans) exactly as the wheel does; a pinch
  // dollies by its own ratio — spread the fingers and the view comes closer.
  //
  // Flutter drops the system's momentum events, so the scroll coasts here:
  // the fingers' velocity at lift-off keeps scrolling and decays, as a
  // scrolled list does. Touching the trackpad again, a wheel or a click
  // stops it.

  Offset _gesturePan = Offset.zero;
  double _gestureScale = 1;
  final _gestureSamples = <(Duration, Offset)>[];
  Timer? _coastTimer;

  /// The coast's decay per second (UIScrollView's normal rate) and the
  /// speed, in logical pixels per second, where it stops.
  static const _coastDecay = 2.0;
  static const _coastStop = 20.0;

  void panZoomStart(PointerPanZoomStartEvent e, Offset px) {
    pointer = px;
    _stopCoast();
    _gesturePan = Offset.zero;
    _gestureScale = 1;
    _gestureSamples
      ..clear()
      ..add((e.timeStamp, Offset.zero));
  }

  void panZoomUpdate(PointerPanZoomUpdateEvent e, Offset px) {
    if (!hasView) return;
    pointer = px;
    final pan = e.pan - _gesturePan;
    final ratio = e.scale / _gestureScale;
    _gesturePan = e.pan;
    _gestureScale = e.scale;
    _gestureSamples.add((e.timeStamp, e.pan));
    // The velocity reads the last 100 ms.
    while (_gestureSamples.length > 2 && e.timeStamp - _gestureSamples.first.$1 > const Duration(milliseconds: 100)) {
      _gestureSamples.removeAt(0);
    }
    if (ratio.isFinite && ratio > 0 && ratio != 1) _dollyAtPointer(math.log(ratio) * 10);
    // A finger movement is the opposite of the scroll it makes.
    if (pan != Offset.zero) _scroll(-pan);
  }

  void panZoomEnd(PointerPanZoomEndEvent e, Offset px) {
    final pinched = _gestureScale != 1;
    final v = _liftVelocity(e.timeStamp);
    _gesturePan = Offset.zero;
    _gestureScale = 1;
    _gestureSamples.clear();
    // A pinch lands where the fingers leave it.
    if (!pinched && hasView && v.distance > _coastStop) _coast(-v, panning: _shiftHeld);
  }

  /// The fingers' velocity (logical pixels per second) over the last
  /// samples; zero when they rested before lifting.
  Offset _liftVelocity(Duration end) {
    if (_gestureSamples.length < 2) return Offset.zero;
    final (t1, p1) = _gestureSamples.last;
    if (end - t1 > const Duration(milliseconds: 50)) return Offset.zero;
    final (t0, p0) = _gestureSamples.first;
    final dt = (t1 - t0).inMicroseconds / 1e6;
    return dt > 0 ? (p1 - p0) / dt : Offset.zero;
  }

  /// Keep scrolling at [velocity] (logical pixels per second, a scroll's
  /// direction), decaying; [panning] holds the mode the gesture ended in.
  void _coast(Offset velocity, {required bool panning}) {
    _stopCoast();
    var v = velocity;
    const dt = 0.016; // a tick, as the fly's
    _coastTimer = Timer.periodic(const Duration(milliseconds: 16), (_) {
      if (!hasView || v.distance < _coastStop) {
        _stopCoast();
        return;
      }
      _scroll(v * dt, panning: panning);
      v *= math.exp(-_coastDecay * dt);
    });
  }

  void _stopCoast() {
    _coastTimer?.cancel();
    _coastTimer = null;
  }

  void _cancelInteraction() {
    if (drag != null) cancelDrag();
    _stopFly();
    _stopCoast();
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

  /// The Scene view's own keys while it has focus: Esc cancel/deselect,
  /// ⌫/⌦ delete, WASDQE fly while the right button is held. The rest —
  /// Q/W/E/R tools, X local/world, F / ⇧F frame, H / ⇧H hide, L lock, 5
  /// projection — are the registry's bare keys and work from any panel;
  /// they fall through to it, as anything with ⌘ does.
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
    if (key == LogicalKeyboardKey.escape) {
      if (drag != null) {
        cancelDrag();
      } else if (current?.tab.editingType != null) {
        current!.tab.editComponent(null);
      } else if (marquee != null) {
        marquee = null;
        _mode = _Mode.none;
        notifyListeners();
      } else {
        current?.tab.clearSelection();
      }
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
    _stopCoast();
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
