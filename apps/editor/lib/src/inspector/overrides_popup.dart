// Unity's Prefab bar and Overrides dropdown (docs/unity-parity.md § Tier 3):
// a prefab instance's Inspector opens with "Prefab  Open  Select
// Overrides ▾". The dropdown lists the instance's overrides by object;
// a row shows the prefab's value beside the instance's with Revert and
// Apply, and Revert All / Apply All close it.
import 'package:flutter/material.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../picker/picker.dart' show globalRectOf;
import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'field_widgets.dart' show prettyJson;
import 'inspector_model.dart';
import 'prefab_actions.dart' show ApplyTarget, applyTargetLabel;

/// The bar under a prefab instance's header: what it is an instance of,
/// Open, Select and the Overrides dropdown; a broken or missing prefab
/// says so under it.
class PrefabBar extends StatelessWidget {
  const PrefabBar({super.key, required this.model, required this.entity});

  final InspectorModel model;
  final EntityInfo entity;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final p = entity.prefab!;
    final x = entity.expansion;
    final info = x?.prefab;
    final assetText = model.describeAsset({'asset': p.assetId, if (p.sub != null) 'sub': p.sub});
    final name = info == null || info.name.isEmpty ? assetText : info.name;
    final broken = x != null && !x.expanded;
    final missing = x == null || x.prefab == null;
    final count = x?.overrides.length ?? p.overrides.length;
    final kind = info?.isVariant ?? false ? 'Prefab Variant' : 'Prefab';
    ButtonStyle compact() => TextButton.styleFrom(
      visualDensity: VisualDensity.compact,
      padding: const EdgeInsets.symmetric(horizontal: 6),
      minimumSize: const Size(0, 22),
      textStyle: const TextStyle(fontSize: 11),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          constraints: BoxConstraints(minHeight: t.row + 4),
          decoration: BoxDecoration(
            color: t.header,
            border: Border(bottom: t.hairline),
          ),
          padding: const EdgeInsets.only(left: 8, right: 4),
          child: LayoutBuilder(
            builder: (context, c) {
              final title = Row(
                children: [
                  Icon(
                    missing || broken ? FIcons.brokenImageOutlined : FIcons.widgetsOutlined,
                    size: 13,
                    color: missing || broken ? t.error : t.accent,
                  ),
                  const SizedBox(width: 6),
                  Expanded(
                    child: Tooltip(
                      message:
                          '$kind instance of $name${info?.isImported ?? false ? ' (imported hierarchy, read-only)' : ''}',
                      waitDuration: const Duration(milliseconds: 500),
                      child: Text(
                        '$kind  $name',
                        key: const ValueKey('prefab.title'),
                        style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ),
                ],
              );
              final buttons = [
                TextButton(
                  key: const ValueKey('prefab.open'),
                  style: compact(),
                  onPressed: x == null || missing ? null : () => model.openPrefab(entity.id),
                  child: const Text('Open'),
                ),
                TextButton(
                  key: const ValueKey('prefab.selectAsset'),
                  style: compact(),
                  onPressed: model.revealAsset == null ? null : () => model.selectPrefabAsset(entity.id),
                  child: const Text('Select'),
                ),
                Builder(
                  builder: (button) => TextButton(
                    key: const ValueKey('prefab.overrides'),
                    style: compact(),
                    onPressed: x == null
                        ? null
                        : () => showOverrides(button, model, entity.id, anchor: globalRectOf(button)),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(count == 0 ? 'Overrides' : 'Overrides ($count)'),
                        const Icon(FIcons.expandMore, size: 13),
                      ],
                    ),
                  ),
                ),
              ];
              // One line when there is room, as in Unity; the buttons wrap
              // under the name in a narrow Inspector.
              if (c.maxWidth >= 320) {
                return Row(
                  children: [
                    Expanded(child: title),
                    ...buttons,
                  ],
                );
              }
              return Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  SizedBox(height: t.row, child: title),
                  Wrap(alignment: WrapAlignment.end, children: buttons),
                ],
              );
            },
          ),
        ),
        if (x != null && x.diagnostics.isNotEmpty)
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 6, 8, 4),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  missing
                      ? 'The prefab is missing — the instance keeps its data and is not rewritten.'
                      : 'The instance does not expand — the instance keeps its data and is not rewritten.',
                  key: const ValueKey('prefab.broken'),
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.error),
                ),
                for (var i = 0; i < x.diagnostics.length; i++)
                  SelectableText(
                    '${x.diagnostics[i].code}: ${x.diagnostics[i].message}',
                    key: ValueKey('prefab.diagnostic.$i'),
                    style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.error),
                  ),
              ],
            ),
          ),
      ],
    );
  }
}

/// Opens the Overrides dropdown of the instance [entityId] under [anchor].
Future<void> showOverrides(BuildContext context, InspectorModel model, String entityId, {Rect? anchor}) {
  return showDialog<void>(
    context: context,
    barrierColor: Colors.transparent,
    builder: (dialog) {
      final screen = MediaQuery.sizeOf(dialog);
      const width = 380.0;
      final a = anchor ?? Rect.fromCenter(center: screen.center(Offset.zero), width: 0, height: 0);
      final left = (a.right - width).clamp(8.0, screen.width - width - 8);
      final top = (a.bottom + 4).clamp(8.0, screen.height - 120);
      return Stack(
        children: [
          Positioned(
            left: left,
            top: top,
            width: width,
            child: Material(
              elevation: 8,
              borderRadius: BorderRadius.circular(6),
              clipBehavior: Clip.antiAlias,
              child: ConstrainedBox(
                constraints: BoxConstraints(maxHeight: screen.height - top - 16),
                child: OverridesPanel(model: model, entityId: entityId),
              ),
            ),
          ),
        ],
      );
    },
  );
}

/// The dropdown's content, live on [model].
class OverridesPanel extends StatefulWidget {
  const OverridesPanel({super.key, required this.model, required this.entityId});

  final InspectorModel model;
  final String entityId;

  @override
  State<OverridesPanel> createState() => _OverridesPanelState();
}

class _OverridesPanelState extends State<OverridesPanel> {
  /// The override whose comparison is open (its index in the instance).
  int? _open;

  @override
  Widget build(BuildContext context) =>
      ListenableBuilder(listenable: widget.model, builder: (context, _) => _build(context));

  Widget _build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final model = widget.model;
    final id = widget.entityId;
    final overrides = model.expansionOf(id)?.overrides ?? const <ExpansionOverride>[];
    final name = model.tab?.tree[id]?.name ?? '';
    final applyWhy = model.cannotApply(id);
    if (_open != null && !overrides.any((o) => o.index == _open)) _open = null;
    // By object, in the order the instance lists them.
    final byObject = <String, List<ExpansionOverride>>{};
    for (final o in overrides) {
      byObject.putIfAbsent(o.entityName?.isNotEmpty ?? false ? o.entityName! : o.entity, () => []).add(o);
    }

    void close() => Navigator.of(context).pop();

    return Column(
      key: const ValueKey('prefab.overridesPanel'),
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 10, 12, 6),
          child: Text(
            "Review, Revert or Apply Overrides on '$name'",
            style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600),
          ),
        ),
        Divider(height: 1, color: t.line),
        if (overrides.isEmpty)
          Padding(
            padding: const EdgeInsets.all(12),
            child: Text(
              'No Overrides',
              key: const ValueKey('prefab.noOverrides'),
              style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary),
            ),
          )
        else
          Flexible(
            child: ListView(
              shrinkWrap: true,
              padding: const EdgeInsets.symmetric(vertical: 4),
              children: [
                for (final e in byObject.entries) ...[
                  Padding(
                    padding: const EdgeInsets.fromLTRB(12, 4, 12, 2),
                    child: Row(
                      children: [
                        Icon(FIcons.categoryOutlined, size: 12, color: t.textSecondary),
                        const SizedBox(width: 4),
                        Text(e.key, style: theme.textTheme.labelSmall?.copyWith(color: t.textSecondary)),
                      ],
                    ),
                  ),
                  for (final o in e.value) ..._row(context, o, applyWhy),
                ],
              ],
            ),
          ),
        Divider(height: 1, color: t.line),
        Padding(
          padding: const EdgeInsets.fromLTRB(8, 6, 8, 8),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              TextButton(
                key: const ValueKey('prefab.revertAll'),
                onPressed: overrides.isEmpty || !model.canEdit
                    ? null
                    : () {
                        model.revertOverride(id);
                        close();
                      },
                child: const Text('Revert All'),
              ),
              const SizedBox(width: 6),
              Tooltip(
                message: applyWhy ?? 'Write every override into the prefab, then revert them here',
                child: _ApplyButton(
                  keyName: 'prefab.applyAll',
                  label: 'Apply All',
                  filled: true,
                  targets: model.applyTargets(id),
                  onApply: overrides.isEmpty || applyWhy != null
                      ? null
                      : (depth) {
                          model.applyOverrides(id, depth: depth);
                          close();
                        },
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }

  List<Widget> _row(BuildContext context, ExpansionOverride o, String? applyWhy) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final model = widget.model;
    final open = _open == o.index;
    final component = _short(o.componentType ?? o.component ?? '');
    final (IconData icon, String text) = switch (o.op) {
      'setField' || 'unsetField' => (FIcons.tune, '$component › ${o.field}'),
      'addComponent' => (FIcons.add, '${_short('${o.rawOp['type'] ?? component}')} (Added)'),
      'removeComponent' => (FIcons.remove, '$component (Removed)'),
      'addObject' => (FIcons.add, '${o.rawOp['name'] ?? o.entity} (Added object)'),
      'removeObject' => (FIcons.remove, '${o.entityName ?? o.entity} (Removed object)'),
      _ => (FIcons.tune, '${o.op} $component'),
    };
    return [
      InkWell(
        key: ValueKey('prefab.override.${o.index}'),
        onTap: () => setState(() => _open = open ? null : o.index),
        child: Container(
          color: open ? t.selectedRow(focused: true).withValues(alpha: .35) : null,
          padding: const EdgeInsets.fromLTRB(26, 3, 12, 3),
          child: Row(
            children: [
              Icon(icon, size: 12, color: t.accent),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  text,
                  style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
        ),
      ),
      if (open)
        Padding(
          key: ValueKey('prefab.compare.${o.index}'),
          padding: const EdgeInsets.fromLTRB(26, 2, 12, 6),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (o.op == 'setField' || o.op == 'unsetField') ...[
                _compare(context, 'Prefab', _prefabText(o)),
                _compare(context, 'Override', o.op == 'unsetField' ? '(default)' : prettyJson(o.value)),
              ] else
                Text(_explain(o), style: theme.textTheme.bodySmall?.copyWith(color: t.textSecondary)),
              const SizedBox(height: 4),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                    key: ValueKey('prefab.revert.${o.index}'),
                    onPressed: model.canEdit ? () => model.revertOverride(widget.entityId, index: o.index) : null,
                    child: const Text('Revert'),
                  ),
                  Tooltip(
                    message: applyWhy ?? 'Write this override into the prefab',
                    child: _ApplyButton(
                      keyName: 'prefab.apply.${o.index}',
                      label: 'Apply',
                      targets: model.applyTargets(widget.entityId),
                      onApply: applyWhy == null
                          ? (depth) => model.applyOverrides(widget.entityId, index: o.index, depth: depth)
                          : null,
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
    ];
  }

  String _prefabText(ExpansionOverride o) {
    final (has, v) = widget.model.prefabValueOf(widget.entityId, o);
    return has ? prettyJson(v) : '(default)';
  }

  Widget _compare(BuildContext context, String label, String value) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 2),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 64,
            child: Text(label, style: theme.textTheme.labelSmall?.copyWith(color: context.tokens.textSecondary)),
          ),
          Expanded(
            child: SelectableText(
              value,
              key: ValueKey('prefab.compare.$label'),
              style: theme.textTheme.bodySmall,
              maxLines: 4,
            ),
          ),
        ],
      ),
    );
  }

  static String _explain(ExpansionOverride o) => switch (o.op) {
    'addComponent' => 'The instance has a component the prefab does not.',
    'removeComponent' => 'The instance leaves out a component the prefab has.',
    'addObject' => 'The instance has an object the prefab does not.',
    'removeObject' => 'The instance leaves out an object the prefab has.',
    _ => o.op,
  };

  static String _short(String type) => type.contains('.') ? type.substring(type.lastIndexOf('.') + 1) : type;
}

/// Apply, or — for an instance of a variant — Apply ▾ with one item per
/// prefab it can go to (Unity's "Apply to Prefab Variant 'X'" / "Apply to
/// Prefab 'Base'").
class _ApplyButton extends StatelessWidget {
  const _ApplyButton({
    required this.keyName,
    required this.label,
    required this.targets,
    required this.onApply,
    this.filled = false,
  });

  final String keyName;
  final String label;
  final List<ApplyTarget> targets;
  final void Function(int depth)? onApply;
  final bool filled;

  @override
  Widget build(BuildContext context) {
    final apply = onApply;
    if (targets.length < 2) {
      final press = apply == null ? null : () => apply(0);
      return filled
          ? FilledButton(key: ValueKey(keyName), onPressed: press, child: Text(label))
          : TextButton(key: ValueKey(keyName), onPressed: press, child: Text(label));
    }
    return MenuAnchor(
      menuChildren: [
        for (final t in targets)
          MenuItemButton(
            key: ValueKey('$keyName.target.${t.depth}'),
            onPressed: apply == null ? null : () => apply(t.depth),
            child: Text('$label to ${applyTargetLabel(t)}'),
          ),
      ],
      builder: (context, controller, _) {
        final press = apply == null ? null : () => controller.isOpen ? controller.close() : controller.open();
        final child = Row(
          mainAxisSize: MainAxisSize.min,
          children: [Text(label), const SizedBox(width: 2), const Icon(FIcons.expandMore, size: 14)],
        );
        return filled
            ? FilledButton(key: ValueKey(keyName), onPressed: press, child: child)
            : TextButton(key: ValueKey(keyName), onPressed: press, child: child);
      },
    );
  }
}
