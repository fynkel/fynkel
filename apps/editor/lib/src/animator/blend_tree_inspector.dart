// A state's blend tree in the Inspector (ADR-0033 § 1), as Unity's Blend
// Tree editor but in the Inspector itself: the path down to a nested tree,
// the type and its parameters, the map (each child a diamond, the circle
// around it its weight, the red dot the parameters' values: drag it to
// try them, during Play on the running Animator; drag a diamond to move
// the child), the list of children with their places and speeds, Compute
// Positions from the clips' root travel, and the tree playing on the
// model below. Every change is one undo step on the controller.
import 'dart:math' as math;

import 'package:flutter/gestures.dart' show DragStartBehavior;
import 'package:flutter/material.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show StageTravel;
import 'package:flutter/scheduler.dart' show Ticker;

import '../inspector/asset_edits.dart';
import '../inspector/field_widgets.dart';
import '../inspector/inspector_chrome.dart';
import '../inspector/material_inspector.dart' show describeAssetRef;
import '../preview/stage_view.dart';
import '../project/editor_project.dart' show AssetEntry;
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'animator_model.dart';
import 'blend_tree_weights.dart';
import 'controller_edits.dart';

/// The first clip a motion plays (depth first): its model is where the
/// preview and Compute Positions play the tree.
Map<String, Object?>? firstClip(Object? motion) {
  if (motion is! Map) return null;
  final tree = motion['blendTree'];
  if (tree is Map) {
    for (final c in (tree['children'] as List?) ?? const []) {
      if (firstClip(c is Map ? c['motion'] : null) case final clip?) return clip;
    }
    return null;
  }
  final sub = motion['sub'];
  return sub is String && sub.startsWith('anim/') ? motion.cast<String, Object?>() : null;
}

/// What Compute Positions reads of a clip's root travel.
enum TravelMeasure {
  speed('Speed'),
  velocityX('Velocity X'),
  velocityY('Velocity Y'),
  velocityZ('Velocity Z'),
  angularSpeed('Angular Speed (Deg)'),
  velocityXZ('Velocity XZ'),
  speedAndAngular('Speed and Angular Speed');

  const TravelMeasure(this.label);
  final String label;

  static const oneD = [speed, velocityX, velocityY, velocityZ, angularSpeed];
  static const twoD = [velocityXZ, speedAndAngular];

  /// The place a clip that travels so takes. X is the character's right
  /// and forward is -Z, so Velocity XZ reads (x, -z): forward is up.
  (double, double) place(List<double> v, double angularSpeed) {
    final (x, y, z) = (v[0], v[1], v[2]);
    final deg = angularSpeed * 180 / math.pi;
    return switch (this) {
      TravelMeasure.speed => (math.sqrt(x * x + y * y + z * z), 0),
      TravelMeasure.velocityX => (x, 0),
      TravelMeasure.velocityY => (y, 0),
      TravelMeasure.velocityZ => (z, 0),
      TravelMeasure.angularSpeed => (deg, 0),
      TravelMeasure.velocityXZ => (x, -z),
      TravelMeasure.speedAndAngular => (math.sqrt(x * x + y * y + z * z), deg),
    };
  }
}

class BlendTreeSection extends StatefulWidget {
  const BlendTreeSection({
    super.key,
    required this.edits,
    required this.controller,
    required this.layer,
    this.machine = const [],
    required this.state,
    this.animator,
    this.onPing,
  });

  final AssetEdits edits;
  final String controller;
  final int layer;

  /// The sub-state machine the state is in (ADR-0033 § 4).
  final List<String> machine;
  final String state;

  /// During Play, its running Animator takes the red dot's values.
  final AnimatorModel? animator;
  final void Function(String assetId)? onPing;

  @override
  State<BlendTreeSection> createState() => BlendTreeSectionState();
}

class BlendTreeSectionState extends State<BlendTreeSection> with SingleTickerProviderStateMixin {
  ControllerEdits get c => ControllerEdits(widget.edits, widget.controller, machine: widget.machine);

  /// The child indices down to the tree shown ([] is the state's own).
  List<int> path = [];

  /// The parameters' values the map and the preview show outside Play.
  final Map<String, double> values = {};

  /// A diamond being dragged: its index and where it is.
  (int, (double, double))? _drag;

  /// The map's range, kept while dragging.
  _Range? _frozen;

  late final Ticker _ticker = createTicker((_) {
    if (_live == null) {
      _ticker.stop();
    } else {
      setState(() {});
    }
  });

  AnimatorLive? get _live {
    final a = widget.animator;
    if (a == null || a.id != widget.controller) return null;
    return a.live;
  }

  @override
  void didUpdateWidget(covariant BlendTreeSection oldWidget) {
    super.didUpdateWidget(oldWidget);
    final old = oldWidget;
    if (old.state != widget.state || old.layer != widget.layer || old.controller != widget.controller) path = [];
  }

  @override
  void dispose() {
    _ticker.dispose();
    super.dispose();
  }

  /// Each float parameter's value: the running Animator's during Play,
  /// else the one tried here, else its default.
  Map<String, double> get current {
    final out = <String, double>{};
    for (final p in c.parameters) {
      if (p['type'] != 'float') continue;
      final name = p['name'] as String;
      final d = p['default'];
      out[name] = values[name] ?? (d is num ? d.toDouble() : 0);
    }
    final live = _live;
    if (live != null) {
      for (final p in live.parameters) {
        if (p.value case final double v) out[p.name] = v;
      }
    }
    return out;
  }

  void _setValue(String name, double v) {
    final live = _live;
    if (live != null) {
      live.setParameter(name, v);
      setState(() {});
    } else {
      setState(() => values[name] = v);
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final layer = widget.layer, state = widget.state;
    final c = this.c;
    var tree = c.treeAt(layer, state, path);
    // A child removed under the path: back up to what is left.
    while (tree == null && path.isNotEmpty) {
      path = path.sublist(0, path.length - 1);
      tree = c.treeAt(layer, state, path);
    }
    if (tree == null) return const SizedBox.shrink();
    if (_live != null && !_ticker.isActive) _ticker.start();
    final enabled = c.canEdit;
    final type = tree['type'] as String? ?? '1d';
    final parameters = [for (final p in (tree['parameters'] as List?) ?? const []) '$p'];
    final children = c.treeChildren(layer, state, path);
    final now = current;
    final weights = _weights(type, parameters, children, now);
    final small = Theme.of(context).textTheme.bodySmall;
    final model = firstClip(c.motionAt(layer, state, const []))?['asset'] as String?;
    return InspectorSection(
      title: 'Blend Tree',
      trailing: _addMenu(context, enabled && children.length < 32),
      children: [
        if (path.isNotEmpty) _breadcrumb(context),
        InspectorFields(
          children: [
            FieldRow(
              label: 'Type',
              child: EnumField(
                key: const Key('blendTree.type'),
                value: type,
                values: treeTypes,
                display: treeTypeName,
                enabled: enabled,
                onCommit: (v) => c.setTreeType(layer, state, path, v),
              ),
            ),
            for (var i = 0; i < treeParameterCount(type); i++)
              FieldRow(
                label: treeParameterCount(type) == 1 ? 'Parameter' : (i == 0 ? 'Parameter X' : 'Parameter Y'),
                tooltip: 'A float parameter of the controller.',
                child: EnumField(
                  key: Key('blendTree.parameter.$i'),
                  value: i < parameters.length ? parameters[i] : '#none',
                  values: [if (i >= parameters.length) '#none', ...c.floats],
                  display: (v) => v == '#none' ? 'None (add a float parameter)' : v,
                  enabled: enabled,
                  onCommit: (v) => v == '#none' ? null : c.setTreeParameter(layer, state, path, i, v),
                ),
              ),
          ],
        ),
        if (type != 'direct' && children.isNotEmpty)
          Padding(
            padding: EdgeInsets.fromLTRB(t.pad + 2, 2, t.pad + 2, 6),
            child: _map(context, type, parameters, children, weights, now, enabled),
          ),
        if (children.isEmpty)
          Padding(
            padding: EdgeInsets.fromLTRB(t.pad + 2, 6, t.pad + 2, 6),
            child: Text('No motions: + adds a clip or a blend tree.', style: small?.copyWith(color: t.textDisabled)),
          ),
        for (var i = 0; i < children.length; i++)
          _child(context, type, children, i, weights.elementAtOrNull(i) ?? 0, enabled),
        if (children.isNotEmpty)
          Padding(
            padding: EdgeInsets.fromLTRB(t.pad + 2, 6, t.pad + 2, 6),
            child: Align(alignment: Alignment.centerLeft, child: _computeMenu(context, type, children, model, enabled)),
          ),
        if (model != null && children.isNotEmpty)
          Padding(
            padding: EdgeInsets.fromLTRB(t.pad + 2, 2, t.pad + 2, 6),
            child: SizedBox(
              height: 220,
              child: StageView(
                key: const Key('blendTree.preview'),
                asset: model,
                tree: c.motionAt(layer, state, path),
                treeValues: now,
              ),
            ),
          ),
      ],
    );
  }

  /// Each child's weight at [now], as the game gives it.
  List<double> _weights(
    String type,
    List<String> parameters,
    List<Map<String, Object?>> children,
    Map<String, double> now,
  ) {
    if (type == 'direct') {
      return directWeights([for (final ch in children) now[ch['parameter']] ?? 0]);
    }
    final places = [
      for (final (i, ch) in children.indexed)
        if (_drag case (final j, final at) when j == i) at else ControllerEdits.placeOf(ch),
    ];
    return treeWeights(type, places, _input(parameters, now));
  }

  (double, double) _input(List<String> parameters, Map<String, double> now) =>
      (parameters.isEmpty ? 0 : now[parameters[0]] ?? 0, parameters.length < 2 ? 0 : now[parameters[1]] ?? 0);

  Widget _breadcrumb(BuildContext context) {
    final t = context.tokens;
    final style = Theme.of(context).textTheme.bodySmall;
    final names = <String>[widget.state];
    for (var d = 0; d < path.length; d++) {
      final child = c.treeChildren(widget.layer, widget.state, path.sublist(0, d)).elementAtOrNull(path[d]);
      names.add(child == null ? '?' : _motionName(child['motion'], path[d]));
    }
    return Padding(
      key: const Key('blendTree.path'),
      padding: EdgeInsets.fromLTRB(t.pad, 4, t.pad, 0),
      child: Wrap(
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          for (var d = 0; d < names.length; d++) ...[
            if (d > 0) Icon(FIcons.chevronRight, size: 12, color: t.textDisabled),
            TextButton(
              key: Key('blendTree.path.$d'),
              style: TextButton.styleFrom(visualDensity: VisualDensity.compact, padding: const EdgeInsets.all(4)),
              onPressed: d == names.length - 1 ? null : () => setState(() => path = path.sublist(0, d)),
              child: Text(names[d], style: style),
            ),
          ],
        ],
      ),
    );
  }

  String _motionName(Object? motion, int i) {
    if (motion is Map && motion['blendTree'] != null) return 'Blend Tree ${i + 1}';
    if (motion is Map) return describeAssetRef(widget.edits.project, motion.cast<String, Object?>());
    return 'None';
  }

  Widget _addMenu(BuildContext context, bool enabled) => PopupMenuButton<String>(
    key: const Key('blendTree.add'),
    enabled: enabled,
    tooltip: 'Add a motion',
    popUpAnimationStyle: AnimationStyle.noAnimation,
    padding: EdgeInsets.zero,
    icon: const Icon(FIcons.add, size: 14),
    onSelected: (v) => add(tree: v == 'tree'),
    itemBuilder: (_) => const [
      PopupMenuItem(key: Key('blendTree.add.motion'), value: 'motion', height: 26, child: Text('Add Motion…')),
      PopupMenuItem(key: Key('blendTree.add.tree'), value: 'tree', height: 26, child: Text('New Blend Tree…')),
    ],
  );

  /// + ▸ Add Motion: a clip or blend space picked, a child at a free place;
  /// + ▸ New Blend Tree: a tree holding the one picked (a tree is never
  /// empty).
  Future<void> add({bool tree = false}) async {
    final picked = await pickAssetRef(
      context,
      kind: null,
      kinds: _motionKinds,
      subject: tree ? 'New Blend Tree' : 'Motion',
      candidates: _motionCandidates,
    );
    final motion = picked?.ref;
    if (motion == null || !mounted) return;
    c.addChild(widget.layer, widget.state, path, motion: motion, tree: tree);
  }

  static const _motionKinds = {'clip', 'blend'};

  List<AssetEntry> _motionCandidates() => [
    for (final a in widget.edits.project.assets())
      if (a.offeredKinds.contains('clip') || a.offeredKinds.contains('blend')) a,
  ];

  // ── The map ─────────────────────────────────────────────────────────────

  Widget _map(
    BuildContext context,
    String type,
    List<String> parameters,
    List<Map<String, Object?>> children,
    List<double> weights,
    Map<String, double> now,
    bool enabled,
  ) {
    final t = context.tokens;
    final oneD = type == '1d';
    final places = [
      for (final (i, ch) in children.indexed)
        if (_drag case (final j, final at) when j == i) at else ControllerEdits.placeOf(ch),
    ];
    final input = _input(parameters, now);
    final range = _frozen ?? _Range.around([...places, input], oneD);
    final height = oneD ? 56.0 : 180.0;
    return LayoutBuilder(
      builder: (context, box) {
        final size = Size(box.maxWidth, height);
        Offset toScreen((double, double) p) => range.toScreen(p, size);
        int? hit(Offset at) {
          for (var i = places.length - 1; i >= 0; i--) {
            if ((toScreen(places[i]) - at).distance <= 8) return i;
          }
          return null;
        }

        void moveDot(Offset at) {
          final (x, y) = range.fromScreen(at, size);
          if (parameters.isNotEmpty) _setValue(parameters[0], _round(x));
          if (!oneD && parameters.length > 1) _setValue(parameters[1], _round(y));
        }

        return GestureDetector(
          key: const Key('blendTree.map'),
          behavior: HitTestBehavior.opaque,
          // A drag starts where the button went down: on a diamond or not.
          dragStartBehavior: DragStartBehavior.down,
          onTapDown: (d) => hit(d.localPosition) == null ? moveDot(d.localPosition) : null,
          onPanStart: (d) {
            final i = enabled ? hit(d.localPosition) : null;
            setState(() {
              _frozen = range;
              if (i != null) _drag = (i, places[i]);
            });
            if (i == null) moveDot(d.localPosition);
          },
          onPanUpdate: (d) {
            if (_drag case (final i, _)) {
              final (x, y) = range.fromScreen(d.localPosition, size);
              setState(() => _drag = (i, (x, oneD ? 0 : y)));
            } else {
              moveDot(d.localPosition);
            }
          },
          onPanEnd: (_) => _endDrag(),
          onPanCancel: _endDrag,
          child: CustomPaint(
            size: size,
            painter: _MapPainter(
              places: places,
              weights: weights,
              input: input,
              range: range,
              oneD: oneD,
              dragging: _drag?.$1,
              tokens: t,
            ),
          ),
        );
      },
    );
  }

  void _endDrag() {
    final drag = _drag;
    setState(() {
      _drag = null;
      _frozen = null;
    });
    if (drag case (final i, final at)) c.placeChildren(widget.layer, widget.state, path, {i: at});
  }

  static double _round(double v) => (v * 100).roundToDouble() / 100;

  // ── A child ─────────────────────────────────────────────────────────────

  Widget _child(
    BuildContext context,
    String type,
    List<Map<String, Object?>> children,
    int i,
    double weight,
    bool enabled,
  ) {
    final t = context.tokens;
    final layer = widget.layer, state = widget.state;
    final ch = children[i];
    final motion = ch['motion'];
    final isTree = motion is Map && motion['blendTree'] != null;
    final (x, y) = ControllerEdits.placeOf(ch);
    final scale = ch['timeScale'];
    void set(String key, Object? value) => c.setChildField(layer, state, path, i, key, value);
    final small = Theme.of(context).textTheme.bodySmall;
    return Padding(
      key: Key('blendTree.child.$i'),
      padding: EdgeInsets.fromLTRB(t.pad + 2, 3, 4, 3),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: isTree
                    ? OutlinedButton.icon(
                        key: Key('blendTree.child.$i.open'),
                        style: OutlinedButton.styleFrom(
                          visualDensity: VisualDensity.compact,
                          alignment: Alignment.centerLeft,
                        ),
                        icon: const Icon(FIcons.accountTreeOutlined, size: 13),
                        label: Text('Blend Tree ${i + 1}', style: small),
                        onPressed: () => setState(() => path = [...path, i]),
                      )
                    : AssetField(
                        key: Key('blendTree.child.$i.motion'),
                        value: motion is Map ? motion.cast<String, Object?>() : null,
                        kinds: _motionKinds,
                        enabled: enabled,
                        subject: 'Motion',
                        describe: (r) => describeAssetRef(widget.edits.project, r),
                        candidates: _motionCandidates,
                        onPing: widget.onPing,
                        onCommit: (r) => r == null ? null : set('motion', r),
                      ),
              ),
              SizedBox(
                width: 40,
                child: Text(
                  '${(weight * 100).round()} %',
                  key: Key('blendTree.child.$i.weight'),
                  textAlign: TextAlign.right,
                  style: small?.copyWith(color: weight > 0 ? t.text : t.textDisabled),
                ),
              ),
              IconButton(
                tooltip: 'Move up',
                visualDensity: VisualDensity.compact,
                icon: const Icon(FIcons.arrowUpward, size: 13),
                onPressed: enabled && i > 0 ? () => c.moveChild(layer, state, path, i, -1) : null,
              ),
              IconButton(
                tooltip: 'Move down',
                visualDensity: VisualDensity.compact,
                icon: const Icon(FIcons.arrowDownward, size: 13),
                onPressed: enabled && i < children.length - 1 ? () => c.moveChild(layer, state, path, i, 1) : null,
              ),
              IconButton(
                key: Key('blendTree.child.$i.remove'),
                tooltip: 'Remove',
                visualDensity: VisualDensity.compact,
                icon: const Icon(FIcons.remove, size: 13),
                onPressed: enabled ? () => c.removeChild(layer, state, path, i) : null,
              ),
            ],
          ),
          Padding(
            padding: const EdgeInsets.only(top: 2, right: 8),
            child: Row(
              children: [
                if (type == '1d')
                  Expanded(
                    child: NumberField(
                      key: Key('blendTree.child.$i.threshold'),
                      label: 'Threshold',
                      compact: true,
                      value: x,
                      step: 0.05,
                      enabled: enabled,
                      onCommit: (v) => set('threshold', v),
                    ),
                  ),
                if (treeIs2d(type)) ...[
                  Expanded(
                    child: NumberField(
                      key: Key('blendTree.child.$i.x'),
                      label: 'X',
                      compact: true,
                      value: x,
                      step: 0.05,
                      enabled: enabled,
                      onCommit: (v) => set('position', [v, y]),
                    ),
                  ),
                  const SizedBox(width: 4),
                  Expanded(
                    child: NumberField(
                      key: Key('blendTree.child.$i.y'),
                      label: 'Y',
                      compact: true,
                      value: y,
                      step: 0.05,
                      enabled: enabled,
                      onCommit: (v) => set('position', [x, v]),
                    ),
                  ),
                ],
                if (type == 'direct')
                  Expanded(
                    child: EnumField(
                      key: Key('blendTree.child.$i.parameter'),
                      value: ch['parameter'] as String? ?? '#none',
                      values: [if (ch['parameter'] == null) '#none', ...c.floats],
                      display: (v) => v == '#none' ? 'None' : v,
                      enabled: enabled,
                      onCommit: (v) => v == '#none' ? null : set('parameter', v),
                    ),
                  ),
                const SizedBox(width: 4),
                SizedBox(
                  width: 96,
                  child: NumberField(
                    key: Key('blendTree.child.$i.timeScale'),
                    label: 'Speed',
                    tooltip: 'How fast this child plays against the others.',
                    compact: true,
                    value: scale is num ? scale : 1,
                    min: 0.01,
                    step: 0.05,
                    enabled: enabled,
                    onCommit: (v) => (v as num) == 1 ? set('timeScale', unset) : set('timeScale', v),
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  // ── Compute Positions ───────────────────────────────────────────────────

  Widget _computeMenu(
    BuildContext context,
    String type,
    List<Map<String, Object?>> children,
    String? model,
    bool enabled,
  ) {
    final measures = switch (type) {
      '1d' => TravelMeasure.oneD,
      'direct' => const <TravelMeasure>[],
      _ => TravelMeasure.twoD,
    };
    final can = enabled && model != null && measures.isNotEmpty;
    return PopupMenuButton<TravelMeasure>(
      key: const Key('blendTree.compute'),
      enabled: can,
      tooltip: model == null ? 'Give a child a clip first' : 'Place each child by how its clip travels',
      popUpAnimationStyle: AnimationStyle.noAnimation,
      padding: EdgeInsets.zero,
      onSelected: (m) => compute(m),
      itemBuilder: (_) => [
        for (final m in measures)
          PopupMenuItem(key: Key('blendTree.compute.${m.name}'), value: m, height: 26, child: Text(m.label)),
      ],
      child: IgnorePointer(
        child: PageAction(icon: FIcons.autoAwesomeOutlined, label: 'Compute Positions', onPressed: can ? () {} : null),
      ),
    );
  }

  /// Unity's Compute Positions: each child playing a clip placed by the
  /// clip's root travel on the model, measured as [m].
  bool compute(TravelMeasure m) {
    final layer = widget.layer, state = widget.state;
    final model = firstClip(c.motionAt(layer, state, const []))?['asset'] as String?;
    final project = PreviewStageScope.maybeOf(context)?.project;
    if (model == null || project == null) return false;
    final children = c.treeChildren(layer, state, path);
    final clips = <int, Map<String, Object?>>{
      for (final (i, ch) in children.indexed)
        if (ch['motion'] case final Map<String, Object?> m when firstClip(m) != null && m['blendTree'] == null) i: m,
    };
    final List<StageTravel?> travel;
    try {
      final stage = project.openStage(model);
      try {
        travel = stage.travel([for (final m in clips.values) m]);
      } finally {
        stage.close();
      }
    } on Exception catch (e) {
      widget.edits.lastError = 'Compute Positions: $e';
      setState(() {});
      return false;
    }
    final byChild = <int, StageTravel?>{for (final (k, i) in clips.keys.indexed) i: travel.elementAtOrNull(k)};
    return c.computePlaces(layer, state, path, (i) {
      final tr = byChild[i];
      return tr == null || tr.velocity.length < 3 ? null : m.place(tr.velocity, tr.angularSpeed);
    });
  }
}

/// The map's range in parameter space.
final class _Range {
  const _Range(this.minX, this.maxX, this.minY, this.maxY);

  final double minX, maxX, minY, maxY;

  /// Around [points] with a margin; at least 1 wide, the origin in view.
  factory _Range.around(List<(double, double)> points, bool oneD) {
    var (x0, x1, y0, y1) = (0.0, 0.0, 0.0, 0.0);
    for (final (x, y) in points) {
      x0 = math.min(x0, x);
      x1 = math.max(x1, x);
      y0 = math.min(y0, y);
      y1 = math.max(y1, y);
    }
    if (x1 - x0 < 1) x1 = x0 + 1;
    if (y1 - y0 < 1) y1 = y0 + 1;
    final (mx, my) = ((x1 - x0) * 0.12, (y1 - y0) * 0.12);
    return oneD ? _Range(x0 - mx, x1 + mx, -1, 1) : _Range(x0 - mx, x1 + mx, y0 - my, y1 + my);
  }

  Offset toScreen((double, double) p, Size size) =>
      Offset((p.$1 - minX) / (maxX - minX) * size.width, size.height - (p.$2 - minY) / (maxY - minY) * size.height);

  (double, double) fromScreen(Offset o, Size size) =>
      (minX + o.dx / size.width * (maxX - minX), minY + (size.height - o.dy) / size.height * (maxY - minY));

  @override
  bool operator ==(Object other) =>
      other is _Range && other.minX == minX && other.maxX == maxX && other.minY == minY && other.maxY == maxY;

  @override
  int get hashCode => Object.hash(minX, maxX, minY, maxY);
}

class _MapPainter extends CustomPainter {
  _MapPainter({
    required this.places,
    required this.weights,
    required this.input,
    required this.range,
    required this.oneD,
    required this.dragging,
    required this.tokens,
  });

  final List<(double, double)> places;
  final List<double> weights;
  final (double, double) input;
  final _Range range;
  final bool oneD;
  final int? dragging;
  final FynkelTokens tokens;

  @override
  void paint(Canvas canvas, Size size) {
    final t = tokens;
    final rect = Offset.zero & size;
    canvas.drawRRect(RRect.fromRectAndRadius(rect, const Radius.circular(3)), Paint()..color = t.field);
    final line = Paint()
      ..color = t.line
      ..strokeWidth = 1;
    final origin = range.toScreen((0, 0), size);
    if (oneD) {
      canvas.drawLine(Offset(0, size.height / 2), Offset(size.width, size.height / 2), line);
    } else {
      canvas.drawLine(Offset(origin.dx, 0), Offset(origin.dx, size.height), line);
      canvas.drawLine(Offset(0, origin.dy), Offset(size.width, origin.dy), line);
    }
    Offset at((double, double) p) {
      final o = range.toScreen(p, size);
      return oneD ? Offset(o.dx, size.height / 2) : o;
    }

    final maxR = oneD ? size.height / 2 - 4 : math.min(size.width, size.height) / 5;
    for (var i = 0; i < places.length; i++) {
      final w = i < weights.length ? weights[i] : 0.0;
      if (w <= 0) continue;
      canvas.drawCircle(at(places[i]), 5 + w * maxR, Paint()..color = t.accent.withValues(alpha: 0.25));
    }
    for (var i = 0; i < places.length; i++) {
      final o = at(places[i]);
      final d = Path()
        ..moveTo(o.dx, o.dy - 5)
        ..lineTo(o.dx + 5, o.dy)
        ..lineTo(o.dx, o.dy + 5)
        ..lineTo(o.dx - 5, o.dy)
        ..close();
      canvas.drawPath(d, Paint()..color = i == dragging ? t.accent : t.textSecondary);
    }
    canvas.drawCircle(at(input), 4, Paint()..color = t.error);
  }

  @override
  bool shouldRepaint(_MapPainter old) =>
      old.places.toString() != places.toString() ||
      old.weights.toString() != weights.toString() ||
      old.input != input ||
      old.range != range ||
      old.dragging != dragging ||
      old.tokens != tokens;
}
