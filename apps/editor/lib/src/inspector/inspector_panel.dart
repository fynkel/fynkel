// The Inspector panel (blueprint §19.2e item 4): the selection's header
// (name, id, count), the "run `fynkel codegen`" banner, one collapsible
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
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import 'codegen_check.dart';
import 'field_spec.dart';
import 'field_widgets.dart';
import 'inspector_math.dart';
import 'inspector_model.dart';

class InspectorPanel extends StatefulWidget {
  const InspectorPanel({super.key, required this.model});

  final InspectorModel model;

  @override
  State<InspectorPanel> createState() => _InspectorPanelState();
}

class _InspectorPanelState extends State<InspectorPanel> {
  final _collapsed = <String>{};

  InspectorModel get model => widget.model;

  @override
  void initState() {
    super.initState();
    model.addListener(_onModel);
  }

  @override
  void didUpdateWidget(covariant InspectorPanel old) {
    super.didUpdateWidget(old);
    if (old.model != model) {
      old.model.removeListener(_onModel);
      model.addListener(_onModel);
    }
  }

  @override
  void dispose() {
    model.removeListener(_onModel);
    super.dispose();
  }

  void _onModel() {
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    if (!model.hasDocument) return _empty(context, Icons.tune, 'Open a document and select an entity.');
    final banner = _banner(context);
    if (model.tooMany) {
      return Column(
        children: [
          ?banner,
          Expanded(
            child: _empty(
              context,
              Icons.select_all,
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
            child: _empty(context, Icons.touch_app_outlined, 'Nothing selected.\nClick an entity in the Hierarchy.'),
          ),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ?banner,
        _header(context),
        const Divider(height: 1),
        Expanded(
          child: ListView(
            key: const ValueKey('inspector.list'),
            padding: const EdgeInsets.only(bottom: 12),
            children: [
              if (model.single?.isDerived ?? false) _DerivedBanner(model: model, entity: model.single!),
              for (final slot in model.slots)
                _ComponentCard(
                  key: ValueKey('card.${slot.key}'),
                  model: model,
                  slot: slot,
                  collapsed: _collapsed.contains(slot.key),
                  onToggle: () => setState(
                    () => _collapsed.contains(slot.key) ? _collapsed.remove(slot.key) : _collapsed.add(slot.key),
                  ),
                ),
              if (model.single?.prefab != null) _PrefabSection(model: model, entity: model.single!),
              Padding(
                padding: const EdgeInsets.fromLTRB(12, 10, 12, 0),
                child: OutlinedButton.icon(
                  key: const ValueKey('inspector.addComponent'),
                  onPressed: model.canEdit ? () => showAddComponent(context, model) : null,
                  icon: const Icon(Icons.add, size: 16),
                  label: const Text('Add Component'),
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
                        icon: const Icon(Icons.close, size: 14),
                        padding: EdgeInsets.zero,
                        constraints: const BoxConstraints(minWidth: 24, minHeight: 24),
                        onPressed: model.dismissError,
                      ),
                    ],
                  ),
                ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _empty(BuildContext context, IconData icon, String text) {
    final theme = Theme.of(context);
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 36, color: theme.hintColor),
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
    final single = model.single;
    if (single != null) {
      return Padding(
        padding: const EdgeInsets.fromLTRB(12, 8, 12, 6),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Icon(
                  single.prefab != null ? Icons.widgets_outlined : Icons.category_outlined,
                  size: 16,
                  color: theme.hintColor,
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
              ],
            ),
            const SizedBox(height: 2),
            Row(
              children: [
                Expanded(
                  child: SelectableText(
                    single.id,
                    style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor, fontFamily: 'monospace'),
                  ),
                ),
                IconButton(
                  tooltip: 'Copy id',
                  icon: const Icon(Icons.copy, size: 12),
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints(minWidth: 20, minHeight: 20),
                  onPressed: () => Clipboard.setData(ClipboardData(text: single.id)),
                ),
              ],
            ),
          ],
        ),
      );
    }
    final n = model.entities.length;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 6),
      child: Row(
        children: [
          Icon(Icons.select_all, size: 16, color: theme.hintColor),
          const SizedBox(width: 6),
          Expanded(
            child: Text(
              '$n entities — ${model.entities.map((e) => e.name).take(3).join(', ')}${n > 3 ? ', …' : ''}',
              key: const ValueKey('inspector.multi'),
              style: theme.textTheme.bodySmall,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }

  /// The codegen banner: committed drift (instant) or the CLI check's answer.
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
    final color = isError ? theme.colorScheme.errorContainer : theme.colorScheme.tertiaryContainer;
    final onColor = isError ? theme.colorScheme.onErrorContainer : theme.colorScheme.onTertiaryContainer;
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
                Icon(isError ? Icons.error_outline : Icons.sync_problem, size: 16, color: onColor),
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
                      icon: const Icon(Icons.copy, size: 14),
                      label: Text(check.command, style: const TextStyle(fontFamily: 'monospace', fontSize: 11)),
                    ),
                    TextButton.icon(
                      key: const ValueKey('inspector.checkAgain'),
                      style: TextButton.styleFrom(foregroundColor: onColor, visualDensity: VisualDensity.compact),
                      onPressed: status == CodegenStatus.running ? null : check.check,
                      icon: status == CodegenStatus.running
                          ? const SizedBox(width: 12, height: 12, child: CircularProgressIndicator(strokeWidth: 1.5))
                          : const Icon(Icons.refresh, size: 14),
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

class _ComponentCard extends StatelessWidget {
  const _ComponentCard({
    super.key,
    required this.model,
    required this.slot,
    required this.collapsed,
    required this.onToggle,
  });

  final InspectorModel model;
  final ComponentSlot slot;
  final bool collapsed;
  final VoidCallback onToggle;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final spec = slot.spec;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        InkWell(
          onTap: onToggle,
          child: Container(
            height: 28,
            color: theme.colorScheme.surfaceContainerHighest.withValues(alpha: 0.5),
            padding: const EdgeInsets.only(left: 4, right: 2),
            child: Row(
              children: [
                Icon(collapsed ? Icons.chevron_right : Icons.expand_more, size: 16),
                Icon(componentIcon(spec), size: 14, color: theme.hintColor),
                const SizedBox(width: 6),
                Expanded(
                  child: Tooltip(
                    message: spec.description ?? spec.type,
                    waitDuration: const Duration(milliseconds: 600),
                    child: Text.rich(
                      TextSpan(
                        text: spec.title,
                        style: theme.textTheme.bodySmall?.copyWith(fontWeight: FontWeight.w600),
                        children: [
                          if (spec.isBehaviour)
                            TextSpan(
                              text: '  ${spec.namespace}',
                              style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor),
                            ),
                          if (slot.index > 0)
                            TextSpan(
                              text: '  #${slot.index + 1}',
                              style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor),
                            ),
                        ],
                      ),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ),
                if (spec.missingSchema)
                  Tooltip(
                    message: 'No schema for ${spec.type}: fields are shown as JSON. Run `fynkel codegen`.',
                    child: Icon(Icons.help_outline, size: 14, color: theme.colorScheme.error),
                  ),
                PopupMenuButton<String>(
                  key: ValueKey('card.menu.${slot.key}'),
                  tooltip: '',
                  padding: EdgeInsets.zero,
                  iconSize: 16,
                  onSelected: (v) => _menu(context, v),
                  itemBuilder: (_) => [
                    PopupMenuItem(
                      value: 'reset',
                      enabled: model.canEdit,
                      height: 28,
                      child: const Text('Reset to Defaults'),
                    ),
                    if (spec.isBehaviour) const PopupMenuItem(value: 'open', height: 28, child: Text('Open Script')),
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
        if (!collapsed) Padding(padding: const EdgeInsets.fromLTRB(8, 4, 8, 6), child: _fields(context)),
      ],
    );
  }

  Future<void> _menu(BuildContext context, String v) async {
    final messenger = ScaffoldMessenger.maybeOf(context);
    switch (v) {
      case 'remove':
        model.removeComponent(slot);
      case 'reset':
        model.resetToDefaults(slot);
      case 'copyType':
        await Clipboard.setData(ClipboardData(text: slot.type));
      case 'open':
        final err = await model.openScript(slot);
        if (err != null) messenger?.showSnackBar(SnackBar(content: Text(err)));
    }
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
          rows.add(
            Padding(
              padding: const EdgeInsets.only(top: 6, bottom: 2),
              child: Text(
                group,
                style: theme.textTheme.labelSmall?.copyWith(
                  color: theme.hintColor,
                  fontWeight: FontWeight.w600,
                  letterSpacing: 0.4,
                ),
              ),
            ),
          );
        }
      }
      if (f.header != null) {
        rows.add(
          Padding(
            padding: const EdgeInsets.only(top: 6, bottom: 2),
            child: Text(
              f.header!,
              style: theme.textTheme.labelSmall?.copyWith(
                color: theme.hintColor,
                fontWeight: FontWeight.w600,
                letterSpacing: 0.4,
              ),
            ),
          ),
        );
      }
      rows.add(
        _FieldEditor(key: ValueKey('field.${slot.key}.${f.name}'), model: model, slot: slot, spec: f, path: f.name),
      );
    }
    // Keys the schema does not list (an open schema, or none): JSON rows.
    if (spec.additionalProperties || spec.missingSchema) {
      for (final key in data.keys) {
        if (spec.field(key) != null) continue;
        any = true;
        rows.add(
          FieldRow(
            key: ValueKey('field.${slot.key}.$key'),
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
          ),
        );
      }
    }
    if (!any) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: Text('No fields.', style: theme.textTheme.bodySmall?.copyWith(color: theme.hintColor)),
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
    'fynkel.Transform' => Icons.open_with,
    'fynkel.Camera' || 'fynkel.ThirdPersonCamera' || 'fynkel.LookAtCamera' => Icons.videocam_outlined,
    'fynkel.DirectionalLight' || 'fynkel.PointLight' || 'fynkel.SpotLight' => Icons.lightbulb_outline,
    'fynkel.Environment' => Icons.wb_sunny_outlined,
    'fynkel.MeshRenderer' => Icons.view_in_ar_outlined,
    'fynkel.Skin' || 'fynkel.Animator' => Icons.accessibility_new,
    'fynkel.RigidBody' ||
    'fynkel.Collider' ||
    'fynkel.CharacterController' ||
    'fynkel.TriggerZone' => Icons.sports_basketball_outlined,
    'fynkel.Emitter' || 'fynkel.LifetimeDespawn' => Icons.auto_awesome_outlined,
    'fynkel.Rotator' || 'fynkel.Oscillate' || 'fynkel.Follow' => Icons.motion_photos_on_outlined,
    _ => spec.isBehaviour ? Icons.code : Icons.extension_outlined,
  };
}

/// The `icon` hint names a Material icon; the ones a behaviour is likely to
/// use. An unknown name falls back to the type's default.
const _materialIcons = <String, IconData>{
  'rotate_right': Icons.rotate_right,
  'rotate_left': Icons.rotate_left,
  'directions_run': Icons.directions_run,
  'person': Icons.person,
  'bolt': Icons.bolt,
  'star': Icons.star,
  'favorite': Icons.favorite,
  'flag': Icons.flag,
  'timer': Icons.timer,
  'toggle_on': Icons.toggle_on,
  'door_front': Icons.door_front_door_outlined,
  'lock': Icons.lock_outline,
  'key': Icons.key,
  'sports_esports': Icons.sports_esports,
  'gamepad': Icons.gamepad,
  'code': Icons.code,
  'extension': Icons.extension,
  'motion': Icons.motion_photos_on_outlined,
  'sensors': Icons.sensors,
  'music_note': Icons.music_note,
  'volume_up': Icons.volume_up,
  'visibility': Icons.visibility,
  'speed': Icons.speed,
  'settings': Icons.settings,
  'build': Icons.build,
  'science': Icons.science,
  'psychology': Icons.psychology,
  'smart_toy': Icons.smart_toy,
  'pets': Icons.pets,
  'eco': Icons.eco,
  'water_drop': Icons.water_drop,
  'local_fire_department': Icons.local_fire_department,
  'ac_unit': Icons.ac_unit,
  'brush': Icons.brush,
  'text_fields': Icons.text_fields,
  'chat': Icons.chat,
  'shopping_cart': Icons.shopping_cart,
  'inventory': Icons.inventory_2_outlined,
  'health_and_safety': Icons.health_and_safety,
  'shield': Icons.shield,
  'gps_fixed': Icons.gps_fixed,
  'explore': Icons.explore,
  'map': Icons.map,
  'camera': Icons.videocam_outlined,
  'light': Icons.lightbulb_outline,
  'move': Icons.open_with,
  'physics': Icons.sports_basketball_outlined,
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
        return NumberField(
          value: value is num ? value : null,
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
            describe: model.describeAsset,
            candidates: () => model.assetsFor(spec.assetKind),
            onCommit: _commit,
          ),
        );
      case FieldKind.entity:
        return _row(
          label,
          error,
          EntityField(
            value: value?.toString(),
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
            layers: model.project.physicsLayers(),
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
                  icon: const Icon(Icons.add, size: 14),
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
                    icon: const Icon(Icons.remove, size: 14),
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
          height: 28,
          color: theme.colorScheme.surfaceContainerHighest.withValues(alpha: 0.5),
          padding: const EdgeInsets.only(left: 8, right: 2),
          child: Row(
            children: [
              Icon(
                missing || broken ? Icons.broken_image_outlined : Icons.widgets_outlined,
                size: 14,
                color: missing || broken ? theme.colorScheme.error : theme.hintColor,
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
                            icon: const Icon(Icons.undo, size: 14),
                            padding: EdgeInsets.zero,
                            constraints: const BoxConstraints(minWidth: 22, minHeight: 22),
                            onPressed: model.canEdit ? () => model.revertOverride(entity.id, index: i) : null,
                          ),
                          IconButton(
                            key: ValueKey('prefab.apply.$i'),
                            tooltip: applyWhy ?? 'Apply this override to the prefab',
                            icon: const Icon(Icons.upload, size: 14),
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
                    icon: const Icon(Icons.upload, size: 14),
                    label: const Text('Apply to prefab', style: TextStyle(fontSize: 11)),
                  ),
                ),
              if (count > 0)
                TextButton.icon(
                  key: const ValueKey('prefab.revertAll'),
                  style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
                  onPressed: model.canEdit ? () => model.revertOverride(entity.id) : null,
                  icon: const Icon(Icons.undo, size: 14),
                  label: const Text('Revert all', style: TextStyle(fontSize: 11)),
                ),
              Tooltip(
                message: unpackWhy ?? 'Unpack — the instance becomes plain entities of this document',
                child: TextButton.icon(
                  key: const ValueKey('prefab.unpack'),
                  style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
                  onPressed: unpackWhy == null ? () => model.unpack(entity.id) : null,
                  icon: const Icon(Icons.unarchive_outlined, size: 14),
                  label: const Text('Unpack', style: TextStyle(fontSize: 11)),
                ),
              ),
              TextButton.icon(
                key: const ValueKey('prefab.open'),
                style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
                onPressed: x == null || missing ? null : () => model.openPrefab(entity.id),
                icon: const Icon(Icons.open_in_new, size: 14),
                label: const Text('Open prefab', style: TextStyle(fontSize: 11)),
              ),
              TextButton.icon(
                key: const ValueKey('prefab.selectAsset'),
                style: TextButton.styleFrom(visualDensity: VisualDensity.compact),
                onPressed: model.revealAsset == null ? null : () => model.selectPrefabAsset(entity.id),
                icon: const Icon(Icons.folder_open_outlined, size: 14),
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
      color: theme.colorScheme.surfaceContainerHighest.withValues(alpha: 0.5),
      padding: const EdgeInsets.fromLTRB(8, 4, 2, 4),
      child: Row(
        children: [
          Icon(Icons.widgets_outlined, size: 14, color: theme.hintColor),
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

/// The searchable Add Component popup; adds to every selected entity.
Future<void> showAddComponent(BuildContext context, InspectorModel model) async {
  final type = await showDialog<String>(
    context: context,
    builder: (_) => _AddComponentDialog(model: model),
  );
  if (type == null) return;
  if (!model.addComponent(type) && model.lastError != null && context.mounted) {
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(SnackBar(content: Text(model.lastError!)));
  }
}

class _AddComponentDialog extends StatefulWidget {
  const _AddComponentDialog({required this.model});

  final InspectorModel model;

  @override
  State<_AddComponentDialog> createState() => _AddComponentDialogState();
}

class _AddComponentDialogState extends State<_AddComponentDialog> {
  final _query = TextEditingController();

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final types = widget.model.addableTypes(_query.text);
    final selected = widget.model.entities.length;
    String? alreadyOn(String type) {
      final on = widget.model.entitiesWith(type);
      if (on.isEmpty) return null;
      if (selected == 1) return 'already on ${on.single.name.isEmpty ? 'this entity' : on.single.name}';
      if (on.length == selected) return 'already on every selected entity';
      return 'already on ${on.length} of $selected';
    }

    final addable = [
      for (final t in types)
        if (alreadyOn(t.type) == null) t,
    ];
    return AlertDialog(
      title: const Text('Add Component'),
      content: SizedBox(
        width: 420,
        height: 380,
        child: Column(
          children: [
            TextField(
              key: const ValueKey('addComponent.search'),
              controller: _query,
              autofocus: true,
              decoration: const InputDecoration(
                isDense: true,
                prefixIcon: Icon(Icons.search, size: 16),
                hintText: 'Search components and behaviours',
              ),
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) {
                if (addable.length == 1) Navigator.of(context).pop(addable.single.type);
              },
            ),
            const SizedBox(height: 6),
            Expanded(
              child: types.isEmpty
                  ? Center(child: Text('Nothing matches.', style: theme.textTheme.bodySmall))
                  : ListView.builder(
                      itemCount: types.length,
                      itemBuilder: (context, i) {
                        final t = types[i];
                        final held = alreadyOn(t.type);
                        final newCategory = i == 0 || types[i - 1].category != t.category;
                        return Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            if (newCategory)
                              Padding(
                                padding: const EdgeInsets.fromLTRB(12, 8, 12, 2),
                                child: Text(
                                  t.category,
                                  style: theme.textTheme.labelSmall?.copyWith(
                                    color: theme.hintColor,
                                    fontWeight: FontWeight.w600,
                                  ),
                                ),
                              ),
                            ListTile(
                              key: ValueKey('addComponent.${t.type}'),
                              dense: true,
                              enabled: held == null,
                              leading: Icon(componentIcon(t.spec), size: 16),
                              title: Text(t.title, style: theme.textTheme.bodySmall),
                              subtitle: Text(
                                t.spec.description ?? t.type,
                                style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                              trailing: held == null
                                  ? null
                                  : Tooltip(
                                      message: 'An entity holds one component per type (FYN-D0204).',
                                      child: Text(
                                        held,
                                        key: ValueKey('addComponent.held.${t.type}'),
                                        style: theme.textTheme.labelSmall?.copyWith(color: theme.hintColor),
                                      ),
                                    ),
                              onTap: held == null ? () => Navigator.of(context).pop(t.type) : null,
                            ),
                          ],
                        );
                      },
                    ),
            ),
          ],
        ),
      ),
      actions: [TextButton(onPressed: () => Navigator.of(context).pop(), child: const Text('Cancel'))],
    );
  }
}
