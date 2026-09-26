// The Inspector for what the Animator window selected (ADR-0032 § 8), as
// Unity's: a **state** — its name, motion (a clip or a blend space),
// speed and speed parameter, loop, root motion, and the transitions out
// of it in the order they are tried — or a **transition** — Has Exit Time
// and the exit time, the duration, the offset, and its conditions with
// + and −. Every change is one undo step on the controller, saved at
// once; ⌘Z undoes it while the Inspector holds the keyboard.
import 'package:flutter/material.dart';

import '../inspector/asset_edits.dart';
import '../inspector/field_widgets.dart';
import '../inspector/inspector_chrome.dart';
import '../inspector/material_inspector.dart' show describeAssetRef;
import '../shell/editor_selection.dart';
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'animator_model.dart';
import 'controller_edits.dart';

class AnimatorItemPage extends StatefulWidget {
  const AnimatorItemPage({
    super.key,
    required this.edits,
    required this.target,
    this.animator,
    this.selection,
    this.onPing,
  });

  final AssetEdits edits;
  final InspectAnimator target;

  /// The Animator window's model: a rename or a pick here moves its
  /// selection too. Null: this page moves [selection] alone.
  final AnimatorModel? animator;
  final EditorSelection? selection;
  final void Function(String assetId)? onPing;

  @override
  State<AnimatorItemPage> createState() => _AnimatorItemPageState();
}

class _AnimatorItemPageState extends State<AnimatorItemPage> {
  AssetEdits get edits => widget.edits;
  InspectAnimator get target => widget.target;
  late ControllerEdits c = ControllerEdits(edits, target.controller);

  @override
  void initState() {
    super.initState();
    edits.addListener(_changed);
    edits.shown = target.controller;
  }

  @override
  void didUpdateWidget(covariant AnimatorItemPage old) {
    super.didUpdateWidget(old);
    if (old.edits != edits) {
      old.edits.removeListener(_changed);
      edits.addListener(_changed);
    }
    c = ControllerEdits(edits, target.controller);
    edits.shown = target.controller;
  }

  @override
  void dispose() {
    edits.removeListener(_changed);
    if (edits.shown == target.controller) edits.shown = null;
    super.dispose();
  }

  void _changed() {
    if (mounted) setState(() {});
  }

  /// Shows another state or transition of the same layer.
  void _go({String? state, int? transition}) {
    final a = widget.animator;
    if (a != null && a.id == target.controller && a.layer == target.layer) {
      state != null ? a.selectState(state) : a.selectTransition(transition);
    } else {
      widget.selection?.inspect(InspectAnimator(target.controller, target.layer, state: state, transition: transition));
    }
  }

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final root = c.root;
    final state = target.state == null ? null : c.state(target.layer, target.state!);
    final transitions = c.transitions(target.layer);
    final index = target.transition;
    final transition = index != null && index < transitions.length ? transitions[index] : null;
    if (root == null || (state == null && transition == null)) {
      return InspectorEmpty(
        key: const Key('animatorItem.gone'),
        icon: FIcons.animator,
        title: 'Nothing to show',
        message: root == null ? (edits.lastError ?? 'The Animator Controller could not be opened.') : 'It was deleted.',
      );
    }
    final layerName = c.layer(target.layer)?['name'] as String? ?? '';
    final String title;
    if (state != null) {
      title = state['name'] as String;
    } else {
      final from = transition!['from'] == 'any' ? 'Any State' : '${transition['from']}';
      title = '$from → ${transition['to']}';
    }
    return Material(
      type: MaterialType.transparency,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          InspectorPageHeader(
            leading: inspectorPictureBox(context, Icon(FIcons.animator, size: 20, color: t.textSecondary)),
            titleKey: const Key('animatorItem.title'),
            title: Text(title),
            subtitle: Text('${state != null ? 'State' : 'Transition'} · ${root['name'] ?? ''} › $layerName'),
            selection: widget.selection,
          ),
          Expanded(
            child: ListView(
              key: const Key('animatorItem.page'),
              padding: const EdgeInsets.only(bottom: 16),
              children: [
                if (state != null) ..._state(context, state) else ..._transition(context, transition!, index!),
                if (edits.lastError != null)
                  Padding(
                    padding: EdgeInsets.fromLTRB(t.pad + 2, 8, t.pad + 2, 0),
                    child: Text(
                      edits.lastError!,
                      key: const Key('animatorItem.error'),
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.error),
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  // ── A state ─────────────────────────────────────────────────────────────

  List<Widget> _state(BuildContext context, Map<String, Object?> s) {
    final t = context.tokens;
    final layer = target.layer;
    final name = s['name'] as String;
    final enabled = c.canEdit;
    final isDefault = c.defaultState(layer) == name;
    final floats = [
      for (final p in c.parameters)
        if (p['type'] == 'float') p['name'] as String,
    ];
    void set(String key, Object? value) => c.setStateField(layer, name, key, value);
    void clear(String key) {
      if (s.containsKey(key)) set(key, unset);
    }

    final motion = s['motion'];
    final speed = s['speed'];
    final rootMotion = s['rootMotion'];
    final out = [
      for (var i = 0; i < c.transitions(layer).length; i++)
        if (c.transitions(layer)[i]['from'] == name) i,
    ];
    return [
      InspectorFields(
        children: [
          FieldRow(
            label: 'Name',
            child: StringField(
              key: const Key('animatorItem.name'),
              value: name,
              enabled: enabled,
              onCommit: (to) {
                final next = to.trim();
                final a = widget.animator;
                if (a != null && a.id == target.controller && a.layer == layer) {
                  a.renameState(name, next);
                } else if (c.renameState(layer, name, next)) {
                  _go(state: next);
                }
              },
            ),
          ),
          FieldRow(
            label: 'Motion',
            tooltip: 'An animation clip or a blend space. None: the state holds the pose.',
            child: AssetField(
              key: const Key('animatorItem.motion'),
              value: motion is Map ? motion.cast<String, Object?>() : null,
              kinds: const {'clip', 'blend'},
              nullable: true,
              enabled: enabled,
              subject: 'Motion',
              describe: (r) => describeAssetRef(edits.project, r),
              candidates: () => [
                for (final a in edits.project.assets())
                  if (a.offeredKinds.contains('clip') || a.offeredKinds.contains('blend')) a,
              ],
              onPing: widget.onPing,
              onCommit: (r) => r == null ? clear('motion') : set('motion', r),
            ),
          ),
          KeyedSubtree(
            key: const Key('animatorItem.speed'),
            child: NumberField(
              label: 'Speed',
              value: speed is num ? speed : 1,
              step: 0.05,
              enabled: enabled,
              onCommit: (v) => set('speed', v),
            ),
          ),
          FieldRow(
            label: 'Multiplier',
            tooltip: 'A float parameter the speed is multiplied by.',
            child: EnumField(
              key: const Key('animatorItem.speedParameter'),
              // `#none` (no parameter name has a #): None.
              value: s['speedParameter'] as String? ?? '#none',
              values: ['#none', ...floats],
              display: (v) => v == '#none' ? 'None' : v,
              enabled: enabled,
              onCommit: (v) => v == '#none' ? clear('speedParameter') : set('speedParameter', v),
            ),
          ),
          FieldRow(
            label: 'Loop',
            child: BoolField(
              key: const Key('animatorItem.loop'),
              value: s['loop'] != false,
              enabled: enabled,
              onCommit: (v) => v ? clear('loop') : set('loop', false),
            ),
          ),
          FieldRow(
            label: 'Root Motion',
            tooltip: 'Whether its root motion moves the character. As the Animator: its Root motion field decides.',
            child: EnumField(
              key: const Key('animatorItem.rootMotion'),
              value: rootMotion == null ? 'animator' : (rootMotion == true ? 'on' : 'off'),
              values: const ['animator', 'on', 'off'],
              display: (v) => switch (v) {
                'on' => 'On',
                'off' => 'Off',
                _ => 'As the Animator',
              },
              enabled: enabled,
              onCommit: (v) => v == 'animator' ? clear('rootMotion') : set('rootMotion', v == 'on'),
            ),
          ),
        ],
      ),
      Padding(
        padding: EdgeInsets.fromLTRB(t.pad + 2, 2, t.pad + 2, 6),
        child: Align(
          alignment: Alignment.centerLeft,
          child: PageAction(
            key: const Key('animatorItem.setDefault'),
            icon: FIcons.flag,
            label: isDefault ? 'The layer\'s default state' : 'Set as Layer Default State',
            onPressed: enabled && !isDefault ? () => c.setDefault(layer, name) : null,
          ),
        ),
      ),
      InspectorSection(
        title: 'Transitions',
        children: [
          if (out.isEmpty)
            Padding(
              padding: EdgeInsets.fromLTRB(t.pad + 2, 6, t.pad + 2, 6),
              child: Text(
                'None. Right-click the state in the Animator window: Make Transition.',
                style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.textDisabled),
              ),
            )
          else
            for (final (n, i) in out.indexed) _transitionRow(context, i, n == 0, n == out.length - 1),
        ],
      ),
    ];
  }

  /// A transition out of the state: its target, ↑ ↓ to change the order
  /// it is tried in, − to delete it; a click shows it.
  Widget _transitionRow(BuildContext context, int i, bool first, bool last) {
    final t = context.tokens;
    final tr = c.transitions(target.layer)[i];
    final enabled = c.canEdit;
    final conditions = tr['conditions'] is List ? (tr['conditions'] as List).length : 0;
    final detail = [
      if (tr['exitTime'] is num) 'exit ${tr['exitTime']}',
      if (conditions > 0) '$conditions condition${conditions == 1 ? '' : 's'}',
    ].join(', ');
    return InkWell(
      key: Key('animatorItem.transition.$i'),
      onTap: () => _go(transition: i),
      child: Padding(
        padding: EdgeInsets.fromLTRB(t.pad + 2, 1, 4, 1),
        child: Row(
          children: [
            Expanded(
              child: Text.rich(
                TextSpan(
                  children: [
                    TextSpan(text: '→ ${tr['to']}'),
                    if (detail.isNotEmpty)
                      TextSpan(
                        text: '   $detail',
                        style: TextStyle(color: t.textDisabled),
                      ),
                  ],
                ),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ),
            IconButton(
              tooltip: 'Try it earlier',
              visualDensity: VisualDensity.compact,
              icon: const Icon(FIcons.arrowUpward, size: 13),
              onPressed: enabled && !first ? () => c.reorderTransition(target.layer, i, -1) : null,
            ),
            IconButton(
              tooltip: 'Try it later',
              visualDensity: VisualDensity.compact,
              icon: const Icon(FIcons.arrowDownward, size: 13),
              onPressed: enabled && !last ? () => c.reorderTransition(target.layer, i, 1) : null,
            ),
            IconButton(
              key: Key('animatorItem.transition.$i.delete'),
              tooltip: 'Delete',
              visualDensity: VisualDensity.compact,
              icon: const Icon(FIcons.remove, size: 13),
              onPressed: enabled ? () => c.deleteTransition(target.layer, i) : null,
            ),
          ],
        ),
      ),
    );
  }

  // ── A transition ────────────────────────────────────────────────────────

  List<Widget> _transition(BuildContext context, Map<String, Object?> tr, int index) {
    final t = context.tokens;
    final layer = target.layer;
    final enabled = c.canEdit;
    void set(String key, Object? value) => c.setTransitionField(layer, index, key, value);
    final exitTime = tr['exitTime'];
    final duration = tr['duration'];
    final offset = tr['offset'];
    final conditions = [
      if (tr['conditions'] is List)
        for (final x in tr['conditions'] as List)
          if (x is Map) x.cast<String, Object?>(),
    ];
    final params = c.parameters;
    return [
      InspectorFields(
        children: [
          FieldRow(
            label: 'Has Exit Time',
            tooltip: 'Wait until the state has played this far. Without conditions it fires then.',
            child: BoolField(
              key: const Key('animatorItem.hasExitTime'),
              value: exitTime is num,
              enabled: enabled,
              onCommit: (v) => v ? set('exitTime', 0.75) : set('exitTime', unset),
            ),
          ),
          if (exitTime is num)
            KeyedSubtree(
              key: const Key('animatorItem.exitTime'),
              child: NumberField(
                label: 'Exit Time',
                tooltip: 'The fraction of the motion: 0.9 = 90 %, 1.5 = halfway through the second loop.',
                value: exitTime,
                min: 0,
                step: 0.01,
                enabled: enabled,
                onCommit: (v) => set('exitTime', v),
              ),
            ),
          KeyedSubtree(
            key: const Key('animatorItem.duration'),
            child: NumberField(
              label: 'Duration',
              tooltip: 'The crossfade, in seconds.',
              unit: 's',
              value: duration is num ? duration : 0.25,
              min: 0,
              step: 0.01,
              enabled: enabled,
              onCommit: (v) => set('duration', v),
            ),
          ),
          KeyedSubtree(
            key: const Key('animatorItem.offset'),
            child: NumberField(
              label: 'Offset',
              tooltip: 'Where the next state starts, as a fraction of its motion.',
              value: offset is num ? offset : 0,
              min: 0,
              max: 1,
              step: 0.01,
              slider: true,
              enabled: enabled,
              onCommit: (v) => set('offset', v),
            ),
          ),
          if (tr['from'] == 'any')
            FieldRow(
              label: 'To Self',
              tooltip: 'Whether it may fire into the state already playing (restarting it).',
              child: BoolField(
                key: const Key('animatorItem.toSelf'),
                value: tr['canTransitionToSelf'] == true,
                enabled: enabled,
                onCommit: (v) => v ? set('canTransitionToSelf', true) : set('canTransitionToSelf', unset),
              ),
            ),
        ],
      ),
      InspectorSection(
        title: 'Conditions',
        trailing: IconButton(
          key: const Key('animatorItem.addCondition'),
          tooltip: params.isEmpty ? 'Add a parameter first' : 'Add a condition',
          visualDensity: VisualDensity.compact,
          icon: const Icon(FIcons.add, size: 14),
          onPressed: enabled && params.isNotEmpty
              ? () => c.setConditions(layer, index, [...conditions, c.conditionOn(params.first['name'] as String)])
              : null,
        ),
        children: [
          if (conditions.isEmpty)
            Padding(
              padding: EdgeInsets.fromLTRB(t.pad + 2, 6, t.pad + 2, 6),
              child: Text(
                exitTime is num ? 'None: it fires at its exit time.' : 'None: add one, or give it an exit time.',
                style: Theme.of(context).textTheme.bodySmall?.copyWith(color: t.textDisabled),
              ),
            ),
          for (var i = 0; i < conditions.length; i++) _condition(context, layer, index, conditions, i),
        ],
      ),
    ];
  }

  /// One condition: the parameter, the test its type allows, the value.
  Widget _condition(BuildContext context, int layer, int index, List<Map<String, Object?>> all, int i) {
    final t = context.tokens;
    final enabled = c.canEdit;
    final cond = all[i];
    final name = cond['parameter'] as String? ?? '';
    final type = c.parameter(name)?['type'] as String?;
    void replace(Map<String, Object?> next) => c.setConditions(layer, index, [...all]..[i] = next);
    final ops = switch (type) {
      'float' => const ['greater', 'less'],
      'int' => const ['greater', 'less', 'equals', 'notEquals'],
      'bool' => const ['true', 'false'],
      _ => const <String>[],
    };
    final op = switch (type) {
      'bool' => cond['is'] == false ? 'false' : 'true',
      _ => cond.keys.where((k) => k != 'parameter').firstOrNull,
    };
    final value = op == null ? null : cond[op];
    return Padding(
      key: Key('animatorItem.condition.$i'),
      padding: EdgeInsets.fromLTRB(t.pad + 2, 2, 4, 2),
      child: Row(
        children: [
          Expanded(
            flex: 3,
            child: EnumField(
              key: Key('animatorItem.condition.$i.parameter'),
              value: name,
              values: [for (final p in c.parameters) p['name'] as String],
              display: (v) => v,
              enabled: enabled,
              onCommit: (v) => replace(c.conditionOn(v)),
            ),
          ),
          if (ops.isNotEmpty) ...[
            const SizedBox(width: 4),
            Expanded(
              flex: 3,
              child: EnumField(
                key: Key('animatorItem.condition.$i.op'),
                value: op,
                values: ops,
                display: (v) => switch (v) {
                  'greater' => 'Greater',
                  'less' => 'Less',
                  'equals' => 'Equals',
                  'notEquals' => 'Not Equal',
                  'true' => 'true',
                  _ => 'false',
                },
                enabled: enabled,
                onCommit: (v) => replace(
                  type == 'bool'
                      ? {'parameter': name, 'is': v == 'true'}
                      : {
                          'parameter': name,
                          v: v == 'greater' || v == 'less' ? (value ?? 0) : ((value as num?)?.round() ?? 0),
                        },
                ),
              ),
            ),
          ],
          if (type == 'float' || type == 'int') ...[
            const SizedBox(width: 4),
            Expanded(
              flex: 2,
              child: NumberField(
                key: Key('animatorItem.condition.$i.value'),
                value: value is num ? value : 0,
                integer: type == 'int',
                enabled: enabled,
                onCommit: (v) => replace({'parameter': name, op!: v}),
              ),
            ),
          ],
          IconButton(
            key: Key('animatorItem.condition.$i.remove'),
            tooltip: 'Remove',
            visualDensity: VisualDensity.compact,
            icon: const Icon(FIcons.remove, size: 13),
            onPressed: enabled ? () => c.setConditions(layer, index, [...all]..removeAt(i)) : null,
          ),
        ],
      ),
    );
  }
}
