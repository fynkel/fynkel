// The Animator window (ADR-0032 § 8), Unity's: the controller's layers as
// tabs along the top (+ adds one), its parameters on the left (+ Float,
// Int, Bool or Trigger; a name edited in place renames it everywhere), and
// the layer's state machine as a graph — a box per state (the layer's
// default in orange), Entry and Any State, an arrow per pair of states
// with transitions (three heads when there are several).
//
// Drag a box to move it, drag the empty graph to look around. Right-click
// the empty graph for Create State, a state for Make Transition (then
// click the target), Set as Layer Default State, Rename and Delete. Drop a
// clip or a blend space from the Project to make a state that plays it. A
// click selects a state or an arrow and shows it in the Inspector; ⌫
// deletes it; F frames the graph. Every change is one undo step on the
// controller (⌘Z while the window holds the keyboard).
import 'dart:math' as math;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart' show Ticker;
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart' as fynkel;

import '../inspector/asset_edits.dart' show unset;
import '../inspector/field_widgets.dart';
import '../inspector/inspector_model.dart' show describeAssetRef;
import '../inspector/inspector_chrome.dart';
import '../project/editor_project.dart' show AssetEntry;
import '../theme/icons.dart';
import '../theme/panel_focus.dart';
import '../theme/tokens.dart';
import 'animator_ik.dart';
import 'animator_model.dart';
import 'controller_edits.dart';

/// A state's box, and Entry's and Any State's (smaller).
const stateSize = Size(150, 36);
const nodeSize = Size(110, 30);

/// Unity's colours: the default state orange, Entry green, Any State teal.
const defaultStateColor = Color(0xFFC86A1E);
const entryColor = Color(0xFF3F8E3F);
const anyColor = Color(0xFF2F8C8C);

class AnimatorPanel extends StatefulWidget {
  const AnimatorPanel({super.key, required this.model, this.assets});

  final AnimatorModel model;

  /// The project's assets (the motion pickers of Create State).
  final List<AssetEntry> Function()? assets;

  @override
  State<AnimatorPanel> createState() => _AnimatorPanelState();
}

class _AnimatorPanelState extends State<AnimatorPanel> with SingleTickerProviderStateMixin {
  /// During Play: a frame's read of the running animator (the progress
  /// bars fill as the game plays).
  late final Ticker _ticker = createTicker((_) {
    if (model.live == null) {
      _ticker.stop();
    } else {
      setState(() {});
    }
  });

  final _focus = FocusNode(debugLabel: 'animator.graph');

  /// Graph → screen: a box at graph point p is drawn at p × _scale + _pan.
  Offset _pan = Offset.zero;
  double _scale = 1;

  static const _minScale = 0.3;
  static const _maxScale = 1.5;

  Offset _toScreen(Offset p) => p * _scale + _pan;
  Offset _toGraph(Offset screen) => (screen - _pan) / _scale;

  /// Zooms by [factor] about the screen point [focus] (it stays put).
  void _zoom(double factor, Offset focus) {
    final next = (_scale * factor).clamp(_minScale, _maxScale);
    final g = _toGraph(focus);
    _scale = next;
    _pan = focus - g * _scale;
  }

  double? _pinchStart;

  /// What [_pan] was framed for (controller and layer); a new one frames again.
  (String?, int, String)? _framed;

  /// Boxes being dragged: their place so far (written on release).
  final _dragged = <String, Offset>{};

  /// The pointer, while Make Transition waits for its target.
  Offset? _pointer;

  /// The state whose box shows a name field (Rename).
  String? _renaming;
  TextEditingController? _renameText;

  Size _view = Size.zero;

  AnimatorModel get model => widget.model;

  @override
  void dispose() {
    _ticker.dispose();
    _focus.dispose();
    _renameText?.dispose();
    super.dispose();
  }

  // ── Geometry ────────────────────────────────────────────────────────────

  Offset _at(ControllerEdits c, String key) => _dragged[key] ?? c.positionOf(model.layer, key);

  Rect _box(ControllerEdits c, String key) => Rect.fromCenter(
    center: _toScreen(_at(c, key)),
    width: _sizeOf(key).width * _scale,
    height: _sizeOf(key).height * _scale,
  );

  Size _sizeOf(String key) => key == entryKey || key == anyKey ? nodeSize : stateSize;

  /// Fits the layer's boxes in the view (Unity's Frame): centred, and
  /// smaller when they do not fit at full size.
  void _frame(ControllerEdits c) {
    final keys = [
      entryKey,
      if (model.machine.isEmpty) anyKey,
      for (final s in c.states(model.layer)) s['name'] as String,
    ];
    Rect boxOf(String k) =>
        Rect.fromCenter(center: c.positionOf(model.layer, k), width: _sizeOf(k).width, height: _sizeOf(k).height);
    var bounds = boxOf(keys.first);
    for (final k in keys) {
      bounds = bounds.expandToInclude(boxOf(k));
    }
    bounds = bounds.inflate(24);
    _scale = [1.0, _view.width / bounds.width, _view.height / bounds.height].reduce(math.min).clamp(_minScale, 1.0);
    _pan = _view.center(Offset.zero) - bounds.center * _scale;
  }

  /// The arrows: one per ordered pair of boxes, with the transitions' indices.
  Map<(String, String), List<int>> _pairs(ControllerEdits c) {
    final out = <(String, String), List<int>>{};
    final ts = c.transitions(model.layer);
    for (var i = 0; i < ts.length; i++) {
      final from = ts[i]['from'] == 'any' ? anyKey : ts[i]['from'] as String? ?? '';
      final to = ts[i]['to'] as String? ?? '';
      if (from != anyKey && c.state(model.layer, from) == null) continue;
      if (c.state(model.layer, to) == null) continue;
      out.putIfAbsent((from, to), () => []).add(i);
    }
    return out;
  }

  /// An arrow's ends on screen: centre to centre, set aside when the pair
  /// has arrows both ways so the two do not overlap.
  (Offset, Offset) _ends(ControllerEdits c, String from, String to, bool both) {
    var a = _toScreen(_at(c, from));
    var b = _toScreen(_at(c, to));
    if (both) {
      final d = b - a;
      final n = d.distance == 0 ? Offset.zero : Offset(-d.dy, d.dx) / d.distance * 6;
      a += n;
      b += n;
    }
    return (a, b);
  }

  /// The transition under [p] (screen), or null.
  int? _transitionAt(ControllerEdits c, Offset p) {
    final pairs = _pairs(c);
    for (final e in pairs.entries) {
      final (from, to) = e.key;
      final (a, b) = _ends(c, from, to, pairs.containsKey((to, from)));
      if (_distanceToSegment(p, a, b) < 6) return e.value.first;
    }
    return null;
  }

  static double _distanceToSegment(Offset p, Offset a, Offset b) {
    final ab = b - a;
    final len2 = ab.dx * ab.dx + ab.dy * ab.dy;
    if (len2 == 0) return (p - a).distance;
    final t = (((p - a).dx * ab.dx + (p - a).dy * ab.dy) / len2).clamp(0.0, 1.0);
    return (p - (a + ab * t)).distance;
  }

  // ── Gestures ────────────────────────────────────────────────────────────

  void _tapGraph(ControllerEdits c, Offset p) {
    _focus.requestFocus();
    if (model.linkFrom != null) {
      model.cancelLink();
      return;
    }
    final t = _transitionAt(c, p);
    t == null ? model.clearSelection() : model.selectTransition(t);
  }

  void _tapBox(String key) {
    _focus.requestFocus();
    if (model.linkFrom != null) {
      if (key != entryKey && key != anyKey) model.link(key);
      return;
    }
    if (key == entryKey || key == anyKey) {
      model.clearSelection();
    } else {
      model.selectState(key);
    }
  }

  void _endDrag(ControllerEdits c) {
    if (_dragged.isEmpty) return;
    final moved = Map.of(_dragged);
    _dragged.clear();
    c.place(model.layer, moved);
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    if (e is! KeyDownEvent) return KeyEventResult.ignored;
    final c = model.controller;
    final k = e.logicalKey;
    if (k == LogicalKeyboardKey.backspace || k == LogicalKeyboardKey.delete) {
      return model.deleteSelected() ? KeyEventResult.handled : KeyEventResult.ignored;
    }
    if (k == LogicalKeyboardKey.escape) {
      model.linkFrom != null ? model.cancelLink() : model.clearSelection();
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.keyF && c != null && !HardwareKeyboard.instance.isMetaPressed) {
      setState(() => _frame(c));
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  Future<String?> _menu(Offset global, List<PopupMenuEntry<String>> items) {
    final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
    return showMenu<String>(
      popUpAnimationStyle: AnimationStyle.noAnimation,
      context: context,
      position: RelativeRect.fromRect(Rect.fromPoints(global, global), Offset.zero & overlay.size),
      items: items,
    );
  }

  PopupMenuItem<String> _item(String value, String label, {bool enabled = true}) => PopupMenuItem(
    key: Key('animator.menu.$value'),
    value: value,
    enabled: enabled,
    height: 30,
    child: Text(label, style: Theme.of(context).textTheme.bodySmall),
  );

  Future<void> _graphMenu(ControllerEdits c, Offset local, Offset global) async {
    final t = _transitionAt(c, local);
    if (t != null) {
      model.selectTransition(t);
      final choice = await _menu(global, [_item('deleteTransition', 'Delete', enabled: c.canEdit)]);
      if (choice == 'deleteTransition') model.deleteTransition(t);
      return;
    }
    final choice = await _menu(global, [
      _item('state.empty', 'Create State ▸ Empty', enabled: c.canEdit),
      _item('machine', 'Create Sub-State Machine', enabled: c.canEdit),
      _item('state.clip', 'Create State ▸ From Clip…', enabled: c.canEdit && widget.assets != null),
      _item('state.blend', 'Create State ▸ From Blend Space…', enabled: c.canEdit && widget.assets != null),
      _item('state.tree', 'Create State ▸ From New Blend Tree…', enabled: c.canEdit && widget.assets != null),
      const PopupMenuDivider(),
      _item('frame', 'Frame All  F'),
    ]);
    if (!mounted) return;
    final at = _toGraph(local);
    switch (choice) {
      case 'state.empty':
        model.createState(at);
      case 'machine':
        model.createMachine(at);
      case 'state.clip' || 'state.blend' || 'state.tree':
        // A new blend tree starts with a clip (a tree is never empty).
        final kind = choice == 'state.blend' ? 'blend' : 'clip';
        final picked = await pickAssetRef(
          context,
          kind: kind,
          subject: 'Motion',
          candidates: () => [
            for (final a in widget.assets!())
              if (a.offeredKinds.contains(kind)) a,
          ],
        );
        if (picked?.ref != null) model.createState(at, motion: picked!.ref, tree: choice == 'state.tree');
      case 'frame':
        setState(() => _frame(c));
    }
  }

  Future<void> _boxMenu(ControllerEdits c, String key, Offset global) async {
    if (key == entryKey) return;
    if (key == anyKey) {
      final choice = await _menu(global, [_item('link', 'Make Transition', enabled: c.canEdit)]);
      if (choice == 'link') model.startLink('any');
      return;
    }
    model.selectState(key);
    final machine = ControllerEdits.isMachine(c.state(model.layer, key));
    final choice = await _menu(global, [
      if (machine) _item('open', 'Open'),
      _item('link', 'Make Transition', enabled: c.canEdit),
      _item(
        'default',
        model.machine.isEmpty ? 'Set as Layer Default State' : 'Set as Default State',
        enabled: c.canEdit && c.defaultState(model.layer) != key,
      ),
      const PopupMenuDivider(),
      _item('rename', 'Rename', enabled: c.canEdit),
      _item('delete', 'Delete', enabled: c.canEdit),
    ]);
    switch (choice) {
      case 'open':
        model.enterMachine(key);
      case 'link':
        model.startLink(key);
      case 'default':
        c.setDefault(model.layer, key);
      case 'rename':
        setState(() {
          _renaming = key;
          _renameText?.dispose();
          _renameText = TextEditingController(text: key)
            ..selection = TextSelection(baseOffset: 0, extentOffset: key.length);
        });
      case 'delete':
        model.deleteState(key);
    }
  }

  void _commitRename(String to) {
    final from = _renaming;
    setState(() => _renaming = null);
    if (from != null && to.trim().isNotEmpty) model.renameState(from, to.trim());
    _focus.requestFocus();
  }

  // ── Build ───────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    PanelFocus.primary(context, _focus);
    return ListenableBuilder(
      listenable: model,
      builder: (context, _) {
        final c = model.controller;
        final root = c?.root;
        if (c == null || root == null) {
          return const InspectorEmpty(
            key: Key('animator.empty'),
            icon: FIcons.animator,
            title: 'No Animator Controller',
            message:
                'Select an object whose Animator has a controller, or an Animator Controller in the Project.\n'
                'Make one with Create ▸ Animator Controller.',
          );
        }
        final t = context.tokens;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _toolbar(context, t, c, root),
            Expanded(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  SizedBox(
                    width: 230,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        _sideTabs(context, t),
                        Expanded(
                          child: _side == 'ik'
                              ? IkList(model: model, controller: c)
                              : _ParameterList(model: model, controller: c, live: model.live),
                        ),
                      ],
                    ),
                  ),
                  VerticalDivider(width: 1, color: t.line),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        if (model.layer > 0) _LayerSettings(model: model, controller: c, layer: model.layer),
                        _SlotRow(
                          name: c.layer(model.layer)?['name'] as String? ?? '',
                          empty: c.inMachine(const []).states(model.layer).isEmpty,
                          info: model.live?.layers.where((s) => s.layer == model.layer).firstOrNull,
                          assets: widget.assets,
                        ),
                        if (model.machine.isNotEmpty) _pathBar(context, t, c),
                        Expanded(child: _graph(context, t, c)),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ],
        );
      },
    );
  }

  /// Unity's tabs over the left column: Parameters, and IK (ADR-0033 § 5).
  String _side = 'parameters';

  Widget _sideTabs(BuildContext context, FynkelTokens t) {
    final small = Theme.of(context).textTheme.bodySmall;
    Widget tab(String id, String label) {
      final on = _side == id;
      return InkWell(
        key: Key('animator.tab.$id'),
        onTap: () => setState(() => _side = id),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 10),
          alignment: Alignment.center,
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: on ? t.accent : Colors.transparent, width: 2)),
          ),
          child: Text(label, style: small?.copyWith(color: on ? t.text : t.textSecondary)),
        ),
      );
    }

    return Container(
      height: 24,
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      child: Row(children: [tab('parameters', 'Parameters'), tab('ik', 'IK')]),
    );
  }

  Widget _toolbar(BuildContext context, FynkelTokens t, ControllerEdits c, Map<String, Object?> root) {
    final theme = Theme.of(context);
    final layers = c.layers;
    return Container(
      height: t.toolbar,
      padding: const EdgeInsets.symmetric(horizontal: 3),
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      child: Row(
        children: [
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: Row(
                children: [
                  for (var i = 0; i < layers.length; i++)
                    _layerTab(context, t, c, i, layers[i]['name'] as String? ?? ''),
                  IconButton(
                    key: const Key('animator.addLayer'),
                    tooltip: 'Add a layer',
                    icon: const Icon(FIcons.add, size: 14),
                    onPressed: c.canEdit ? model.addLayer : null,
                  ),
                ],
              ),
            ),
          ),
          Icon(FIcons.animator, size: 13, color: t.textSecondary),
          const SizedBox(width: 4),
          Text(
            root['name'] as String? ?? '',
            key: const Key('animator.name'),
            style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
          ),
          const SizedBox(width: 6),
        ],
      ),
    );
  }

  Widget _layerTab(BuildContext context, FynkelTokens t, ControllerEdits c, int i, String name) {
    final on = i == model.layer;
    return GestureDetector(
      onSecondaryTapUp: i == 0 || !c.canEdit
          ? null
          : (d) async {
              model.layer = i;
              final choice = await _menu(d.globalPosition, [
                _item('renameLayer', 'Rename'),
                _item('deleteLayer', 'Delete'),
              ]);
              if (!mounted) return;
              if (choice == 'deleteLayer') model.deleteLayer(i);
              if (choice == 'renameLayer') {
                final to = await _askName(this.context, 'Rename Layer', name);
                if (to != null) c.renameLayer(i, to);
              }
            },
      child: InkWell(
        key: Key('animator.layer.$i'),
        onTap: () => model.layer = i,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 10),
          height: t.toolbar,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: on ? t.accent : Colors.transparent, width: 2)),
          ),
          child: Text(
            name,
            style: Theme.of(context).textTheme.bodySmall?.copyWith(color: on ? t.text : t.textSecondary),
          ),
        ),
      ),
    );
  }

  /// Where the graph is (ADR-0033 § 4): "Base Layer ▸ Airborne"; a click
  /// goes back up.
  Widget _pathBar(BuildContext context, FynkelTokens t, ControllerEdits c) {
    final small = Theme.of(context).textTheme.bodySmall;
    final names = [c.layer(model.layer)?['name'] as String? ?? '', ...model.machine];
    return Container(
      key: const Key('animator.path'),
      height: 24,
      padding: const EdgeInsets.symmetric(horizontal: 6),
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      child: Row(
        children: [
          for (final (i, name) in names.indexed) ...[
            if (i > 0) Icon(FIcons.chevronRight, size: 12, color: t.textSecondary),
            InkWell(
              key: Key('animator.path.$i'),
              onTap: i == names.length - 1 ? null : () => model.openMachine(i),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
                child: Text(
                  name,
                  style: small?.copyWith(color: i == names.length - 1 ? t.text : t.accent),
                ),
              ),
            ),
          ],
        ],
      ),
    );
  }

  /// A running state's path (`Airborne/Fall`) as the box of the graph
  /// shown that holds it: the state, or the sub-state machine it is in;
  /// null when it is outside the machine shown.
  String? _boxOf(String? path) {
    if (path == null) return null;
    final prefix = model.machine.isEmpty ? '' : '${model.machine.join('/')}/';
    if (!path.startsWith(prefix)) return null;
    return path.substring(prefix.length).split('/').first;
  }

  Widget _graph(BuildContext context, FynkelTokens t, ControllerEdits c) {
    final layer = model.layer;
    final states = c.states(layer);
    final def = c.defaultState(layer);
    return LayoutBuilder(
      builder: (context, box) {
        _view = box.biggest;
        final key = (model.id, layer, model.machine.join('/'));
        if (_framed != key && _view.width > 0) {
          _framed = key;
          _frame(c);
        }
        final pairs = _pairs(c);
        // During Play: the layer's running state, and the one it fades to.
        final live = model.live;
        if (live != null && !_ticker.isActive) _ticker.start();
        final info = live?.layers.where((s) => s.layer == layer).firstOrNull;
        double progressOf(String name, double t) =>
            c.state(layer, name)?['loop'] == false ? t.clamp(0.0, 1.0) : t - t.floorToDouble();
        // A state inside a sub-state machine lights the machine's box.
        final progress = <String, double>{};
        final (now, next) = (_boxOf(info?.name), _boxOf(info?.next));
        if (info != null) {
          if (now != null) progress[now] = progressOf(now, info.normalizedTime);
          if (next != null) progress[next] = progressOf(next, info.nextNormalizedTime);
        }
        // The transition being taken: from the state, else from Any State.
        final taking = switch ((now, next)) {
          (final a?, final b?) when a != b => pairs.containsKey((a, b)) ? (a, b) : (anyKey, b),
          (null, final b?) => (anyKey, b),
          _ => null,
        };
        return Focus(
          focusNode: _focus,
          onKeyEvent: _onKey,
          child: DragTarget<AssetDragPayload>(
            onWillAcceptWithDetails: (d) => c.canEdit && (d.data.kind == 'clip' || d.data.kind == 'blend'),
            onAcceptWithDetails: (d) {
              final local = (context.findRenderObject() as RenderBox).globalToLocal(d.offset);
              model.createState(_toGraph(local), motion: d.data.toRef());
            },
            builder: (context, candidates, _) => MouseRegion(
              onHover: model.linkFrom == null ? null : (e) => setState(() => _pointer = e.localPosition),
              child: Listener(
                // As in Unity: the wheel zooms about the pointer; on a
                // trackpad two fingers move the graph and a pinch zooms.
                onPointerSignal: (e) {
                  if (e is PointerScrollEvent) {
                    setState(() => _zoom(math.pow(0.998, e.scrollDelta.dy).toDouble(), e.localPosition));
                  }
                },
                onPointerPanZoomStart: (_) => _pinchStart = _scale,
                onPointerPanZoomUpdate: (e) => setState(() {
                  _pan += e.panDelta;
                  final start = _pinchStart;
                  if (start != null && e.scale != 1) _zoom(start * e.scale / _scale, e.localPosition);
                }),
                onPointerPanZoomEnd: (_) => _pinchStart = null,
                child: ClipRect(
                  child: Stack(
                    key: const Key('animator.graph'),
                    children: [
                      Positioned.fill(
                        child: GestureDetector(
                          behavior: HitTestBehavior.opaque,
                          onTapUp: (d) => _tapGraph(c, d.localPosition),
                          onSecondaryTapUp: (d) => _graphMenu(c, d.localPosition, d.globalPosition),
                          onPanUpdate: (d) => setState(() => _pan += d.delta),
                          child: CustomPaint(
                            painter: _GraphPainter(
                              tokens: t,
                              pan: _pan,
                              scale: _scale,
                              arrows: [
                                for (final e in pairs.entries)
                                  () {
                                    final (from, to) = e.key;
                                    final (a, b) = _ends(c, from, to, pairs.containsKey((to, from)));
                                    return _Arrow(
                                      a,
                                      b,
                                      e.value.length,
                                      e.value.contains(model.selectedTransition),
                                      active: e.key == taking ? info!.transitionProgress : null,
                                    );
                                  }(),
                              ],
                              entry: def == null || c.state(layer, def) == null
                                  ? null
                                  : _Arrow(_toScreen(_at(c, entryKey)), _toScreen(_at(c, def)), 1, false),
                              link: switch ((model.linkFrom, _pointer)) {
                                (final from?, final p?) => (_toScreen(_at(c, from == 'any' ? anyKey : from)), p),
                                _ => null,
                              },
                              drop: candidates.isNotEmpty,
                            ),
                          ),
                        ),
                      ),
                      _node(context, t, c, entryKey, 'Entry', entryColor),
                      // Any State belongs to the layer, not to a machine.
                      if (model.machine.isEmpty) _node(context, t, c, anyKey, 'Any State', anyColor),
                      for (final s in states)
                        _node(
                          context,
                          t,
                          c,
                          s['name'] as String,
                          s['name'] as String,
                          s['name'] == def ? defaultStateColor : null,
                          progress: progress[s['name']],
                          tree: s['motion'] is Map && (s['motion'] as Map)['blendTree'] is Map,
                          machine: ControllerEdits.isMachine(s),
                        ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        );
      },
    );
  }

  Widget _node(
    BuildContext context,
    FynkelTokens t,
    ControllerEdits c,
    String key,
    String label,
    Color? fill, {
    double? progress,
    bool tree = false,
    bool machine = false,
  }) {
    final r = _box(c, key);
    final selected = key == model.selectedState;
    final linking = model.linkFrom != null;
    final theme = Theme.of(context);
    final renaming = _renaming == key;
    return Positioned.fromRect(
      rect: r,
      child: MouseRegion(
        cursor: linking && key != entryKey && key != anyKey ? SystemMouseCursors.precise : SystemMouseCursors.basic,
        child: GestureDetector(
          key: Key('animator.state.$key'),
          onTap: () => _tapBox(key),
          // A sub-state machine opens on a double-click (ADR-0033 § 4).
          onDoubleTap: machine && model.linkFrom == null ? () => model.enterMachine(key) : null,
          onSecondaryTapUp: (d) => _boxMenu(c, key, d.globalPosition),
          onPanStart: c.canEdit ? (_) => _focus.requestFocus() : null,
          onPanUpdate: c.canEdit ? (d) => setState(() => _dragged[key] = _at(c, key) + d.delta / _scale) : null,
          onPanEnd: c.canEdit ? (_) => _endDrag(c) : null,
          // Drawn at full size, then scaled with the graph.
          child: FittedBox(
            fit: BoxFit.fill,
            child: SizedBox.fromSize(
              size: _sizeOf(key),
              child: Container(
                alignment: Alignment.center,
                padding: const EdgeInsets.symmetric(horizontal: 8),
                decoration: BoxDecoration(
                  color: fill ?? t.field,
                  // A sub-state machine: rounder, as a stack of states.
                  borderRadius: BorderRadius.circular(machine ? 12 : 5),
                  border: Border.all(color: selected ? t.accent : t.lineStrong, width: selected ? 2 : 1),
                  boxShadow: [
                    BoxShadow(color: t.shadow, blurRadius: 4, offset: const Offset(0, 1)),
                    if (machine) BoxShadow(color: t.lineStrong, offset: const Offset(3, 3)),
                  ],
                ),
                foregroundDecoration: progress == null
                    ? null
                    : _ProgressDecoration(progress: progress, color: t.ok, track: t.window),
                child: renaming
                    ? TextField(
                        key: const Key('animator.rename'),
                        controller: _renameText,
                        autofocus: true,
                        textAlign: TextAlign.center,
                        style: theme.textTheme.bodySmall?.copyWith(color: fill == null ? t.text : Colors.white),
                        decoration: const InputDecoration(isDense: true, border: InputBorder.none),
                        onSubmitted: _commitRename,
                        onTapOutside: (_) => _commitRename(_renameText?.text ?? ''),
                      )
                    : Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (machine)
                            Padding(
                              padding: const EdgeInsets.only(right: 4),
                              child: Icon(
                                FIcons.categoryOutlined,
                                key: Key('animator.state.$key.machine'),
                                size: 11,
                                color: fill == null ? t.textSecondary : Colors.white70,
                              ),
                            ),
                          // A state playing a blend tree (ADR-0033): the Inspector edits it.
                          if (tree)
                            Padding(
                              padding: const EdgeInsets.only(right: 4),
                              child: Icon(
                                FIcons.accountTreeOutlined,
                                key: Key('animator.state.$key.tree'),
                                size: 11,
                                color: fill == null ? t.textSecondary : Colors.white70,
                              ),
                            ),
                          Flexible(
                            child: Text(
                              label,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: theme.textTheme.bodySmall?.copyWith(color: fill == null ? t.text : Colors.white),
                            ),
                          ),
                        ],
                      ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// A name asked for in a small dialog; null when cancelled.
Future<String?> _askName(BuildContext context, String title, String current) {
  final text = TextEditingController(text: current);
  return showDialog<String>(
    context: context,
    animationStyle: AnimationStyle.noAnimation,
    builder: (context) => AlertDialog(
      title: Text(title),
      content: TextField(
        key: const Key('animator.nameField'),
        controller: text,
        autofocus: true,
        onSubmitted: (v) => Navigator.pop(context, v),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
        FilledButton(onPressed: () => Navigator.pop(context, text.text), child: const Text('Rename')),
      ],
    ),
  ).whenComplete(text.dispose);
}

final class _Arrow {
  const _Arrow(this.a, this.b, this.count, this.selected, {this.active});

  /// During Play, the transition being taken: how far its crossfade is.
  final double? active;

  final Offset a;
  final Offset b;
  final int count;
  final bool selected;
}

class _GraphPainter extends CustomPainter {
  _GraphPainter({
    required this.tokens,
    required this.pan,
    required this.scale,
    required this.arrows,
    required this.entry,
    required this.link,
    required this.drop,
  });

  final FynkelTokens tokens;
  final Offset pan;
  final double scale;
  final List<_Arrow> arrows;
  final _Arrow? entry;
  final (Offset, Offset)? link;
  final bool drop;

  @override
  void paint(Canvas canvas, Size size) {
    final t = tokens;
    canvas.drawRect(Offset.zero & size, Paint()..color = t.window);
    // Unity's grid: fine lines every 20, stronger every 100.
    final fine = Paint()..color = t.line.withValues(alpha: 0.35);
    final strong = Paint()..color = t.line.withValues(alpha: 0.8);
    for (final (step, paint) in [(20.0 * scale, fine), (100.0 * scale, strong)]) {
      for (var x = pan.dx % step; x < size.width; x += step) {
        canvas.drawLine(Offset(x, 0), Offset(x, size.height), paint);
      }
      for (var y = pan.dy % step; y < size.height; y += step) {
        canvas.drawLine(Offset(0, y), Offset(size.width, y), paint);
      }
    }
    if (entry case final e?) _arrow(canvas, e, defaultStateColor.withValues(alpha: 0.9));
    for (final a in arrows) {
      _arrow(canvas, a, a.selected ? t.accent : t.textSecondary);
      if (a.active case final p?) {
        // Unity's running transition: the arrow fills as the crossfade goes.
        canvas.drawLine(
          a.a,
          Offset.lerp(a.a, a.b, p.clamp(0.0, 1.0))!,
          Paint()
            ..color = t.ok
            ..strokeWidth = 3,
        );
      }
    }
    if (link case (final a, final b)) _arrow(canvas, _Arrow(a, b, 1, true), t.accent);
    if (drop) {
      canvas.drawRect(
        (Offset.zero & size).deflate(1),
        Paint()
          ..color = t.accent
          ..style = PaintingStyle.stroke
          ..strokeWidth = 2,
      );
    }
  }

  void _arrow(Canvas canvas, _Arrow a, Color color) {
    final paint = Paint()
      ..color = color
      ..strokeWidth = a.selected ? 2.5 : 1.5;
    canvas.drawLine(a.a, a.b, paint);
    final d = a.b - a.a;
    if (d.distance < 1) return;
    final u = d / d.distance;
    final n = Offset(-u.dy, u.dx);
    final mid = a.a + d / 2;
    final fill = Paint()..color = color;
    // One head at the middle; three when the pair has several transitions.
    for (final k in a.count > 1 ? const [-1, 0, 1] : const [0]) {
      final tip = mid + u * (6 + k * 9.0);
      final path = Path()
        ..moveTo(tip.dx, tip.dy)
        ..lineTo((tip - u * 9 + n * 5).dx, (tip - u * 9 + n * 5).dy)
        ..lineTo((tip - u * 9 - n * 5).dx, (tip - u * 9 - n * 5).dy)
        ..close();
      canvas.drawPath(path, fill);
    }
  }

  @override
  bool shouldRepaint(_GraphPainter old) => true;
}

/// The parameters (Unity's Parameters tab): + adds one, the name field
/// renames it everywhere, the value is its default, ⋯ deletes it.
class _ParameterList extends StatelessWidget {
  const _ParameterList({required this.model, required this.controller, this.live});

  /// During Play: the running values, shown and set in place of the
  /// defaults (Unity's way to try a transition).
  final AnimatorLive? live;

  final AnimatorModel model;
  final ControllerEdits controller;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final c = controller;
    final params = c.parameters;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: t.toolbar,
          padding: const EdgeInsets.only(left: 8),
          decoration: BoxDecoration(border: Border(bottom: t.hairline)),
          child: Row(
            children: [
              Expanded(
                child: Text.rich(
                  TextSpan(
                    children: [
                      const TextSpan(text: 'Parameters'),
                      if (live != null)
                        TextSpan(
                          text: '  live',
                          style: TextStyle(color: t.ok),
                        ),
                    ],
                  ),
                  key: const Key('animator.parameters.title'),
                  style: theme.textTheme.labelMedium,
                ),
              ),
              PopupMenuButton<String>(
                key: const Key('animator.addParameter'),
                tooltip: 'Add a parameter',
                enabled: c.canEdit,
                popUpAnimationStyle: AnimationStyle.noAnimation,
                icon: const Icon(FIcons.add, size: 14),
                onSelected: c.addParameter,
                itemBuilder: (_) => [
                  for (final type in parameterTypes)
                    PopupMenuItem(
                      key: Key('animator.addParameter.$type'),
                      value: type,
                      height: 30,
                      child: Text(_typeLabel(type), style: theme.textTheme.bodySmall),
                    ),
                ],
              ),
            ],
          ),
        ),
        Expanded(
          child: params.isEmpty
              ? Padding(
                  padding: const EdgeInsets.all(10),
                  child: Text(
                    'No parameters. Code sets them (setFloat, setBool, setTrigger) and transitions test them.',
                    style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                  ),
                )
              : ListView(
                  key: const Key('animator.parameters'),
                  padding: const EdgeInsets.symmetric(vertical: 4),
                  children: [for (final p in params) _row(context, t, p)],
                ),
        ),
        if (c.lastError != null)
          Padding(
            padding: const EdgeInsets.all(6),
            child: Text(
              c.lastError!,
              key: const Key('animator.error'),
              style: theme.textTheme.bodySmall?.copyWith(color: t.error),
            ),
          ),
      ],
    );
  }

  static String _typeLabel(String type) => switch (type) {
    'float' => 'Float',
    'int' => 'Int',
    'bool' => 'Bool',
    _ => 'Trigger',
  };

  Widget _row(BuildContext context, FynkelTokens t, Map<String, Object?> p) {
    final c = controller;
    final name = p['name'] as String? ?? '';
    final type = p['type'] as String? ?? 'float';
    final running = live;
    final value = running == null ? p['default'] : running.parameters.where((x) => x.name == name).firstOrNull?.value;
    // During Play a value goes to the running animator; otherwise it is the default.
    final bool enabled;
    final void Function(Object v) commit;
    if (running != null) {
      enabled = true;
      commit = (v) => running.setParameter(name, v);
    } else {
      enabled = c.canEdit;
      commit = (v) => c.setParameterDefault(name, v);
    }
    final Widget editor = switch (type) {
      'float' || 'int' => SizedBox(
        width: 58,
        child: NumberField(
          key: Key('animator.parameter.$name.value'),
          value: value is num ? value : 0,
          integer: type == 'int',
          step: type == 'int' ? 1 : 0.1,
          enabled: enabled,
          onCommit: commit,
        ),
      ),
      'bool' => BoolField(
        key: Key('animator.parameter.$name.value'),
        value: value == true,
        enabled: enabled,
        onCommit: commit,
      ),
      // A trigger: set during Play by a click (filled while set).
      _ => IconButton(
        key: Key('animator.parameter.$name.value'),
        tooltip: running == null ? 'A trigger starts unset' : 'Set the trigger',
        visualDensity: VisualDensity.compact,
        icon: Icon(
          value == true ? FIcons.circle : FIcons.circleOutlined,
          size: 13,
          color: value == true ? t.ok : t.textDisabled,
        ),
        onPressed: running == null ? null : () => running.setParameter(name, true),
      ),
    };
    return Padding(
      key: Key('animator.parameter.$name'),
      padding: const EdgeInsets.fromLTRB(8, 2, 0, 2),
      child: Row(
        children: [
          Expanded(
            child: StringField(
              key: Key('animator.parameter.$name.name'),
              value: name,
              enabled: enabled,
              onCommit: (to) => c.renameParameter(name, to.trim()),
            ),
          ),
          const SizedBox(width: 4),
          Tooltip(message: _typeLabel(type), child: editor),
          PopupMenuButton<String>(
            key: Key('animator.parameter.$name.menu'),
            tooltip: '',
            enabled: enabled,
            popUpAnimationStyle: AnimationStyle.noAnimation,
            icon: const Icon(FIcons.moreVert, size: 14),
            onSelected: (v) {
              if (v == 'delete') c.deleteParameter(name);
            },
            itemBuilder: (_) => [
              PopupMenuItem(
                key: Key('animator.parameter.$name.delete'),
                value: 'delete',
                height: 30,
                child: Text('Delete', style: Theme.of(context).textTheme.bodySmall),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// A layer after the base one: its weight, how it blends, the node it
/// poses from (Unity's layer settings).
/// The layer's slot (ADR-0033 § 2): its name — what
/// `animation.playOneShot(clip, slot: …)` names — and during Play the
/// one-shot covering the machine, with how much of it shows.
class _SlotRow extends StatelessWidget {
  const _SlotRow({required this.name, required this.empty, required this.info, required this.assets});

  final String name;

  /// The layer has no states: it shows only its slot.
  final bool empty;
  final fynkel.AnimatorStateInfo? info;
  final List<AssetEntry> Function()? assets;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall;
    final shot = info?.oneShot;
    String label(fynkel.AssetRef r) {
      final ref = {'asset': r.asset, if (r.sub != null) 'sub': r.sub};
      final named = describeAssetRef(assets?.call() ?? const [], ref);
      return named.startsWith(r.asset) ? 'a clip made in code' : named;
    }

    return Container(
      key: const Key('animator.slot'),
      height: 24,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      decoration: BoxDecoration(
        color: t.panel,
        border: Border(bottom: t.hairline),
      ),
      child: Row(
        children: [
          Tooltip(
            message: "A one-shot played into this slot covers the layer's states, which keep running under it",
            child: Text('Slot', style: small?.copyWith(color: t.textSecondary)),
          ),
          const SizedBox(width: 6),
          Text(name, key: const Key('animator.slot.name'), style: small?.copyWith(color: t.text)),
          const SizedBox(width: 12),
          if (shot != null) ...[
            Icon(FIcons.playArrow, size: 11, color: t.accent),
            const SizedBox(width: 4),
            Flexible(
              child: Text(
                label(shot),
                key: const Key('animator.slot.oneShot'),
                overflow: TextOverflow.ellipsis,
                style: small?.copyWith(color: t.text),
              ),
            ),
            const SizedBox(width: 8),
            SizedBox(
              width: 80,
              child: ClipRRect(
                borderRadius: BorderRadius.circular(2),
                child: LinearProgressIndicator(
                  key: const Key('animator.slot.weight'),
                  value: info!.oneShotWeight.clamp(0.0, 1.0),
                  minHeight: 4,
                  color: t.accent,
                  backgroundColor: t.line,
                ),
              ),
            ),
            const SizedBox(width: 6),
            Text('${(info!.oneShotTime.clamp(0.0, 1.0) * 100).round()}%', style: small?.copyWith(color: t.textSecondary)),
          ] else
            Flexible(
              child: Text(
                empty
                    ? "No states: this layer shows only one-shots — animation.playOneShot(clip, slot: '$name')"
                    : 'No one-shot playing',
                key: const Key('animator.slot.hint'),
                overflow: TextOverflow.ellipsis,
                style: small?.copyWith(color: t.textSecondary),
              ),
            ),
        ],
      ),
    );
  }
}

class _LayerSettings extends StatelessWidget {
  const _LayerSettings({required this.model, required this.controller, required this.layer});

  final AnimatorModel model;
  final ControllerEdits controller;
  final int layer;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final c = controller;
    final l = c.layer(layer) ?? const {};
    final enabled = c.canEdit;
    final weight = l['weight'];
    return Container(
      padding: const EdgeInsets.fromLTRB(8, 3, 8, 3),
      decoration: BoxDecoration(
        color: t.panel,
        border: Border(bottom: t.hairline),
      ),
      child: Row(
        children: [
          SizedBox(
            width: 220,
            child: KeyedSubtree(
              key: const Key('animator.layer.weight'),
              child: NumberField(
                label: 'Weight',
                value: weight is num ? weight : 1,
                min: 0,
                max: 1,
                step: 0.01,
                slider: true,
                enabled: enabled,
                onCommit: (v) => c.setLayerField(layer, 'weight', v),
              ),
            ),
          ),
          const SizedBox(width: 12),
          SizedBox(
            width: 120,
            child: EnumField(
              key: const Key('animator.layer.blending'),
              value: l['blending'] as String? ?? 'override',
              values: const ['override', 'additive'],
              enabled: enabled,
              onCommit: (v) => c.setLayerField(layer, 'blending', v),
            ),
          ),
          const SizedBox(width: 12),
          Text('Root', style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.textSecondary)),
          const SizedBox(width: 6),
          SizedBox(
            width: 160,
            child: StringField(
              key: const Key('animator.layer.root'),
              value: l['root'] as String? ?? '',
              hint: 'Whole character',
              enabled: enabled,
              onCommit: (v) {
                if (v.trim().isNotEmpty) {
                  c.setLayerField(layer, 'root', v.trim());
                } else if (l['root'] != null) {
                  c.setLayerField(layer, 'root', unset);
                }
              },
            ),
          ),
          const SizedBox(width: 12),
          // Unity's Avatar Mask, inside the layer (ADR-0033 § 5).
          Flexible(
            child: TextButton.icon(
              key: const Key('animator.layer.mask'),
              onPressed: () => showMaskEditor(context, model, c, layer),
              icon: const Icon(FIcons.person, size: 13),
              label: Text(
                'Mask: ${maskSummary(c.mask(layer))}',
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// A running state's progress: a bar along the bottom of its box.
class _ProgressDecoration extends Decoration {
  const _ProgressDecoration({required this.progress, required this.color, required this.track});

  final double progress;
  final Color color;
  final Color track;

  @override
  BoxPainter createBoxPainter([VoidCallback? onChanged]) => _ProgressPainter(this);
}

class _ProgressPainter extends BoxPainter {
  _ProgressPainter(this.d);

  final _ProgressDecoration d;

  @override
  void paint(Canvas canvas, Offset offset, ImageConfiguration configuration) {
    final size = configuration.size;
    if (size == null) return;
    final bar = Rect.fromLTWH(offset.dx + 4, offset.dy + size.height - 7, size.width - 8, 3);
    canvas.drawRRect(RRect.fromRectAndRadius(bar, const Radius.circular(1.5)), Paint()..color = d.track);
    canvas.drawRRect(
      RRect.fromRectAndRadius(
        Rect.fromLTWH(bar.left, bar.top, bar.width * d.progress.clamp(0.0, 1.0), bar.height),
        const Radius.circular(1.5),
      ),
      Paint()..color = d.color,
    );
  }
}
