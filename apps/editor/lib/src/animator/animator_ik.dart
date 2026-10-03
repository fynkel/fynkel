// Masks and IK in the Animator window (ADR-0033 § 5).
//
// A layer's mask is Unity's Avatar Mask, inside the layer: the followed
// object's nodes as a tree with a tick each — a tick poses the node and
// what is under it with this layer, no tick leaves them to the layers
// below; right-click a node for "This node only". Without an object the
// entries show as a list.
//
// The IK tab (beside Parameters) lists the controller's IK goals: + adds a
// Two Bone (an arm or a leg reaches a target), a Look At (the head turns
// to one) or a Foot Placement (the feet land on the ground); each names
// the nodes it moves (picked from the object's), its weight (a number or
// a float parameter) and an object to aim at — code aims it too, with
// `animation.setIKTarget`.
import 'package:flutter/material.dart';

import '../inspector/asset_edits.dart' show unset;
import '../inspector/field_widgets.dart';
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'animator_model.dart';
import 'controller_edits.dart';

/// What the + menu and a goal's header call each type.
String ikTypeLabel(String type) => switch (type) {
  'twoBone' => 'Two Bone',
  'lookAt' => 'Look At',
  'footPlacement' => 'Foot Placement',
  _ => type,
};

/// A node the character has for [wanted] (`RightHand` →
/// `mixamorig:RightHand`), else [wanted] itself.
String nodeLike(List<AnimatorNode> nodes, String wanted) {
  final matches = nodes
      .where(
        (n) =>
            n.binding.isNotEmpty &&
            (n.name == wanted ||
                n.name.endsWith(':$wanted') ||
                n.name.endsWith('_$wanted') ||
                n.name.toLowerCase().endsWith(wanted.toLowerCase())),
      )
      .toList();
  // Only an unambiguous suggestion may select a binding automatically.
  return matches.length == 1 ? matches.single.binding : wanted;
}

/// The IK tab: the controller's goals.
class IkList extends StatelessWidget {
  const IkList({super.key, required this.model, required this.controller});

  final AnimatorModel model;
  final ControllerEdits controller;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final c = controller;
    final goals = c.ik;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: t.toolbar,
          padding: const EdgeInsets.only(left: 8),
          decoration: BoxDecoration(border: Border(bottom: t.hairline)),
          child: Row(
            children: [
              Expanded(child: Text('IK goals', style: theme.textTheme.labelMedium)),
              PopupMenuButton<String>(
                key: const Key('animator.addIk'),
                tooltip: 'Add an IK goal',
                enabled: c.canEdit,
                popUpAnimationStyle: AnimationStyle.noAnimation,
                icon: const Icon(FIcons.add, size: 14),
                onSelected: (type) {
                  final nodes = model.nodes;
                  c.addIk(type, node: (w) => nodeLike(nodes, w));
                },
                itemBuilder: (_) => [
                  for (final type in ikTypes)
                    PopupMenuItem(
                      key: Key('animator.addIk.$type'),
                      value: type,
                      height: 30,
                      child: Text(ikTypeLabel(type), style: theme.textTheme.bodySmall),
                    ),
                ],
              ),
            ],
          ),
        ),
        Expanded(
          child: goals.isEmpty
              ? Padding(
                  padding: const EdgeInsets.all(10),
                  child: Text(
                    'No IK goals. + adds an arm or a leg that reaches (Two Bone), a head that looks (Look At) '
                    'or feet that land on the ground (Foot Placement). Code aims them: '
                    "animation.setIKTarget('hand', position).",
                    key: const Key('animator.ik.empty'),
                    style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled),
                  ),
                )
              : ListenableBuilder(
                  listenable: model.nodesChanged ?? const _Never(),
                  builder: (context, _) {
                    final nodes = [
                      for (final n in model.nodes)
                        if (n.binding.isNotEmpty) n,
                    ];
                    return ListView(
                      key: const Key('animator.ik'),
                      padding: const EdgeInsets.symmetric(vertical: 4),
                      children: [
                        for (final (i, g) in goals.indexed) _Goal(controller: c, index: i, goal: g, nodes: nodes),
                      ],
                    );
                  },
                ),
        ),
      ],
    );
  }
}

class _Never implements Listenable {
  const _Never();

  @override
  void addListener(VoidCallback listener) {}

  @override
  void removeListener(VoidCallback listener) {}
}

/// One goal: its name, type, weight, target and nodes.
class _Goal extends StatelessWidget {
  const _Goal({required this.controller, required this.index, required this.goal, required this.nodes});

  final ControllerEdits controller;
  final int index;
  final Map<String, Object?> goal;
  final List<AnimatorNode> nodes;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final small = Theme.of(context).textTheme.bodySmall;
    final c = controller;
    final enabled = c.canEdit;
    final type = goal['type'] as String? ?? '';
    final name = goal['name'] as String? ?? '';
    final key = 'animator.ik.$index';

    Widget row(String label, Widget field, {String? tip}) => Padding(
      padding: const EdgeInsets.only(top: 3),
      child: Row(
        children: [
          SizedBox(
            width: 58,
            child: Tooltip(
              message: tip ?? '',
              child: Text(label, style: small?.copyWith(color: t.textSecondary)),
            ),
          ),
          Expanded(child: field),
        ],
      ),
    );

    // A node the goal names: typed, or picked from the object's.
    Widget node(String field, {String? hint, bool required = false}) => Row(
      children: [
        Expanded(
          child: StringField(
            key: Key('$key.$field'),
            value: goal[field] as String? ?? '',
            hint: hint,
            enabled: enabled,
            onCommit: (v) {
              final s = v.trim();
              if (s.isNotEmpty) {
                c.setIkField(index, field, s);
              } else if (!required && goal[field] != null) {
                c.setIkField(index, field, unset);
              }
            },
          ),
        ),
        if (nodes.isNotEmpty)
          PopupMenuButton<String>(
            key: Key('$key.$field.pick'),
            tooltip: 'Pick a node',
            enabled: enabled,
            popUpAnimationStyle: AnimationStyle.noAnimation,
            icon: const Icon(FIcons.expandMore, size: 14),
            onSelected: (v) => c.setIkField(index, field, v),
            itemBuilder: (_) => [
              for (final n in nodes)
                PopupMenuItem(
                  value: n.binding,
                  height: 26,
                  child: Text('${n.name} (${n.binding})', style: small),
                ),
            ],
          ),
      ],
    );

    final floats = c.floats;
    final weight = goal['weight'];
    final fields = <Widget>[
      row(
        'Weight',
        Row(
          children: [
            SizedBox(
              width: 92,
              child: EnumField(
                key: Key('$key.weightFrom'),
                value: weight is String ? weight : 'number',
                values: ['number', ...floats],
                display: (v) => v == 'number' ? 'Number' : v,
                enabled: enabled,
                onCommit: (v) => c.setIkField(index, 'weight', v == 'number' ? unset : v),
              ),
            ),
            if (weight is! String) ...[
              const SizedBox(width: 4),
              Expanded(
                child: NumberField(
                  key: Key('$key.weight'),
                  value: weight is num ? weight : 1,
                  min: 0,
                  max: 1,
                  step: 0.05,
                  enabled: enabled,
                  onCommit: (v) => c.setIkField(index, 'weight', v),
                ),
              ),
            ],
          ],
        ),
        tip: 'How much it bends the pose: a number, or a float parameter code sets',
      ),
      if (type != 'footPlacement')
        row(
          'Target',
          node('target', hint: 'Set from code'),
          tip: 'An object to reach for or look at; animation.setIKTarget wins over it',
        ),
      ...switch (type) {
        'twoBone' => [
          row('Tip', node('tip', required: true), tip: 'The hand or the foot'),
          row('Middle', node('middle', hint: "The tip's parent"), tip: 'The elbow or the knee'),
          row('Root', node('root', hint: "The middle's parent"), tip: 'The upper arm or the thigh'),
          row('Pole', node('pole', hint: 'Bends as it does'), tip: 'An object the elbow or the knee points to'),
        ],
        'lookAt' => [
          row('Head', node('head', required: true)),
          row(
            'Spine',
            StringField(
              key: Key('$key.spine'),
              value: [for (final s in (goal['spine'] as List?) ?? const []) '$s'].join(', '),
              hint: 'None',
              enabled: enabled,
              onCommit: (v) {
                final list = [
                  for (final s in v.split(','))
                    if (s.trim().isNotEmpty) s.trim(),
                ];
                c.setIkField(index, 'spine', list.isEmpty ? unset : list);
              },
            ),
            tip: 'Nodes that turn a little with the head, comma separated',
          ),
          row(
            'Limit',
            NumberField(
              key: Key('$key.limit'),
              value: goal['limit'] is num ? goal['limit'] as num : 70,
              min: 0,
              max: 180,
              step: 1,
              unit: '°',
              enabled: enabled,
              onCommit: (v) => c.setIkField(index, 'limit', v),
            ),
            tip: 'The most it turns',
          ),
          row(
            'Body',
            NumberField(
              key: Key('$key.body'),
              value: goal['body'] is num ? goal['body'] as num : 0.3,
              min: 0,
              max: 1,
              step: 0.05,
              enabled: enabled,
              onCommit: (v) => c.setIkField(index, 'body', v),
            ),
            tip: 'The share of the turn the spine takes',
          ),
        ],
        'footPlacement' => [
          row('Hips', node('hips', required: true), tip: 'Goes down so both feet reach the ground'),
          row('Left', node('left', required: true), tip: 'The left foot (its parent is the knee)'),
          row('Right', node('right', required: true), tip: 'The right foot'),
          row(
            'Reach',
            NumberField(
              key: Key('$key.reach'),
              value: goal['reach'] is num ? goal['reach'] as num : 0.5,
              min: 0.01,
              max: 10,
              step: 0.05,
              unit: 'm',
              enabled: enabled,
              onCommit: (v) => c.setIkField(index, 'reach', v),
            ),
            tip: 'How far above and below the feet it looks for ground (needs the physics module)',
          ),
        ],
        _ => const <Widget>[],
      },
    ];

    return Container(
      key: Key(key),
      margin: const EdgeInsets.fromLTRB(6, 2, 4, 6),
      padding: const EdgeInsets.fromLTRB(6, 2, 2, 6),
      decoration: BoxDecoration(
        color: t.panel,
        borderRadius: BorderRadius.circular(4),
        border: Border.all(color: t.line),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: StringField(
                  key: Key('$key.name'),
                  value: name,
                  enabled: enabled,
                  onCommit: (to) => c.renameIk(index, to.trim()),
                ),
              ),
              const SizedBox(width: 6),
              Text(
                ikTypeLabel(type),
                key: Key('$key.type'),
                style: small?.copyWith(color: t.textSecondary),
              ),
              PopupMenuButton<String>(
                key: Key('$key.menu'),
                tooltip: '',
                enabled: enabled,
                popUpAnimationStyle: AnimationStyle.noAnimation,
                icon: const Icon(FIcons.moreVert, size: 14),
                onSelected: (v) {
                  if (v == 'delete') c.deleteIk(index);
                },
                itemBuilder: (_) => [
                  PopupMenuItem(
                    key: Key('$key.delete'),
                    value: 'delete',
                    height: 30,
                    child: Text('Delete', style: small),
                  ),
                ],
              ),
            ],
          ),
          ...fields,
        ],
      ),
    );
  }
}

/// A layer's mask in words, for the layer bar.
String maskSummary(List<Map<String, Object?>> mask) {
  if (mask.isEmpty) return 'Whole character';
  final ins = mask.where((e) => e['include'] == true).map((e) => e['node']).toList();
  final outs = mask.where((e) => e['include'] != true).map((e) => e['node']).toList();
  return [if (ins.isNotEmpty) ins.join(', '), if (outs.isNotEmpty) 'not ${outs.join(', ')}'].join('; ');
}

/// Opens [layer]'s mask (Unity's Avatar Mask, inside the layer).
Future<void> showMaskEditor(BuildContext context, AnimatorModel model, ControllerEdits controller, int layer) {
  return showDialog<void>(
    context: context,
    animationStyle: AnimationStyle.noAnimation,
    builder: (context) => Dialog(
      key: const Key('animator.mask'),
      child: SizedBox(
        width: 420,
        height: 520,
        child: MaskEditor(model: model, controller: controller, layer: layer),
      ),
    ),
  );
}

/// The object's nodes with a tick each; without an object, the entries.
class MaskEditor extends StatelessWidget {
  const MaskEditor({super.key, required this.model, required this.controller, required this.layer});

  final AnimatorModel model;
  final ControllerEdits controller;
  final int layer;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final theme = Theme.of(context);
    final small = theme.textTheme.bodySmall;
    return ListenableBuilder(
      listenable: Listenable.merge([controller.edits, model, ?model.nodesChanged]),
      builder: (context, _) {
        final c = controller;
        final name = c.layer(layer)?['name'] as String? ?? '';
        final mask = c.mask(layer);
        final nodes = model.nodes;
        final enabled = c.canEdit;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 12, 8, 6),
              child: Row(
                children: [
                  Expanded(child: Text('Mask — $name', style: theme.textTheme.titleSmall)),
                  TextButton(
                    key: const Key('animator.mask.clear'),
                    onPressed: enabled && mask.isNotEmpty ? () => c.clearMask(layer) : null,
                    child: const Text('Whole character'),
                  ),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 0, 14, 6),
              child: Text(
                nodes.isEmpty
                    ? 'Select an object whose Animator uses this controller to tick its nodes.'
                    : 'A tick poses the node and what is under it with this layer; no tick leaves them to the '
                          'layers below. Right-click a node for this node only.',
                style: small?.copyWith(color: t.textSecondary),
              ),
            ),
            Divider(height: 1, color: t.line),
            Expanded(
              child: nodes.isEmpty
                  ? _entries(context, t, c, mask, enabled)
                  : ListView(
                      key: const Key('animator.mask.tree'),
                      padding: const EdgeInsets.symmetric(vertical: 4),
                      children: [for (final (i, n) in nodes.indexed) _node(context, t, c, mask, nodes, i, n, enabled)],
                    ),
            ),
            if (nodes.isNotEmpty && mask.isNotEmpty) ...[
              Divider(height: 1, color: t.line),
              SizedBox(height: 96, child: _entries(context, t, c, mask, enabled)),
            ],
            Padding(
              padding: const EdgeInsets.all(8),
              child: Align(
                alignment: Alignment.centerRight,
                child: FilledButton(
                  key: const Key('animator.mask.close'),
                  onPressed: () => Navigator.pop(context),
                  child: const Text('Done'),
                ),
              ),
            ),
          ],
        );
      },
    );
  }

  Widget _node(
    BuildContext context,
    FynkelTokens t,
    ControllerEdits c,
    List<Map<String, Object?>> mask,
    List<AnimatorNode> nodes,
    int i,
    AnimatorNode n,
    bool enabled,
  ) {
    enabled = enabled && n.binding.isNotEmpty;
    final on = ControllerEdits.maskedIn(mask, n.bindings);
    final small = Theme.of(context).textTheme.bodySmall;
    final named = mask.any((e) => e['node'] == n.binding);
    void toggle({bool alone = false}) =>
        c.setMasked(layer, n.binding, !on, alone: alone, under: alone ? const [] : bindingsUnder(nodes, i));
    return GestureDetector(
      onSecondaryTapUp: !enabled
          ? null
          : (d) async {
              final choice = await showMenu<String>(
                context: context,
                position: RelativeRect.fromLTRB(d.globalPosition.dx, d.globalPosition.dy, d.globalPosition.dx, 0),
                popUpAnimationStyle: AnimationStyle.noAnimation,
                items: [
                  PopupMenuItem(
                    key: const Key('animator.mask.alone'),
                    value: 'alone',
                    height: 30,
                    child: Text(on ? 'Leave out this node only' : 'Pose this node only', style: small),
                  ),
                ],
              );
              if (choice == 'alone') toggle(alone: true);
            },
      child: InkWell(
        key: Key('animator.mask.node.${n.binding}'),
        onTap: enabled ? toggle : null,
        child: Padding(
          padding: EdgeInsets.only(left: 8.0 + 14 * n.depth, right: 8),
          child: Row(
            children: [
              SizedBox(
                width: 22,
                height: 22,
                child: Checkbox(
                  key: Key('animator.mask.node.${n.binding}.check'),
                  value: on,
                  visualDensity: VisualDensity.compact,
                  onChanged: enabled ? (_) => toggle() : null,
                ),
              ),
              const SizedBox(width: 4),
              Expanded(
                child: Text(
                  n.name,
                  overflow: TextOverflow.ellipsis,
                  style: small?.copyWith(
                    color: on ? t.text : t.textDisabled,
                    fontWeight: named ? FontWeight.w600 : null,
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _entries(
    BuildContext context,
    FynkelTokens t,
    ControllerEdits c,
    List<Map<String, Object?>> mask,
    bool enabled,
  ) {
    final small = Theme.of(context).textTheme.bodySmall;
    if (mask.isEmpty) {
      return Padding(
        padding: const EdgeInsets.all(14),
        child: Text('No mask: the layer poses the whole character.', style: small?.copyWith(color: t.textDisabled)),
      );
    }
    return ListView(
      key: const Key('animator.mask.entries'),
      padding: const EdgeInsets.symmetric(vertical: 4),
      children: [
        for (final (i, e) in mask.indexed)
          Padding(
            key: Key('animator.mask.entry.$i'),
            padding: const EdgeInsets.only(left: 14, right: 4),
            child: Row(
              children: [
                Icon(
                  e['include'] == true ? FIcons.check : FIcons.close,
                  size: 13,
                  color: e['include'] == true ? t.ok : t.textSecondary,
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    '${e['node']}${e['subtree'] == false ? ' (this node only)' : ''}',
                    overflow: TextOverflow.ellipsis,
                    style: small,
                  ),
                ),
                IconButton(
                  key: Key('animator.mask.entry.$i.delete'),
                  tooltip: 'Remove',
                  visualDensity: VisualDensity.compact,
                  icon: const Icon(FIcons.close, size: 13),
                  onPressed: enabled ? () => c.removeMaskEntry(layer, i) : null,
                ),
              ],
            ),
          ),
      ],
    );
  }
}
