// The object header's layer and tags (ADR-0020): the entity's
// `fynkel.Layer` as a dropdown of the project's layers and its
// `fynkel.Tags` as chips with a menu of the project's tags — never as two
// component cards with numbers in them. Several selected entities show a
// mixed layer as `—` and a tag only some of them carry as a dim chip.
import 'package:flutter/material.dart';

import '../theme/icons.dart';
import '../theme/tokens.dart';
import 'field_widgets.dart';
import 'inspector_model.dart';

/// The Layer and Tags rows under the header's name.
class LayerTagsRows extends StatelessWidget {
  const LayerTagsRows({super.key, required this.model, this.onEditLayers, this.onEditTags});

  final InspectorModel model;

  /// Opens the project's Tags and Layers settings; null hides the entries.
  final VoidCallback? onEditLayers;
  final VoidCallback? onEditTags;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        FieldRow(
          key: const ValueKey('inspector.layer'),
          label: 'Layer',
          tooltip: 'What the object is for physics (what it collides with), cameras and lights',
          child: LayerField(
            value: model.layer,
            layers: model.layerNames,
            enabled: model.canEdit,
            onCommit: model.setLayer,
            onEdit: onEditLayers,
          ),
        ),
        FieldRow(
          key: const ValueKey('inspector.tags'),
          label: 'Tags',
          tooltip: 'What the object is for gameplay: world.findWithTag, go.hasTag',
          child: TagsField(model: model, onEdit: onEditTags),
        ),
      ],
    );
  }
}

/// A layer index as the project's layer names; null is mixed. An index
/// past the project's list shows as `Layer N`.
class LayerField extends StatelessWidget {
  const LayerField({
    super.key,
    required this.value,
    required this.layers,
    required this.onCommit,
    this.enabled = true,
    this.onEdit,
  });

  final int? value;
  final List<String> layers;
  final bool enabled;
  final void Function(int index) onCommit;
  final VoidCallback? onEdit;

  static const _edit = -1;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final v = value;
    final shown = v == null ? '—' : (v < layers.length ? layers[v] : 'Layer $v');
    return SizedBox(
      height: context.tokens.fieldHeight,
      child: PopupMenuButton<int>(
        popUpAnimationStyle: AnimationStyle.noAnimation,
        enabled: enabled,
        tooltip: '',
        onSelected: (i) => i == _edit ? onEdit?.call() : onCommit(i),
        itemBuilder: (_) => [
          for (var i = 0; i < layers.length; i++)
            PopupMenuItem(
              key: ValueKey('layer.$i'),
              value: i,
              height: 26,
              child: Row(
                children: [
                  SizedBox(width: 18, child: i == v ? const Icon(FIcons.check, size: 14) : null),
                  Text(layers[i], style: theme.textTheme.bodySmall),
                ],
              ),
            ),
          if (onEdit != null) ...[
            const PopupMenuDivider(),
            PopupMenuItem(
              key: const ValueKey('layer.edit'),
              value: _edit,
              height: 26,
              child: Text('Edit Layers…', style: theme.textTheme.bodySmall),
            ),
          ],
        ],
        child: FieldBox(
          padding: const EdgeInsets.only(left: 6, right: 4),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  shown,
                  key: const ValueKey('layer.summary'),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: v == null ? context.tokens.textDisabled : context.tokens.text,
                  ),
                ),
              ),
              Icon(FIcons.chevronDown, size: 12, color: context.tokens.textDisabled),
            ],
          ),
        ),
      ),
    );
  }
}

/// The selection's tags as chips (a tag only some entities carry is dim),
/// each with a remove button, and a `+` menu of every project tag.
class TagsField extends StatelessWidget {
  const TagsField({super.key, required this.model, this.onEdit});

  final InspectorModel model;
  final VoidCallback? onEdit;

  static const _edit = -1;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final names = model.tagNames;
    String name(int i) => i < names.length ? names[i] : 'Tag $i';
    final enabled = model.canEdit;
    return Wrap(
      spacing: 4,
      runSpacing: 4,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        for (final i in model.tagsInUse)
          _TagChip(
            key: ValueKey('tag.chip.$i'),
            label: name(i),
            mixed: model.tagState(i) == TagState.mixed,
            onRemove: enabled ? () => model.setTag(i, false) : null,
          ),
        SizedBox(
          height: t.fieldHeight,
          child: PopupMenuButton<int>(
            key: const ValueKey('tag.add'),
            popUpAnimationStyle: AnimationStyle.noAnimation,
            enabled: enabled,
            tooltip: 'Add or remove tags',
            onSelected: (i) {
              if (i == _edit) return onEdit?.call();
              model.setTag(i, model.tagState(i) != TagState.all);
            },
            itemBuilder: (_) => [
              if (names.isEmpty)
                PopupMenuItem(
                  enabled: false,
                  height: 26,
                  child: Text('The project declares no tags', style: theme.textTheme.bodySmall),
                ),
              for (var i = 0; i < names.length; i++)
                PopupMenuItem(
                  key: ValueKey('tag.$i'),
                  value: i,
                  height: 26,
                  child: Row(
                    children: [
                      SizedBox(
                        width: 18,
                        child: switch (model.tagState(i)) {
                          TagState.all => const Icon(FIcons.check, size: 14),
                          TagState.mixed => const Icon(FIcons.remove, size: 14),
                          TagState.none => null,
                        },
                      ),
                      Text(names[i], style: theme.textTheme.bodySmall),
                    ],
                  ),
                ),
              if (onEdit != null) ...[
                const PopupMenuDivider(),
                PopupMenuItem(
                  key: const ValueKey('tag.edit'),
                  value: _edit,
                  height: 26,
                  child: Text('Edit Tags…', style: theme.textTheme.bodySmall),
                ),
              ],
            ],
            child: FieldBox(
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(FIcons.add, size: 12, color: enabled ? t.textSecondary : t.textDisabled),
                  if (model.tagsInUse.isEmpty) ...[
                    const SizedBox(width: 3),
                    Text('Add tag', style: theme.textTheme.bodySmall?.copyWith(color: t.textDisabled)),
                  ],
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _TagChip extends StatelessWidget {
  const _TagChip({super.key, required this.label, required this.mixed, this.onRemove});

  final String label;
  final bool mixed;
  final VoidCallback? onRemove;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final color = mixed ? t.textDisabled : t.text;
    return Tooltip(
      message: mixed ? '$label — on some of the selected entities' : label,
      waitDuration: const Duration(milliseconds: 500),
      child: Container(
        height: t.fieldHeight,
        padding: const EdgeInsets.only(left: 6, right: 2),
        decoration: BoxDecoration(
          color: t.surfaceOf(t.accent, mixed ? .06 : .16),
          border: Border.all(color: t.surfaceOf(t.accent, mixed ? .2 : .45)),
          borderRadius: t.cornerControl,
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(label, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: color)),
            if (onRemove != null)
              IconButton(
                key: const ValueKey('tag.remove'),
                tooltip: 'Remove $label',
                icon: Icon(FIcons.close, size: 11, color: color),
                padding: EdgeInsets.zero,
                constraints: const BoxConstraints(minWidth: 16, minHeight: 16),
                onPressed: onRemove,
              )
            else
              const SizedBox(width: 4),
          ],
        ),
      ),
    );
  }
}
