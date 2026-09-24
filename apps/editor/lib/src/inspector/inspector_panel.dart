// The Inspector panel (blueprint §19.2e item 4): the selection's header
// (name, id, count; the layer and tags, ADR-0020), the "run `fynkel codegen`" banner, one collapsible
// card per component slot with its fields grouped by the `group`/`header`
// hints and filtered by `showIf`, the card menu (Remove, Reset to defaults,
// Open script, Copy type), the prefab instance's Prefab bar, and Add Component as a searchable popup. Everything it writes goes
// through `InspectorModel` — one transaction per gesture.
//
// Prefab mode (item 7, ADR-0022; Unity's layout since the parity pass): an
// instance (the prefab's root) opens with the Prefab bar — its prefab,
// Open, Select and the Overrides dropdown (overrides_popup.dart), a missing
// or broken prefab with its diagnostics. Overridden fields are bold with a
// blue bar in the margin and Apply / Revert on a right-click; an added
// component carries a '+' and Apply / Revert Added Component in its menu;
// a derived entity's banner names the instance it belongs to.
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../env/tools.dart';
import '../picker/picker.dart';
import '../project_browser/asset_preview.dart';
import '../project_browser/project_browser_model.dart';
import '../shell/editor_selection.dart';
import '../shell/notices.dart';
import '../theme/icons.dart';
import 'asset_pages.dart';
import 'codegen_check.dart';
import 'field_spec.dart';
import 'overrides_popup.dart';
import 'field_widgets.dart';
import 'inspector_chrome.dart';
import 'inspector_math.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'inspector_model.dart';
import 'prefab_actions.dart' show applyTargetLabel;
import 'asset_edits.dart';
import 'object_header.dart';
import '../project/tags_and_layers_dialog.dart';

class InspectorPanel extends StatefulWidget {
  const InspectorPanel({
    super.key,
    required this.model,
    this.selection,
    this.browser,
    this.onOpenDocument,
    this.onOpenReference,
    this.run = runProcess,
    this.assetEdits,
  });

  final InspectorModel model;

  /// A material asset's page edits it through this (Tier 2).
  final AssetEdits? assetEdits;

  /// What to show (item 9c): entities, an asset, a part, a folder. Null
  /// shows the entity page alone (a test that pumps the model by itself).
  final EditorSelection? selection;
  final ProjectBrowserModel? browser;
  final void Function(String id)? onOpenDocument;
  final void Function(String documentPath, String? pointer)? onOpenReference;
  final RunProcess run;

  @override
  State<InspectorPanel> createState() => _InspectorPanelState();
}

class _InspectorPanelState extends State<InspectorPanel> {
  final _collapsed = <String>{};

  /// The cards drawn last, by slot key: while the model's generation holds
  /// (only the running game's values moved) a card whose slot is the same
  /// object is handed back as it was and not built again.
  final _cards = <String, ({ComponentSlot slot, int generation, bool collapsed, Widget card})>{};

  /// The same one level down: a field whose value did not move keeps its
  /// widget when its card is built again (a moving position rebuilds three
  /// number fields, not the Transform's nine).
  final _fieldWidgets = <String, _KeptField>{};

  InspectorModel get model => widget.model;

  @override
  void initState() {
    super.initState();
    model.addListener(_onModel);
    widget.selection?.addListener(_onModel);
  }

  @override
  void didUpdateWidget(covariant InspectorPanel old) {
    super.didUpdateWidget(old);
    if (old.model != model) {
      old.model.removeListener(_onModel);
      model.addListener(_onModel);
      _cards.clear();
      _fieldWidgets.clear();
      _chrome.clear();
    }
    if (old.selection != widget.selection) {
      old.selection?.removeListener(_onModel);
      widget.selection?.addListener(_onModel);
    }
  }

  @override
  void dispose() {
    model.removeListener(_onModel);
    widget.selection?.removeListener(_onModel);
    super.dispose();
  }

  void _onModel() {
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final browser = widget.browser;
    final sel = widget.selection;
    final target = sel?.target;
    if (browser != null) {
      void openDocument(String id) => widget.onOpenDocument?.call(id);
      void openReference(String doc, String? pointer) => widget.onOpenReference?.call(doc, pointer);
      switch (target) {
        case InspectAsset(:final id, sub: null):
          return AssetPage(
            key: ValueKey('inspector.asset.$id'),
            model: browser,
            id: id,
            selection: sel,
            onOpenDocument: openDocument,
            onOpenReference: openReference,
            run: widget.run,
            assetEdits: widget.assetEdits,
          );
        case InspectAsset(:final id, :final sub?):
          return SubAssetPage(
            key: ValueKey('inspector.part.$id.$sub'),
            model: browser,
            id: id,
            sub: sub,
            selection: sel,
            onOpenDocument: openDocument,
            onOpenReference: openReference,
          );
        case InspectFolder(:final rel):
          return FolderPage(
            key: ValueKey('inspector.folder.$rel'),
            model: browser,
            rel: rel,
            selection: sel,
            run: widget.run,
          );
        case InspectEntities() || null:
          break;
      }
    }
    if (target == null && sel != null && !model.hasDocument) {
      return const InspectorEmpty(
        key: ValueKey('inspector.empty'),
        icon: FIcons.touchAppOutlined,
        title: 'Nothing selected',
        message: 'Select an entity in the Hierarchy, or an asset or a folder in the Project panel.',
      );
    }
    return _entityPage(context);
  }

  /// A script dropped on the Inspector goes onto the selection (Unity's
  /// script drop; Add Component's rules, so not twice on one object).
  Widget _scriptDrop(Widget child) => DragTarget<AssetDragPayload>(
    onWillAcceptWithDetails: (d) =>
        d.data.kind == 'script' && model.canEdit && model.entities.isNotEmpty && model.entitiesWith(d.data.id).isEmpty,
    onAcceptWithDetails: (d) => model.addComponent(d.data.id),
    builder: (context, candidates, _) => DecoratedBox(
      key: const ValueKey('inspector.scriptDrop'),
      position: DecorationPosition.foreground,
      decoration: BoxDecoration(
        border: candidates.isEmpty ? null : Border.all(color: Theme.of(context).colorScheme.primary, width: 2),
      ),
      child: child,
    ),
  );

  Widget _entityPage(BuildContext context) {
    final theme = Theme.of(context);
    if (!model.hasDocument) return _empty(context, FIcons.tune, 'Open a document and select an entity.');
    final banner = _banner(context);
    if (model.tooMany) {
      return Column(
        children: [
          ?banner,
          Expanded(
            child: _empty(
              context,
              FIcons.selectAll,
              '${model.selection.length} entities selected.\nSelect up to ${model.maxEntities} to edit them together.',
            ),
          ),
        ],
      );
    }
    if (model.entities.isEmpty) {
      return Column(
        children: [
          ?banner,
          Expanded(
            child: _empty(
              context,
              FIcons.touchAppOutlined,
              widget.selection == null
                  ? 'Nothing selected.\nClick an entity in the Hierarchy.'
                  : 'Nothing selected.\nClick an entity in the Hierarchy, or an asset in the Project panel.',
            ),
          ),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ?banner,
        _kept('header', () => _header(context)),
        Expanded(
          child: _scriptDrop(
            PrefabNameScope(
              targets: _applyTargets(),
              child: CustomScrollView(
              key: const ValueKey('inspector.list'),
              slivers: [
                if (model.single?.prefab != null)
                  SliverToBoxAdapter(
                    child: PrefabBar(model: model, entity: model.single!),
                  ),
                if (model.single?.isDerived ?? false)
                  SliverToBoxAdapter(
                    child: _DerivedBanner(model: model, entity: model.single!),
                  ),
                for (final slot in model.slots) _card(slot),
                _kept('footer', () => SliverPadding(
                  padding: const EdgeInsets.only(bottom: 12),
                  sliver: SliverList.list(
                    children: [
                      Padding(
                        padding: const EdgeInsets.fromLTRB(12, 10, 12, 0),
                        child: Builder(
                          builder: (button) => OutlinedButton.icon(
                            key: const ValueKey('inspector.addComponent'),
                            onPressed: model.canEdit
                                ? () => showAddComponent(context, model, anchor: globalRectOf(button))
                                : null,
                            icon: const Icon(FIcons.add),
                            label: const Text('Add Component'),
                          ),
                        ),
                      ),
                      if (model.lastError != null && model.errors.isEmpty)
                        Padding(
                          padding: const EdgeInsets.fromLTRB(12, 8, 4, 0),
                          child: Row(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Expanded(
                                child: SelectableText(
                                  model.lastError!,
                                  key: const ValueKey('inspector.lastError'),
                                  style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.error),
                                ),
                              ),
                              IconButton(
                                key: const ValueKey('inspector.dismissError'),
                                tooltip: 'Dismiss',
                                icon: const Icon(FIcons.close, size: 14),
                                padding: EdgeInsets.zero,
                                constraints: const BoxConstraints(minWidth: 24, minHeight: 24),
                                onPressed: model.dismissError,
                              ),
                            ],
                          ),
                        ),
                    ],
                  ),
                )),
              ],
            ),
            ),
          ),
        ),
      ],
    );
  }

  /// Where a field's Apply would write: the selection's instance's prefab,
  /// then (a variant's) its bases.
  List<String> _applyTargets() {
    final e = model.single;
    if (e == null || (e.prefab == null && e.anchor == null)) return const [];
    return [
      for (final t in model.applyTargets(e.anchor ?? e.id))
        if (t.name.isNotEmpty) applyTargetLabel(t),
    ];
  }

  /// The page around the cards (the header, the Add Component section)
  /// built once per generation and theme, like the cards.
  final _chrome = <String, ({int generation, ThemeData theme, Widget widget})>{};

  Widget _kept(String what, Widget Function() build) {
    final theme = Theme.of(context);
    final old = _chrome[what];
    if (old != null && old.generation == model.generation && identical(old.theme, theme)) return old.widget;
    final w = build();
    _chrome[what] = (generation: model.generation, theme: theme, widget: w);
    return w;
  }

  Widget _card(ComponentSlot slot) {
    final collapsed = _collapsed.contains(slot.key);
    final kept = _cards[slot.key];
    if (kept != null &&
        identical(kept.slot, slot) &&
        kept.generation == model.generation &&
        kept.collapsed == collapsed) {
      return kept.card;
    }
    if (_cards.length > 64) _cards.clear();
    if (_fieldWidgets.length > 512) _fieldWidgets.clear();
    final card = _ComponentCard(
      key: ValueKey('card.${slot.key}'),
      model: model,
      slot: slot,
      kept: _fieldWidgets,
      collapsed: collapsed,
      onToggle: () => setState(
        () => _collapsed.contains(slot.key) ? _collapsed.remove(slot.key) : _collapsed.add(slot.key),
      ),
    );
    _cards[slot.key] = (slot: slot, generation: model.generation, collapsed: collapsed, card: card);
    return card;
  }

  Widget _empty(BuildContext context, IconData icon, String text) {
    final theme = Theme.of(context);
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 32, color: context.tokens.textDisabled),
          const SizedBox(height: 8),
          Text(
            text,
            textAlign: TextAlign.center,
            style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
          ),
        ],
      ),
    );
  }

  Widget _header(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final single = model.single;
    if (single != null) {
      return Container(
        decoration: BoxDecoration(
          color: t.header,
          border: Border(bottom: t.hairline),
        ),
        padding: EdgeInsets.fromLTRB(t.pad, 5, t.pad, 5),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                ActiveToggle(model: model),
                const SizedBox(width: 2),
                Icon(
                  single.prefab != null ? FIcons.widgetsOutlined : FIcons.categoryOutlined,
                  size: t.icon,
                  color: single.prefab != null ? t.accent : t.textSecondary,
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: StringField(
                    key: const ValueKey('inspector.name'),
                    value: single.name,
                    enabled: model.canRename,
                    hint: 'Name',
                    onCommit: model.rename,
                  ),
                ),
                if (widget.selection != null) ...[
                  const SizedBox(width: 2),
                  InspectorLockButton(selection: widget.selection!),
                ],
              ],
            ),
            const SizedBox(height: 2),
            Row(
              children: [
                Expanded(child: SelectableText(single.id, style: monoStyle(context, size: 10))),
                IconButton(
                  tooltip: 'Copy id',
                  icon: const Icon(FIcons.copy, size: 12),
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(minWidth: 20, minHeight: 20),
                  onPressed: () => Clipboard.setData(ClipboardData(text: single.id)),
                ),
              ],
            ),
            _layerTags(context),
          ],
        ),
      );
    }
    final n = model.entities.length;
    return Container(
      decoration: BoxDecoration(
        color: t.header,
        border: Border(bottom: t.hairline),
      ),
      padding: EdgeInsets.fromLTRB(t.pad, 5, t.pad, 5),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              ActiveToggle(model: model),
              const SizedBox(width: 2),
              Icon(FIcons.selectAll, size: t.icon, color: t.textSecondary),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  '$n entities — ${model.entities.map((e) => e.name).take(3).join(', ')}${n > 3 ? ', …' : ''}',
                  key: const ValueKey('inspector.multi'),
                  style: theme.textTheme.bodySmall,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              if (widget.selection != null) InspectorLockButton(selection: widget.selection!),
            ],
          ),
          _layerTags(context),
        ],
      ),
    );
  }

  /// The codegen banner: committed drift (instant) or the CLI check's answer.
  /// The header's Layer and Tags rows; their menus' "Edit…" entries open
  /// the project's Tags and Layers.
  Widget _layerTags(BuildContext context) {
    final model = widget.model;
    return LayerTagsRows(
      model: model,
      onEditLayers: () => showTagsAndLayersDialog(context, model.project),
      onEditTags: () => showTagsAndLayersDialog(context, model.project, focus: TagsAndLayersFocus.tags),
    );
  }

  Widget? _banner(BuildContext context) {
    final theme = Theme.of(context);
    final drift = model.hasMetadata ? model.committedDrift : const <CommittedDrift>[];
    final check = model.codegen;
    final status = check?.status ?? CodegenStatus.unknown;
    String? headline;
    String? detail;
    var isError = false;
    if (drift.isNotEmpty) {
      headline = 'Behaviour metadata is out of date — run `fynkel codegen`.';
      detail = drift.map((d) => '${d.type}: ${d.reason}').join('\n');
    } else if (status == CodegenStatus.stale) {
      headline = 'Generated behaviour files are stale — run `fynkel codegen`.';
      detail = check!.stalePaths.isEmpty ? null : check.stalePaths.join('\n');
    } else if (status == CodegenStatus.failed && check!.watching) {
      headline = 'A script has an error: the Inspector keeps its last fields, and Play waits until it is fixed.';
      detail = check.error;
      isError = true;
    } else if (status == CodegenStatus.failed) {
      headline = 'The codegen check could not run.';
      detail = check!.error;
      isError = true;
    }
    if (headline == null) return null;
    final t = context.tokens;
    final color = t.surfaceOf(isError ? t.error : t.warning, .2);
    final onColor = isError ? t.error : t.warning;
    return Material(
      key: const ValueKey('inspector.codegenBanner'),
      color: color,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 8, 8, 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(isError ? FIcons.errorOutline : FIcons.syncProblem, size: t.icon, color: onColor),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    headline,
                    style: theme.textTheme.bodySmall?.copyWith(color: onColor, fontWeight: FontWeight.w600),
                  ),
                ),
              ],
            ),
            if (detail != null && detail.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 4, left: 22),
                child: Text(
                  detail,
                  style: theme.textTheme.labelSmall?.copyWith(color: onColor, fontFamily: 'monospace'),
                  maxLines: 6,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            if (check != null && !check.watching)
              Padding(
                padding: const EdgeInsets.only(top: 4, left: 14),
                child: Wrap(
                  spacing: 4,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    TextButton.icon(
                      key: const ValueKey('inspector.copyCommand'),
                      style: TextButton.styleFrom(foregroundColor: onColor, visualDensity: VisualDensity.compact),
                      onPressed: () => Clipboard.setData(ClipboardData(text: check.command)),
                      icon: const Icon(FIcons.copy, size: 14),
                      label: Text(check.command, style: const TextStyle(fontFamily: 'monospace', fontSize: 11)),
                    ),
                    TextButton.icon(
                      key: const ValueKey('inspector.checkAgain'),
                      style: TextButton.styleFrom(foregroundColor: onColor, visualDensity: VisualDensity.compact),
                      onPressed: status == CodegenStatus.running ? null : check.check,
                      icon: status == CodegenStatus.running
                          ? const SizedBox(width: 12, height: 12, child: CircularProgressIndicator(strokeWidth: 1.5))
                          : const Icon(FIcons.refresh, size: 14),
                      label: const Text('Check again', style: TextStyle(fontSize: 11)),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}

// ── Component card ─────────────────────────────────────────────────────────

/// The card's Edit toggle (item 9g): the component's handles in the Scene
/// view — a light's range, a collider's size, a camera's field of view.
/// During Play: a right click on a field row (its label — a text box keeps
/// its own menu) offers Apply Field to Scene (ADR-0021 P7 L4).
class _ApplyFieldMenu extends StatelessWidget {
  const _ApplyFieldMenu({required this.model, required this.slot, required this.field, required this.child});

  final InspectorModel model;
  final ComponentSlot slot;
  final String field;
  final Widget child;

  @override
  Widget build(BuildContext context) => GestureDetector(
    behavior: HitTestBehavior.translucent,
    onSecondaryTapUp: (d) async {
      final overlay = Overlay.of(context).context.findRenderObject() as RenderBox;
      final at = overlay.globalToLocal(d.globalPosition);
      final why = model.cannotApplyToScene(slot);
      final v = await showMenu<String>(
        context: context,
        popUpAnimationStyle: AnimationStyle.noAnimation,
        position: RelativeRect.fromRect(at & const Size(1, 1), Offset.zero & overlay.size),
        items: [
          PopupMenuItem(
            value: 'apply',
            enabled: why == null,
            height: 28,
            child: Tooltip(
              message: why ?? 'Write this running value into the scene (its tab goes unsaved)',
              child: const Text('Apply Field to Scene'),
            ),
          ),
        ],
      );
      if (v == 'apply') model.applyToScene(slot, field: field);
    },
    child: child,
  );
}

class _EditInSceneButton extends StatelessWidget {
  const _EditInSceneButton({required this.model, required this.slot});

  final InspectorModel model;
  final ComponentSlot slot;

  @override
  Widget build(BuildContext context) {
    final t = context.tokens;
    final on = model.isEditingInScene(slot);
    return Tooltip(
      message: on ? 'Stop editing in the Scene view (Esc)' : 'Edit in the Scene view: drag its handles',
      waitDuration: const Duration(milliseconds: 600),
      child: InkWell(
        key: ValueKey('card.editInScene.${slot.key}'),
        borderRadius: BorderRadius.circular(4),
        onTap: () => model.toggleEditInScene(slot),
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 2),
          margin: const EdgeInsets.only(right: 2),
          decoration: BoxDecoration(
            color: on ? t.accent.withValues(alpha: 0.22) : null,
            borderRadius: BorderRadius.circular(4),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(FIcons.editShape, size: 13, color: on ? t.accent : t.textSecondary),
              const SizedBox(width: 3),
              Text(
                'Edit',
                style: Theme.of(context).textTheme.labelSmall?.copyWith(color: on ? t.accent : t.textSecondary),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

typedef _KeptField = ({int generation, Object? value, bool mixed, Widget widget});

/// A component card's header being dragged.
final class _CardDrag {
  const _CardDrag(this.slot);

  final ComponentSlot slot;
}

class _ComponentCard extends StatelessWidget {
  const _ComponentCard({
    super.key,
    required this.model,
    required this.slot,
    required this.collapsed,
    required this.onToggle,
    this.kept,
  });

  final InspectorModel model;
  final ComponentSlot slot;

  /// The panel's field widgets from the last build, by `slot.key/field`.
  final Map<String, _KeptField>? kept;
  final bool collapsed;
  final VoidCallback onToggle;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final t = context.tokens;
    final spec = slot.spec;
    // Unity marks an added component with a '+' and a blue bar, and a
    // component with overridden fields with the bar alone.
    final added = model.addedComponentOverride(slot);
    final marked = added != null || slot.instances.any((i) => i.overridden.isNotEmpty);
    final prefabName = added == null ? null : model.expansionOf(added.$1)?.prefab?.name;
    final applyAddedWhy = added == null ? null : model.cannotApply(added.$1);
    return SliverMainAxisGroup(
      slivers: [
        PinnedHeaderSliver(
          child: _reorder(
            context,
            Material(
            color: t.header,
            child: InkWell(
              onTap: onToggle,
              child: Container(
                height: t.row + 6,
                decoration: BoxDecoration(
                  color: t.header,
                  border: Border(top: t.hairline, bottom: t.hairline),
                ),
                padding: const EdgeInsets.only(left: 2, right: 2),
                child: Row(
                  children: [
                    if (marked)
                      Container(
                        key: ValueKey('card.overrideBar.${slot.key}'),
                        width: 2,
                        height: t.row,
                        margin: const EdgeInsets.only(right: 2),
                        color: t.accent,
                      ),
                    Icon(collapsed ? FIcons.chevronRight : FIcons.expandMore, size: 14, color: t.textSecondary),
                    const SizedBox(width: 2),
                    Stack(
                      clipBehavior: Clip.none,
                      children: [
                        Icon(componentIcon(spec), size: 14, color: t.textSecondary),
                        if (added != null)
                          Positioned(
                            right: -4,
                            bottom: -3,
                            child: Icon(FIcons.add, key: ValueKey('card.added.${slot.key}'), size: 10, color: t.accent),
                          ),
                      ],
                    ),
                    // Unity: a component's `enabled` is the header's checkbox.
                    if (_enabledField != null)
                      SizedBox(
                        width: 22,
                        height: 18,
                        child: Checkbox(
                          key: ValueKey('card.enabled.${slot.key}'),
                          tristate: slot.isMixed('enabled'),
                          value: slot.isMixed('enabled') ? null : slot.shown('enabled') != false,
                          visualDensity: VisualDensity.compact,
                          materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                          onChanged: model.canEdit
                              ? (v) => model.setField(slot, 'enabled', v ?? true, spec: _enabledField)
                              : null,
                        ),
                      )
                    else
                      const SizedBox(width: 7),
                    Expanded(
                      child: Tooltip(
                        message: spec.description ?? spec.type,
                        waitDuration: const Duration(milliseconds: 600),
                        child: Text.rich(
                          TextSpan(
                            text: spec.title,
                            style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600, color: t.text),
                            children: [
                              if (spec.isBehaviour)
                                TextSpan(
                                  text: '  ${spec.namespace}',
                                  style: theme.textTheme.labelSmall?.copyWith(color: t.textDisabled),
                                ),
                              if (slot.index > 0)
                                TextSpan(
                                  text: '  #${slot.index + 1}',
                                  style: theme.textTheme.labelSmall?.copyWith(color: t.textDisabled),
                                ),
                            ],
                          ),
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    ),
                    if (model.canEditInScene(slot)) _EditInSceneButton(model: model, slot: slot),
                    if (spec.missingSchema)
                      Tooltip(
                        message: 'No schema for ${spec.type}: fields are shown as JSON. Run `fynkel codegen`.',
                        child: Icon(FIcons.helpOutline, size: 13, color: t.error),
                      ),
                    PopupMenuButton<String>(
                      popUpAnimationStyle: AnimationStyle.noAnimation,
                      key: ValueKey('card.menu.${slot.key}'),
                      tooltip: '',
                      padding: EdgeInsets.zero,
                      iconSize: 14,
                      // Its name for a screen reader; no hover tooltip.
                      icon: Icon(Icons.adaptive.more, semanticLabel: '${spec.title} options'),
                      onSelected: (v) => _menu(context, v),
                      itemBuilder: (_) => [
                        const PopupMenuItem(
                          key: Key('card.menu.copyValues'),
                          value: 'copyValues',
                          height: 28,
                          child: Text('Copy Component'),
                        ),
                        PopupMenuItem(
                          key: const Key('card.menu.pasteValues'),
                          value: 'pasteValues',
                          enabled: model.canPasteValues(slot),
                          height: 28,
                          child: const Text('Paste Component Values'),
                        ),
                        const PopupMenuDivider(),
                        PopupMenuItem(
                          value: 'reset',
                          enabled: model.canEdit,
                          height: 28,
                          child: const Text('Reset to Defaults'),
                        ),
                        if (spec.isBehaviour)
                          const PopupMenuItem(value: 'open', height: 28, child: Text('Open Script')),
                        const PopupMenuItem(value: 'copyType', height: 28, child: Text('Copy Type Name')),
                        if (added != null) ...[
                          const PopupMenuDivider(),
                          PopupMenuItem(
                            key: const Key('card.menu.applyAdded'),
                            value: 'applyAdded',
                            enabled: applyAddedWhy == null,
                            height: 28,
                            child: Tooltip(
                              message: applyAddedWhy ?? 'Add this component to the prefab',
                              child: Text(
                                prefabName == null || prefabName.isEmpty
                                    ? 'Apply Added Component to Prefab'
                                    : "Apply Added Component to Prefab '$prefabName'",
                              ),
                            ),
                          ),
                          PopupMenuItem(
                            key: const Key('card.menu.revertAdded'),
                            value: 'revertAdded',
                            enabled: model.canEdit,
                            height: 28,
                            child: const Text('Revert Added Component'),
                          ),
                        ],
                        if (model.showsPlay) ...[
                          const PopupMenuDivider(),
                          PopupMenuItem(
                            value: 'applyToScene',
                            enabled: model.cannotApplyToScene(slot) == null,
                            height: 28,
                            child: Tooltip(
                              message: model.cannotApplyToScene(slot) ??
                                  'Write these running values into the scene (its tab goes unsaved)',
                              child: const Text('Apply to Scene'),
                            ),
                          ),
                        ],
                        const PopupMenuDivider(),
                        PopupMenuItem(
                          key: const Key('card.menu.moveUp'),
                          value: 'moveUp',
                          enabled: model.canMove(slot, -1),
                          height: 28,
                          child: const Text('Move Up'),
                        ),
                        PopupMenuItem(
                          key: const Key('card.menu.moveDown'),
                          value: 'moveDown',
                          enabled: model.canMove(slot, 1),
                          height: 28,
                          child: const Text('Move Down'),
                        ),
                        PopupMenuItem(
                          value: 'remove',
                          enabled: model.canEdit && slot.type != transformType,
                          height: 28,
                          child: slot.type == transformType
                              ? const Tooltip(message: transformStays, child: Text('Remove Component'))
                              : const Text('Remove Component'),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ),
          ),
          ),
        ),
        if (!collapsed)
          SliverToBoxAdapter(
            child: Padding(padding: EdgeInsets.fromLTRB(t.pad, 4, t.pad, 8), child: _fields(context)),
          ),
      ],
    );
  }

  /// Unity's drag to reorder: the header drags onto another card's header,
  /// taking its place (a line shows where), as Move Up / Move Down do.
  Widget _reorder(BuildContext context, Widget header) {
    final t = context.tokens;
    final draggable = Draggable<_CardDrag>(
      data: _CardDrag(slot),
      maxSimultaneousDrags: model.canDrag(slot) ? 1 : 0,
      feedback: Material(
        color: t.header,
        elevation: 4,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
          child: Text(slot.spec.title, style: Theme.of(context).textTheme.bodySmall),
        ),
      ),
      childWhenDragging: Opacity(opacity: 0.5, child: header),
      child: header,
    );
    return DragTarget<_CardDrag>(
      onWillAcceptWithDetails: (d) => model.canMoveTo(d.data.slot, slot),
      onAcceptWithDetails: (d) => model.moveComponentTo(d.data.slot, slot),
      builder: (context, candidates, _) {
        final from = candidates.firstOrNull?.slot;
        if (from == null) return draggable;
        // Moving down lands after this card, moving up before it.
        final after = model.slotIndex(from) < model.slotIndex(slot);
        return Stack(
          children: [
            draggable,
            Positioned(
              left: 0,
              right: 0,
              top: after ? null : 0,
              bottom: after ? 0 : null,
              child: Container(key: ValueKey('card.dropLine.${slot.key}'), height: 2, color: t.accent),
            ),
          ],
        );
      },
    );
  }

  Future<void> _menu(BuildContext context, String v) async {
    final notices = Notices.maybeOf(context);
    switch (v) {
      case 'remove':
        model.removeComponent(slot);
      case 'reset':
        model.resetToDefaults(slot);
      case 'copyValues':
        model.copyValues(slot);
      case 'pasteValues':
        model.pasteValues(slot);
      case 'moveUp':
        model.moveComponent(slot, -1);
      case 'moveDown':
        model.moveComponent(slot, 1);
      case 'applyAdded':
        final added = model.addedComponentOverride(slot);
        if (added != null) model.applyOverrides(added.$1, index: added.$2);
      case 'revertAdded':
        final added = model.addedComponentOverride(slot);
        if (added != null) model.revertOverride(added.$1, index: added.$2);
      case 'applyToScene':
        model.applyToScene(slot);
      case 'copyType':
        await Clipboard.setData(ClipboardData(text: slot.type));
      case 'open':
        final err = await model.openScript(slot);
        if (err != null) notices?.show(err);
    }
  }

  /// [f]'s editor — the one built before while the model's generation
  /// holds and the value is the same.
  Widget _field(FieldSpec f) => _kept(f.name, () => _FieldEditor(model: model, slot: slot, spec: f, path: f.name));

  Widget _kept(String name, Widget Function() build) {
    final k = '${slot.key}/$name';
    final value = slot.shown(name);
    final mixed = slot.isMixed(name);
    final old = kept?[k];
    if (old != null && old.generation == model.generation && old.mixed == mixed && deepEquals(old.value, value)) {
      return old.widget;
    }
    final row = model.showsPlay ? _ApplyFieldMenu(model: model, slot: slot, field: name, child: build()) : build();
    final w = RepaintBoundary(key: ValueKey('field.${slot.key}.$name'), child: row);
    kept?[k] = (generation: model.generation, value: value, mixed: mixed, widget: w);
    return w;
  }

  /// The component's boolean `enabled`, shown as the header's checkbox
  /// rather than a row.
  FieldSpec? get _enabledField {
    final f = slot.spec.field('enabled');
    return f != null && f.kind == FieldKind.boolean ? f : null;
  }

  Widget _fields(BuildContext context) {
    final theme = Theme.of(context);
    final spec = slot.spec;
    final data = slot.first.data;
    final rows = <Widget>[];
    String? group;
    var any = false;
    final enabled = _enabledField;
    for (final f in spec.fields) {
      if (identical(f, enabled)) continue;
      if (!f.visibleWith(data, siblings: spec.fields)) continue;
      any = true;
      if (f.group != group) {
        group = f.group;
        if (group != null) {
          rows.add(Padding(padding: const EdgeInsets.only(top: 10, bottom: 3), child: InspectorCaption(group)));
        }
      }
      if (f.header != null) {
        rows.add(Padding(padding: const EdgeInsets.only(top: 10, bottom: 3), child: InspectorCaption(f.header!)));
      }
      rows.add(_field(f));
    }
    // Keys the schema does not list (an open schema, or none): JSON rows.
    if (spec.additionalProperties || spec.missingSchema) {
      for (final key in data.keys) {
        if (spec.field(key) != null) continue;
        any = true;
        rows.add(
          _kept(key, () => FieldRow(
            label: humanize(key),
            tooltip: 'Not in the schema — edited as JSON.',
            error: model.errorFor(slot, key),
            overridden: slot.isOverridden(key),
            onRevert: model.canRevertField(slot, key) ? () => model.revertField(slot, key) : null,
            onApply: slot.isOverridden(key) && model.cannotApplyField(slot, key) == null
                ? (depth) => model.applyField(slot, key, depth: depth)
                : null,
            child: JsonField(
              value: slot.value(key),
              mixed: slot.isMixed(key),
              enabled: model.canEdit,
              onCommit: (v) => model.setField(slot, key, v),
            ),
          )),
        );
      }
    }
    if (!any) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: Text('No fields.', style: theme.textTheme.bodySmall?.copyWith(color: context.tokens.textDisabled)),
      );
    }
    return Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: rows);
  }
}

IconData componentIcon(ComponentSpec spec) {
  final icon = spec.icon;
  if (icon != null) {
    final known = _materialIcons[icon];
    if (known != null) return known;
  }
  return switch (spec.type) {
    'fynkel.Transform' => FIcons.openWith,
    'fynkel.Camera' || 'fynkel.ThirdPersonCamera' || 'fynkel.LookAtCamera' => FIcons.videocamOutlined,
    'fynkel.DirectionalLight' || 'fynkel.PointLight' || 'fynkel.SpotLight' => FIcons.lightbulbOutline,
    'fynkel.Environment' => FIcons.wbSunnyOutlined,
    'fynkel.MeshRenderer' => FIcons.viewInArOutlined,
    'fynkel.Skin' || 'fynkel.Animator' => FIcons.accessibilityNew,
    'fynkel.RigidBody' ||
    'fynkel.Collider' ||
    'fynkel.CharacterController' ||
    'fynkel.TriggerZone' => FIcons.sportsBasketballOutlined,
    'fynkel.Emitter' || 'fynkel.LifetimeDespawn' => FIcons.autoAwesomeOutlined,
    'fynkel.Rotator' || 'fynkel.Oscillate' || 'fynkel.Follow' => FIcons.motionPhotosOnOutlined,
    _ => spec.isBehaviour ? FIcons.code : FIcons.extensionOutlined,
  };
}

/// The `icon` hint names a Material icon; the ones a behaviour is likely to
/// use. An unknown name falls back to the type's default.
const _materialIcons = <String, IconData>{
  'rotate_right': FIcons.rotateRight,
  'rotate_left': FIcons.rotateLeft,
  'directions_run': FIcons.directionsRun,
  'person': FIcons.person,
  'bolt': FIcons.bolt,
  'star': FIcons.star,
  'favorite': FIcons.favorite,
  'flag': FIcons.flag,
  'timer': FIcons.timer,
  'toggle_on': FIcons.toggleOn,
  'door_front': FIcons.doorFrontDoorOutlined,
  'lock': FIcons.lockOutline,
  'key': FIcons.key,
  'sports_esports': FIcons.sportsEsports,
  'gamepad': FIcons.gamepad,
  'code': FIcons.code,
  'extension': FIcons.extension,
  'motion': FIcons.motionPhotosOnOutlined,
  'sensors': FIcons.sensors,
  'music_note': FIcons.musicNote,
  'volume_up': FIcons.volumeUp,
  'visibility': FIcons.visibility,
  'speed': FIcons.speed,
  'settings': FIcons.settings,
  'build': FIcons.build,
  'science': FIcons.science,
  'psychology': FIcons.psychology,
  'smart_toy': FIcons.smartToy,
  'pets': FIcons.pets,
  'eco': FIcons.eco,
  'water_drop': FIcons.waterDrop,
  'local_fire_department': FIcons.localFireDepartment,
  'ac_unit': FIcons.acUnit,
  'brush': FIcons.brush,
  'text_fields': FIcons.textFields,
  'chat': FIcons.chat,
  'shopping_cart': FIcons.shoppingCart,
  'inventory': FIcons.inventory2Outlined,
  'health_and_safety': FIcons.healthAndSafety,
  'shield': FIcons.shield,
  'gps_fixed': FIcons.gpsFixed,
  'explore': FIcons.explore,
  'map': FIcons.map,
  'camera': FIcons.videocamOutlined,
  'light': FIcons.lightbulbOutline,
  'move': FIcons.openWith,
  'physics': FIcons.sportsBasketballOutlined,
};

// ── One field ──────────────────────────────────────────────────────────────

/// Picks the widget for a spec and wires it to the model. [path] is the
/// `data` path (`speed`, `inner/offset`, `/points/2`).
class _FieldEditor extends StatelessWidget {
  const _FieldEditor({
    super.key,
    required this.model,
    required this.slot,
    required this.spec,
    required this.path,
    this.indent = 0,
  });

  final InspectorModel model;
  final ComponentSlot slot;
  final FieldSpec spec;
  final String path;
  final double indent;

  bool get _enabled => model.canEdit && !spec.readOnly;
  String get _pointer => path.startsWith('/') ? path : '/$path';

  /// A field name at the top level writes by name (the crate adds a missing
  /// member); a nested path writes by pointer.
  String get _writePath => path.contains('/') ? _pointer : path;

  void _commit(Object? v) => model.setField(slot, _writePath, v, spec: spec);

  static List<num>? _quaternionOf(Object? v) =>
      v is List && v.length == 4 ? [for (final x in v) x is num ? x : 0] : null;

  /// Where [EulerHints] keeps the angles typed for instance [i]'s rotation.
  String _hintKey(ComponentInstance i) => '${i.entityId}/${i.componentId}/$path';

  /// Instance [i]'s rotation in degrees as the Inspector shows it.
  Euler _eulerOf(ComponentInstance i) {
    final own = _quaternionOf(slot.shownFor(i, path));
    return own == null ? Euler.zero : EulerHints.of(_hintKey(i), own);
  }

  /// Per axis, whether the selected objects' angles differ (to 0.001°).
  List<bool> _mixedAxes() {
    final eulers = [
      for (final i in slot.instances)
        if (_quaternionOf(slot.shownFor(i, path)) != null) _eulerOf(i),
    ];
    if (eulers.length < 2) return const [false, false, false];
    return [
      for (var a = 0; a < 3; a++) eulers.any((e) => (e[a] - eulers.first[a]).abs() > 1e-3),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final value = slot.shown(path);
    final mixed = slot.isMixed(path);
    final error = model.errorFor(
      slot,
      path.startsWith('/') ? path.substring(1).split('/').first : path.split('/').first,
    );
    final label = spec.label + (spec.readOnly ? '  🔒' : '');
    switch (spec.kind) {
      case FieldKind.number:
      case FieldKind.integer:
        final live = model.liveField(slot, path);
        return NumberField(
          value: value is num ? value : null,
          live: live == null ? null : _Number(live),
          previous: value,
          mixed: mixed,
          label: label,
          tooltip: spec.tooltip,
          error: error,
          min: spec.min,
          max: spec.max,
          step: spec.step,
          unit: spec.unit,
          angle: spec.angle,
          slider: spec.slider,
          integer: spec.kind == FieldKind.integer,
          enabled: _enabled,
          overridden: _overridden,
          onRevert: _onRevert,
          onApply: _onApply,
          onCommit: _commit,
        );
      case FieldKind.boolean:
        return _row(
          label,
          error,
          BoolField(value: value is bool ? value : null, mixed: mixed, enabled: _enabled, onCommit: _commit),
        );
      case FieldKind.string:
        return _row(
          label,
          error,
          StringField(
            value: value?.toString(),
            mixed: mixed,
            multiline: spec.multiline,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.enumeration:
        return _row(
          label,
          error,
          EnumField(
            value: value?.toString(),
            values: spec.enumValues,
            mixed: mixed,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.vec2:
      case FieldKind.vec3:
      case FieldKind.vec4:
        final list = value is List ? [for (final v in value) v is num ? v : 0] : null;
        return _row(
          label,
          error,
          unit: spec.unit,
          VecField(
            value: list,
            live: model.liveField(slot, path),
            components: spec.components,
            mixed: [for (var i = 0; i < spec.components; i++) slot.isMixed('$path/$i')],
            enabled: _enabled,
            unit: spec.unit,
            step: spec.step,
            min: spec.min,
            max: spec.max,
            // A Transform's axis drag moves the object in the Scene view live.
            onPreviewComponent: (i, v) {
              if (v == null) return model.endPreview(slot);
              model.previewEach(slot, path, (inst) {
                final own = slot.shownFor(inst, path);
                final list = own is List ? [for (final x in own) x is num ? x.toDouble() : 0.0] : null;
                if (list == null || i >= list.length) return own;
                return [...list]..[i] = v;
              });
            },
            onCommitComponent: (i, v) {
              if (list == null || list.length != spec.components) {
                // No vector yet: write the whole thing.
                final full = List<Object>.filled(spec.components, 0);
                full[i] = v;
                _commit(full);
              } else {
                model.setField(slot, '$_pointer/$i', v);
              }
            },
          ),
        );
      case FieldKind.quaternion:
        final q = value is List && value.length == 4 ? [for (final v in value) v is num ? v : 0] : null;
        return _row(
          label,
          error,
          EulerField(
            quaternion: q,
            hintKey: _hintKey(slot.first),
            live: model.liveField(slot, path),
            mixed: mixed,
            enabled: _enabled,
            onCommit: (quat) => _commit(_numbersLike(quat, q)),
            // Several objects: each keeps its own angles but the edited one.
            mixedAxes: slot.single ? null : _mixedAxes(),
            onPreviewAxis: (axis, degrees) {
              if (degrees == null) return model.endPreview(slot);
              model.previewEach(slot, path, (i) => _eulerOf(i).withComponent(axis, degrees).toQuaternion());
            },
            onCommitAxis: slot.single
                ? null
                : (axis, degrees) => model.setFieldEach(slot, _writePath, (i) {
                    final e = _eulerOf(i).withComponent(axis, degrees);
                    final q = e.toQuaternion();
                    EulerHints.remember(_hintKey(i), q, e);
                    return _numbersLike(q, _quaternionOf(slot.shownFor(i, path)));
                  }),
          ),
        );
      case FieldKind.color:
        final rgb = value is List ? [for (final v in value) v is num ? v : 0] : null;
        return _row(
          label,
          error,
          ColorField(
            value: rgb,
            components: spec.components,
            mixed: mixed,
            enabled: _enabled,
            onCommit: (c) => _commit(_numbersLike(c, rgb)),
            // The Scene view follows the picker; one transaction on OK.
            onPreview: (c) => c == null
                ? model.endPreview(slot)
                : model.previewEach(slot, path, (_) => _numbersLike(c, rgb)),
          ),
        );
      case FieldKind.assetRef:
        return _row(
          label,
          error,
          AssetField(
            value: value is Map ? Map<String, Object?>.from(value) : null,
            kind: spec.assetKind,
            nullable: spec.nullable,
            mixed: mixed,
            enabled: _enabled,
            subject: label,
            describe: model.describeAsset,
            candidates: () => model.assetsFor(spec.assetKind),
            thumbnail: switch (model.assetPreview) {
              final preview? => (e, size) => AssetPreviewWidget(preview: preview(e), size: size),
              null => null,
            },
            onPing: model.pingAsset,
            onCommit: _commit,
          ),
        );
      case FieldKind.entity:
        return _row(
          label,
          error,
          EntityField(
            value: value?.toString(),
            subject: label,
            names: model.entityNames,
            mixed: mixed,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.objectRef:
      case FieldKind.scriptRef:
        return _row(
          label,
          error,
          ObjectField(
            value: value is Map ? Map<String, Object?>.from(value) : null,
            typeLabel: spec.kind == FieldKind.objectRef
                ? 'GameObject'
                : humanize(spec.behaviourType?.split('.').last ?? 'Script'),
            subject: label,
            describe: model.describeObject,
            candidates: () => [
              for (final o in model.objectsFor(spec))
                PickerItem(
                  value: o.id,
                  title: o.name,
                  icon: FIcons.cropSquare,
                  detail: o.path.isEmpty ? null : o.path,
                  keywords: o.path,
                  facts: [('Name', o.name), if (o.path.isNotEmpty) ('Parent', o.path)],
                ),
            ],
            refFor: (entity) => model.objectRef(spec, entity),
            mixed: mixed,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.layerMask:
        return _row(
          label,
          error,
          LayerMaskField(
            value: value is num ? value.toInt() : null,
            layers: model.project.layerNames(),
            mixed: mixed,
            enabled: _enabled,
            onCommit: _commit,
          ),
        );
      case FieldKind.array:
        return _array(context, label, error, value);
      case FieldKind.object:
        return _object(context, label, value);
      case FieldKind.any:
        return _row(label, error, JsonField(value: value, mixed: mixed, enabled: _enabled, onCommit: _commit));
    }
  }

  Widget _row(String label, String? error, Widget child, {String? unit}) => FieldRow(
    label: label,
    tooltip: spec.tooltip,
    error: error,
    indent: indent,
    unit: unit,
    overridden: _overridden,
    onRevert: _onRevert,
    onApply: _onApply,
    child: child,
  );

  /// Prefab mode: the field's top-level key was written by an override on
  /// a derived instance — bold, with Revert and (when the prefab can take
  /// it) Apply.
  bool get _overridden => slot.isOverridden(path);
  VoidCallback? get _onRevert =>
      _overridden && model.canRevertField(slot, path) ? () => model.revertField(slot, _writePath) : null;
  ApplyTo? get _onApply => _overridden && model.cannotApplyField(slot, path) == null
      ? (depth) => model.applyField(slot, _writePath, depth: depth)
      : null;

  Widget _array(BuildContext context, String label, String? error, Object? value) {
    final theme = Theme.of(context);
    final item = spec.item;
    final list = value is List ? value : const [];
    final mixed = slot.isMixed(path);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        FieldRow(
          label: label,
          tooltip: spec.tooltip,
          error: error,
          indent: indent,
          overridden: _overridden,
          onRevert: _onRevert,
          onApply: _onApply,
          child: Row(
            children: [
              Expanded(
                child: Text(
                  mixed ? '—' : '${list.length} item${list.length == 1 ? '' : 's'}',
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
                ),
              ),
              if (_enabled && !mixed && item != null)
                IconButton(
                  key: ValueKey('array.add.$path'),
                  tooltip: 'Add item',
                  icon: const Icon(FIcons.add, size: 14),
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(minWidth: 22, minHeight: 22),
                  onPressed: () => model.setField(
                    slot,
                    '$_pointer/${list.length}',
                    item.hasDefault ? item.defaultValue : _blank(item),
                  ),
                ),
            ],
          ),
        ),
        if (!mixed && item != null)
          for (var i = 0; i < list.length; i++)
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Expanded(
                  child: _FieldEditor(
                    key: ValueKey('field.${slot.key}.$path.$i'),
                    model: model,
                    slot: slot,
                    spec: FieldSpec(
                      name: '$i',
                      label: '${item.label} $i',
                      kind: item.kind,
                      tooltip: item.tooltip,
                      min: item.min,
                      max: item.max,
                      step: item.step,
                      unit: item.unit,
                      enumValues: item.enumValues,
                      assetKind: item.assetKind,
                      angle: item.angle,
                      slider: item.slider,
                      multiline: item.multiline,
                      nullable: item.nullable,
                      item: item.item,
                      fixedLength: item.fixedLength,
                      children: item.children,
                    ),
                    path: '$_pointer/$i',
                    indent: indent + 10,
                  ),
                ),
                if (_enabled)
                  IconButton(
                    tooltip: 'Remove item',
                    icon: const Icon(FIcons.remove, size: 14),
                    padding: EdgeInsets.zero,
                    constraints: const BoxConstraints(minWidth: 22, minHeight: 26),
                    onPressed: () {
                      final next = [
                        for (var j = 0; j < list.length; j++)
                          if (j != i) list[j],
                      ];
                      _commit(next);
                    },
                  ),
              ],
            ),
      ],
    );
  }

  Widget _object(BuildContext context, String label, Object? value) {
    final theme = Theme.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: EdgeInsets.only(left: indent, top: 4, bottom: 1),
          child: Text(label, style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600)),
        ),
        if (value == null && spec.nullable)
          Padding(
            padding: EdgeInsets.only(left: indent + 10),
            child: Align(
              alignment: Alignment.centerLeft,
              child: TextButton(onPressed: _enabled ? () => _commit(_blank(spec)) : null, child: const Text('Set…')),
            ),
          )
        else
          for (final child in spec.children)
            if (child.visibleWith(value is Map ? Map<String, Object?>.from(value) : const {}, siblings: spec.children))
              _FieldEditor(
                key: ValueKey('field.${slot.key}.$path.${child.name}'),
                model: model,
                slot: slot,
                spec: child,
                path: '$_pointer/${child.name}',
                indent: indent + 10,
              ),
      ],
    );
  }

  /// A fresh value of a spec (an added array item, a nullable object set).
  static Object? _blank(FieldSpec s) => switch (s.kind) {
    FieldKind.number => 0,
    FieldKind.integer || FieldKind.layerMask => 0,
    FieldKind.boolean => false,
    FieldKind.string || FieldKind.entity => '',
    FieldKind.enumeration => s.enumValues.isEmpty ? '' : s.enumValues.first,
    FieldKind.vec2 || FieldKind.vec3 || FieldKind.vec4 => List<Object>.filled(s.components, 0),
    FieldKind.quaternion => [0, 0, 0, 1],
    FieldKind.color => List<Object>.filled(s.components, 1),
    FieldKind.assetRef || FieldKind.objectRef || FieldKind.scriptRef => null,
    FieldKind.array => <Object?>[],
    FieldKind.object => {
      for (final c in s.children)
        if (c.hasDefault) c.name: c.defaultValue,
    },
    FieldKind.any => null,
  };
}

/// The list to write for [values] where the document held [previous]: each
/// slot keeps its int-ness (`[0, 0, 0, 1]` stays ints where it can).
List<Object> _numbersLike(List<double> values, List<num>? previous) => [
  for (var i = 0; i < values.length; i++)
    jsonNumber(values[i], previous: previous != null && i < previous.length ? previous[i] : null),
];

// ── Prefab overrides ───────────────────────────────────────────────────────

/// A derived entity's banner: the instance it belongs to, whether an
/// override added it, and a way back to the anchor.
class _DerivedBanner extends StatelessWidget {
  const _DerivedBanner({required this.model, required this.entity});

  final InspectorModel model;
  final EntityInfo entity;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final anchor = entity.anchor!;
    final x = model.expansionOf(anchor);
    final anchorName = model.tab?.tree[anchor]?.name ?? anchor;
    final prefabName = x?.prefab?.name ?? x?.asset ?? '';
    return Container(
      color: context.tokens.surfaceOf(context.tokens.accent, .18),
      padding: const EdgeInsets.fromLTRB(8, 4, 2, 4),
      child: Row(
        children: [
          Icon(FIcons.widgetsOutlined, size: 13, color: context.tokens.accent),
          const SizedBox(width: 6),
          Expanded(
            child: Text(
              entity.added
                  ? 'Added to the instance $anchorName by an override'
                  : 'Part of the instance $anchorName${prefabName.isEmpty ? '' : ' (prefab $prefabName)'} — '
                        'inherited; edits become overrides',
              key: const ValueKey('prefab.derived'),
              style: theme.textTheme.bodySmall,
              overflow: TextOverflow.ellipsis,
              maxLines: 2,
            ),
          ),
          TextButton(
            key: const ValueKey('prefab.selectInstance'),
            style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
            onPressed: () => model.tab?.select(anchor),
            child: const Text('Select instance', style: TextStyle(fontSize: 11)),
          ),
        ],
      ),
    );
  }
}

/// `setField Mesh (Crate) › Mesh Renderer › cast_shadows = false` — an
/// override as the expansion annotates it (names, not prefab-local ids).
String describeExpansionOverride(ExpansionOverride o) {
  final entity = o.entityName?.isNotEmpty ?? false ? o.entityName! : o.entity;
  final component = o.componentType ?? o.component;
  final target = [entity, component, o.field].where((s) => s != null).join(' › ');
  return switch (o.op) {
    'setField' => '$target = ${prettyJson(o.value)}',
    'unsetField' => '$target unset',
    'addComponent' => 'add ${o.rawOp['type'] ?? component ?? ''} on $entity',
    'removeComponent' => 'remove $target',
    'addObject' => 'add object ${o.rawOp['name'] ?? o.entity}',
    'removeObject' => 'remove object $entity',
    _ => '${o.op} $target',
  };
}

/// `setField robot/mesh.cast_shadows = false`, `addComponent …`.
String describeOverride(Map<String, Object?> o) {
  final op = o['op'] ?? '?';
  final target = [o['entity'], o['component'], o['field']].where((s) => s != null).join(' › ');
  return switch (op) {
    'setField' => '$target = ${prettyJson(o['value'])}',
    'unsetField' => '$target unset',
    'addComponent' =>
      'add ${(o['component'] is Map ? (o['component'] as Map)['type'] : o['component']) ?? ''} on ${o['entity']}',
    'removeComponent' => 'remove $target',
    'addObject' => 'add object ${o['name'] ?? o['id'] ?? ''}',
    'removeObject' => 'remove object ${o['entity'] ?? o['id'] ?? ''}',
    _ => '$op $target',
  };
}

// ── Add Component ──────────────────────────────────────────────────────────

/// Add Component through the shared picker (item 9d), adding to every
/// selected entity: the addable types
/// grouped by category, a type the selection already holds listed dim with
/// where it is (an entity holds one component per type, FYN-D0204). Under
/// [anchor] (the Add button) or at the top of the window (the command).
Future<void> showAddComponent(BuildContext context, InspectorModel model, {Rect? anchor}) async {
  final selected = model.entities.length;
  String? alreadyOn(String type) {
    final on = model.entitiesWith(type);
    if (on.isEmpty) return null;
    if (selected == 1) return 'already on ${on.single.name.isEmpty ? 'this entity' : on.single.name}';
    if (on.length == selected) return 'already on every selected entity';
    return 'already on ${on.length} of $selected';
  }

  final single = model.single;
  final choice = await showPicker<String>(
    context,
    title: 'Add Component',
    subject: selected == 1
        ? (single == null || single.name.isEmpty ? null : single.name)
        : (selected > 1 ? '$selected entities' : null),
    anchor: anchor,
    hint: 'Search components and behaviours',
    keyPrefix: 'addComponent',
    countNoun: 'types',
    items: [
      for (final t in model.addableTypes())
        if (alreadyOn(t.type) case final held)
          PickerItem(
            value: t.type,
            id: t.type,
            title: t.title,
            detail: t.spec.isBehaviour ? 'behaviour' : null,
            group: t.category,
            icon: componentIcon(t.spec),
            enabled: held == null,
            note: held,
            keywords: t.type,
            facts: [('Type', t.type), ('Kind', t.spec.isBehaviour ? 'behaviour' : 'component')],
            description: held == null
                ? t.spec.description
                : '${t.spec.description == null ? '' : '${t.spec.description}\n\n'}'
                      'An entity holds one component per type (FYN-D0204).',
          ),
    ],
  );
  if (choice == null || choice.isNone) return;
  if (!model.addComponent(choice.value!) && model.lastError != null && context.mounted) {
    Notices.maybeOf(context)?.show(model.lastError!);
  }
}

/// A followed field's value as a number (see [InspectorModel.liveField]).
final class _Number implements ValueListenable<num?> {
  const _Number(this.source);

  final ValueListenable<Object?> source;

  @override
  num? get value => switch (source.value) {
    final num n => n,
    _ => null,
  };

  @override
  void addListener(VoidCallback listener) => source.addListener(listener);
  @override
  void removeListener(VoidCallback listener) => source.removeListener(listener);

  @override
  bool operator ==(Object other) => other is _Number && identical(other.source, source);
  @override
  int get hashCode => identityHashCode(source);
}
