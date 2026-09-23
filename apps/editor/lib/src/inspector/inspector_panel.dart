// The Inspector panel (blueprint §19.2e item 4): the selection's header
// (name, id, count; the layer and tags, ADR-0020), the "run `fynkel codegen`" banner, one collapsible
// card per component slot with its fields grouped by the `group`/`header`
// hints and filtered by `showIf`, the card menu (Remove, Reset to defaults,
// Open script, Copy type), the prefab instance's override list with Revert,
// and Add Component as a searchable popup. Everything it writes goes
// through `InspectorModel` — one transaction per gesture.
//
// Prefab mode (item 7): an instance's anchor shows its prefab (name, kind,
// a missing or broken one with its diagnostics), the annotated override
// list with Revert / Apply to prefab per row and for all, Unpack, Open
// prefab and Select prefab asset; a derived entity's fields show the
// overridden ones bold with Revert and Apply arrows, and a banner names
// the instance it belongs to.
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
import 'field_widgets.dart';
import 'inspector_chrome.dart';
import 'inspector_math.dart';
import '../theme/theme.dart';
import '../theme/tokens.dart';
import 'inspector_model.dart';
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
  });

  final InspectorModel model;

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
          child: CustomScrollView(
            key: const ValueKey('inspector.list'),
            slivers: [
              if (model.single?.isDerived ?? false)
                SliverToBoxAdapter(
                  child: _DerivedBanner(model: model, entity: model.single!),
                ),
              for (final slot in model.slots) _card(slot),
              _kept('footer', () => SliverPadding(
                padding: const EdgeInsets.only(bottom: 12),
                sliver: SliverList.list(
                  children: [
                    if (model.single?.prefab != null) _PrefabSection(model: model, entity: model.single!),
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
      ],
    );
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
            if (check != null)
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
    return SliverMainAxisGroup(
      slivers: [
        PinnedHeaderSliver(
          child: Material(
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
                    Icon(collapsed ? FIcons.chevronRight : FIcons.expandMore, size: 14, color: t.textSecondary),
                    const SizedBox(width: 2),
                    Icon(componentIcon(spec), size: 14, color: t.textSecondary),
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
                      onSelected: (v) => _menu(context, v),
                      itemBuilder: (_) => [
                        PopupMenuItem(
                          value: 'reset',
                          enabled: model.canEdit,
                          height: 28,
                          child: const Text('Reset to Defaults'),
                        ),
                        if (spec.isBehaviour)
                          const PopupMenuItem(value: 'open', height: 28, child: Text('Open Script')),
                        const PopupMenuItem(value: 'copyType', height: 28, child: Text('Copy Type Name')),
                        const PopupMenuDivider(),
                        PopupMenuItem(
                          value: 'remove',
                          enabled: model.canEdit,
                          height: 28,
                          child: const Text('Remove Component'),
                        ),
                      ],
                    ),
                  ],
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

  Future<void> _menu(BuildContext context, String v) async {
    final notices = Notices.maybeOf(context);
    switch (v) {
      case 'remove':
        model.removeComponent(slot);
      case 'reset':
        model.resetToDefaults(slot);
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
    final value = slot.value(name);
    final mixed = slot.isMixed(name);
    final old = kept?[k];
    if (old != null && old.generation == model.generation && old.mixed == mixed && deepEquals(old.value, value)) {
      return old.widget;
    }
    final w = RepaintBoundary(key: ValueKey('field.${slot.key}.$name'), child: build());
    kept?[k] = (generation: model.generation, value: value, mixed: mixed, widget: w);
    return w;
  }

  Widget _fields(BuildContext context) {
    final theme = Theme.of(context);
    final spec = slot.spec;
    final data = slot.first.data;
    final rows = <Widget>[];
    String? group;
    var any = false;
    for (final f in spec.fields) {
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
                ? () => model.applyField(slot, key)
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

  @override
  Widget build(BuildContext context) {
    final value = slot.value(path);
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
            live: model.liveField(slot, path),
            mixed: mixed,
            enabled: _enabled,
            onCommit: (quat) => _commit(_numbersLike(quat, q)),
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
  VoidCallback? get _onApply =>
      _overridden && model.cannotApplyField(slot, path) == null ? () => model.applyField(slot, _writePath) : null;

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
    FieldKind.assetRef => null,
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

/// The anchor's Prefab mode section: the prefab it comes from, the
/// override list (Revert / Apply per row, for all), Unpack, Open prefab,
/// Select prefab asset; a missing or broken prefab shows its diagnostics
/// and keeps the instance's data.
class _PrefabSection extends StatelessWidget {
  const _PrefabSection({required this.model, required this.entity});

  final InspectorModel model;
  final EntityInfo entity;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final p = entity.prefab!;
    final x = entity.expansion;
    final prefabInfo = x?.prefab;
    final assetText = model.describeAsset({'asset': p.assetId, if (p.sub != null) 'sub': p.sub});
    final title = prefabInfo == null
        ? 'Prefab instance — $assetText'
        : 'Prefab instance — ${prefabInfo.name.isEmpty ? assetText : prefabInfo.name}'
              '${prefabInfo.isImported ? ' (imported hierarchy, read-only)' : ''}';
    final broken = x != null && !x.expanded;
    final missing = x == null || x.prefab == null;
    final applyWhy = model.cannotApply(entity.id);
    final unpackWhy = model.cannotUnpack(entity.id);
    final overrides = x?.overrides;
    final count = overrides?.length ?? p.overrides.length;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: context.tokens.row + 4,
          decoration: BoxDecoration(
            color: context.tokens.header,
            border: Border(top: context.tokens.hairline, bottom: context.tokens.hairline),
          ),
          padding: const EdgeInsets.only(left: 8, right: 2),
          child: Row(
            children: [
              Icon(
                missing || broken ? FIcons.brokenImageOutlined : FIcons.widgetsOutlined,
                size: 13,
                color: missing || broken ? context.tokens.error : context.tokens.accent,
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  title,
                  key: const ValueKey('prefab.title'),
                  style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
        ),
        if (x != null && x.diagnostics.isNotEmpty)
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 6, 8, 0),
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
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 4, 8, 6),
          child: count == 0
              ? Text(
                  'No overrides — every field is the prefab\'s.',
                  style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor),
                )
              : Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    for (var i = 0; i < count; i++)
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              overrides != null
                                  ? describeExpansionOverride(overrides[i])
                                  : describeOverride(p.overrides[i]),
                              key: ValueKey('prefab.override.$i'),
                              style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.bold),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                          IconButton(
                            key: ValueKey('prefab.revert.$i'),
                            tooltip: 'Revert this override',
                            icon: const Icon(FIcons.undo, size: 14),
                            padding: EdgeInsets.zero,
                            constraints: const BoxConstraints(minWidth: 22, minHeight: 22),
                            onPressed: model.canEdit ? () => model.revertOverride(entity.id, index: i) : null,
                          ),
                          IconButton(
                            key: ValueKey('prefab.apply.$i'),
                            tooltip: applyWhy ?? 'Apply this override to the prefab',
                            icon: const Icon(FIcons.upload, size: 14),
                            padding: EdgeInsets.zero,
                            constraints: const BoxConstraints(minWidth: 22, minHeight: 22),
                            onPressed: applyWhy == null ? () => model.applyOverrides(entity.id, index: i) : null,
                          ),
                        ],
                      ),
                  ],
                ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(10, 0, 8, 6),
          child: Wrap(
            spacing: 4,
            runSpacing: 2,
            children: [
              if (count > 0)
                Tooltip(
                  message: applyWhy ?? 'Apply every override to the prefab, then revert them here',
                  child: TextButton.icon(
                    key: const ValueKey('prefab.applyAll'),
                    style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
                    onPressed: applyWhy == null ? () => model.applyOverrides(entity.id) : null,
                    icon: const Icon(FIcons.upload, size: 14),
                    label: const Text('Apply to prefab', style: TextStyle(fontSize: 11)),
                  ),
                ),
              if (count > 0)
                TextButton.icon(
                  key: const ValueKey('prefab.revertAll'),
                  style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
                  onPressed: model.canEdit ? () => model.revertOverride(entity.id) : null,
                  icon: const Icon(FIcons.undo, size: 14),
                  label: const Text('Revert all', style: TextStyle(fontSize: 11)),
                ),
              Tooltip(
                message: unpackWhy ?? 'Unpack — the instance becomes plain entities of this document',
                child: TextButton.icon(
                  key: const ValueKey('prefab.unpack'),
                  style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
                  onPressed: unpackWhy == null ? () => model.unpack(entity.id) : null,
                  icon: const Icon(FIcons.unarchiveOutlined, size: 14),
                  label: const Text('Unpack', style: TextStyle(fontSize: 11)),
                ),
              ),
              TextButton.icon(
                key: const ValueKey('prefab.open'),
                style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
                onPressed: x == null || missing ? null : () => model.openPrefab(entity.id),
                icon: const Icon(FIcons.openInNew, size: 14),
                label: const Text('Open prefab', style: TextStyle(fontSize: 11)),
              ),
              TextButton.icon(
                key: const ValueKey('prefab.selectAsset'),
                style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
                onPressed: model.revealAsset == null ? null : () => model.selectPrefabAsset(entity.id),
                icon: const Icon(FIcons.folderOpenOutlined, size: 14),
                label: const Text('Select prefab asset', style: TextStyle(fontSize: 11)),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

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
