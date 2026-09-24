// Prefab mode (blueprint §19.2e item 7): the actions over a prefab
// instance that the Inspector, the Hierarchy's context menu and the
// `prefab.*` commands share — the edits of a derived entity as override
// ops on its anchor, Revert override, Apply to prefab (a transaction on
// the prefab document, then `revertPrefabOverride` on the instance: two
// undo steps, one per document), Unpack (one `unpackPrefab` transaction),
// and what each of them needs before it can run.
import 'dart:convert';
import 'dart:math';

import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../documents/document_tabs.dart';
import '../documents/open_document.dart';

/// A random UUID (version 4) for the component or object an override adds
/// — the prefab-local id the op names, chosen here like the crate's own
/// `createObject` would choose one.
String newUuid([Random? random]) {
  final r = random ?? Random.secure();
  final b = List<int>.generate(16, (_) => r.nextInt(256));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  String hex(int from, int to) => [for (var i = from; i < to; i++) b[i].toRadixString(16).padLeft(2, '0')].join();
  return '${hex(0, 4)}-${hex(4, 6)}-${hex(6, 8)}-${hex(8, 10)}-${hex(10, 16)}';
}

/// The anchor a selected entity is about: itself when it is a prefab
/// instance, its instance's anchor when it is a derived entity; null for a
/// plain entity or an unknown id.
String? anchorOf(OpenDocument tab, String id) {
  final n = tab.tree[id];
  if (n == null) return null;
  if (n.isPrefabInstance) return id;
  return n.anchor;
}

/// The one anchor the tab's selection is about (every selected entity the
/// same instance's anchor or a derived entity of it), or null.
String? selectionAnchor(OpenDocument? tab) {
  if (tab == null || tab.selection.isEmpty) return null;
  String? anchor;
  for (final id in tab.selection) {
    final a = anchorOf(tab, id);
    if (a == null || (anchor != null && a != anchor)) return null;
    anchor = a;
  }
  return anchor;
}

/// Why the overrides of [x] cannot be applied to its prefab, or null when
/// they can: the scene is read-only, there is nothing to apply, the prefab
/// is missing or broken, or it is an imported hierarchy (a model source's
/// or a rig document's node tree — derived from the file, never edited).
String? cannotApply(OpenDocument tab, PrefabExpansion x) {
  final readOnly = tab.document.readOnly;
  if (readOnly != null) return 'Read-only: $readOnly';
  if (x.overrides.isEmpty) return 'No overrides to apply.';
  final p = x.prefab;
  if (p == null) {
    final why = x.diagnostics.map((d) => '${d.code}: ${d.message}').join('; ');
    return 'The prefab is missing — ${why.isEmpty ? 'it cannot be opened' : why}.';
  }
  if (p.readOnly != null) return 'The prefab cannot be edited: ${p.readOnly}';
  if (!x.expanded) {
    final why = x.diagnostics.map((d) => '${d.code}: ${d.message}').join('; ');
    return 'The instance does not expand ($why); fix or revert the broken override first.';
  }
  return null;
}

/// Why the instance of [x] cannot be unpacked, or null.
String? cannotUnpack(OpenDocument tab, PrefabExpansion x) {
  final readOnly = tab.document.readOnly;
  if (readOnly != null) return 'Read-only: $readOnly';
  if (!x.expanded) {
    final why = x.diagnostics.map((d) => '${d.code}: ${d.message}').join('; ');
    return 'The instance does not expand ($why); nothing to unpack.';
  }
  return null;
}

/// A prefab an instance's overrides can be applied to: the one it comes
/// from ([depth] 0) and, when that is a variant, its base and the base's
/// base (Unity's "Apply to Prefab Variant 'X'" / "Apply to Prefab 'Base'").
typedef ApplyTarget = ({int depth, String name, bool variant});

/// [x]'s apply targets, nearest first.
List<ApplyTarget> applyTargets(DocumentTabs tabs, PrefabExpansion x) {
  final p = x.prefab;
  if (p == null) return const [];
  final out = <ApplyTarget>[(depth: 0, name: p.name, variant: p.isVariant)];
  final seen = {p.id};
  var next = p.variantOf?['asset'] as String?;
  while (next != null && seen.add(next)) {
    final d = tabs.project.assetDetail(next);
    if (d == null) break;
    out.add((depth: out.length, name: _stem(d.name), variant: d.isVariant));
    next = d.variantOf?['asset'] as String?;
  }
  return out;
}

/// `Crate.prefab.json` → `Crate`.
String _stem(String name) {
  final dot = name.indexOf('.');
  return dot <= 0 ? name : name.substring(0, dot);
}

/// "Prefab Variant 'Big Crate'" or "Prefab 'Crate'".
String applyTargetLabel(ApplyTarget t) => "${t.variant ? 'Prefab Variant' : 'Prefab'} '${t.name}'";

/// What Apply to prefab did.
final class ApplyOutcome {
  const ApplyOutcome({required this.ok, this.error, this.prefabTab, this.applied = 0});

  final bool ok;
  final String? error;

  /// The prefab's tab (opened behind the scene's when it was not open).
  final OpenDocument? prefabTab;

  /// Overrides made permanent.
  final int applied;
}

/// Apply to prefab: the overrides at [indices] (every one when null) of
/// the instance anchored at [anchor] become one transaction on the prefab
/// document (`applyPrefabOverride` each: a plain edit, or an override of
/// what the prefab takes from its base or a nested instance) — opened into
/// a tab behind the focused one, so its unsaved edit is visible and
/// undoable there — then are reverted on the instance in one
/// `revertPrefabOverride` transaction. Two undo steps, one per document;
/// the scene is untouched when the prefab refuses. [depth] 1 applies to
/// the variant's base instead (see [applyTargets]): the overrides this
/// made on the variant's root are applied on down, so the variant keeps
/// none of them.
ApplyOutcome applyOverrides(DocumentTabs tabs, OpenDocument tab, String anchor, {List<int>? indices, int depth = 0}) {
  final x = tab.expansionOf(anchor);
  if (x == null) return const ApplyOutcome(ok: false, error: 'Not a prefab instance.');
  final why = cannotApply(tab, x);
  if (why != null) return ApplyOutcome(ok: false, error: why);
  final chosen = indices == null
      ? x.overrides
      : [
          for (final o in x.overrides)
            if (indices.contains(o.index)) o,
        ];
  if (chosen.isEmpty) return const ApplyOutcome(ok: false, error: 'No overrides to apply.');
  final commands = [for (final o in chosen) Command.applyPrefabOverride(o.rawOp)];
  final prefabTab = tabs.openDocument(x.open, focus: false);
  if (prefabTab == null) return ApplyOutcome(ok: false, error: tabs.openError ?? 'The prefab could not be opened.');
  // Toward the base: what the variant's root instance holds before.
  final root = depth > 0 ? prefabTab.tree.roots.firstOrNull : null;
  final before = root == null ? null : {...?prefabTab.expansionOf(root)?.overrides.map((o) => jsonEncode(o.rawOp))};
  final applied = prefabTab.transaction(commands);
  if (applied == null || !applied.ok) {
    return ApplyOutcome(
      ok: false,
      error: 'The prefab refused the change: ${prefabTab.lastError ?? 'rejected'}',
      prefabTab: prefabTab,
    );
  }
  final all = indices == null || chosen.length == x.overrides.length;
  // Several indices in one transaction: highest first, so a removal never
  // shifts the next one.
  final sorted = chosen.map((o) => o.index).toList()..sort((a, b) => b.compareTo(a));
  final reverted = tab.transaction(
    all
        ? [Command.revertPrefabOverride(anchor)]
        : [for (final i in sorted) Command.revertPrefabOverride(anchor, index: i)],
  );
  if (reverted == null || !reverted.ok) {
    return ApplyOutcome(
      ok: false,
      error:
          'The prefab took the change (undo it there if you do not want it) but the instance kept its overrides: '
          '${tab.lastError ?? 'rejected'}',
      prefabTab: prefabTab,
      applied: chosen.length,
    );
  }
  if (root != null && before != null) {
    final now = prefabTab.expansionOf(root);
    final made = [
      for (final o in now?.overrides ?? const <ExpansionOverride>[])
        if (!before.contains(jsonEncode(o.rawOp))) o.index,
    ];
    if (made.isNotEmpty) {
      final down = applyOverrides(tabs, prefabTab, root, indices: made, depth: depth - 1);
      if (!down.ok) {
        return ApplyOutcome(
          ok: false,
          error: 'Applied to ${prefabTab.title}, but its base refused it: ${down.error}',
          prefabTab: prefabTab,
          applied: chosen.length,
        );
      }
    }
  }
  return ApplyOutcome(ok: true, prefabTab: prefabTab, applied: chosen.length);
}

/// Revert: drops the override at [index] (every one when null). One transaction.
bool revertOverrides(OpenDocument tab, String anchor, {int? index}) =>
    tab.transaction([Command.revertPrefabOverride(anchor, index: index)])?.ok ?? false;

/// Unpack: the instance becomes plain entities in place, one transaction;
/// the anchor stays selected. Unity's: one level — a nested instance stays
/// an instance, an instance of a variant becomes one of its base;
/// [completely] leaves no instance at all.
bool unpackInstance(OpenDocument tab, String anchor, {bool completely = false}) {
  final x = tab.expansionOf(anchor);
  if (x != null) {
    final why = cannotUnpack(tab, x);
    if (why != null) {
      tab.lastError = why;
      tab.keepEditing();
      return false;
    }
  }
  final a = tab.transaction([Command.unpackPrefab(anchor, completely: completely)]);
  if (a == null || !a.ok) return false;
  tab.select(anchor);
  return true;
}

// ── A derived entity's edits, as override ops on its anchor ────────────────
//
// Each takes prefab-local ids (`ExpandedEntity.local`, `ExpandedComponent.local`)
// — what an override names — and the component's applied `data`.

/// The `setField` override for [path] on a derived component: an
/// override's `field` is a whole key of `data` (never a pointer into it),
/// so a nested path (`/position/1`) is folded into its top-level key over
/// the applied value — the whole vector is written with one lane changed.
Map<String, Object?> setFieldOverride({
  required String entity,
  required String component,
  required Map<String, Object?> data,
  required String path,
  required Object? value,
}) {
  final parts = path.startsWith('/') ? path.substring(1).split('/') : path.split('/');
  final key = parts.first;
  Object? whole;
  if (parts.length == 1) {
    whole = _copyJson(value);
  } else {
    whole = _setIn(_copyJson(data[key]), parts.sublist(1), _copyJson(value));
  }
  return {'op': 'setField', 'entity': entity, 'component': component, 'field': key, 'value': whole};
}

Map<String, Object?> unsetFieldOverride({required String entity, required String component, required String path}) {
  final parts = path.startsWith('/') ? path.substring(1).split('/') : path.split('/');
  return {'op': 'unsetField', 'entity': entity, 'component': component, 'field': parts.first};
}

/// The `addComponent` override that puts [type] on the derived entity
/// under a fresh prefab-local id.
Map<String, Object?> addComponentOverride({
  required String entity,
  required String type,
  required int version,
  required Map<String, Object?> data,
  String? id,
}) => {
  'op': 'addComponent',
  'entity': entity,
  'component': id ?? newUuid(),
  'type': type,
  'version': version,
  'data': _copyJson(data),
};

Map<String, Object?> removeComponentOverride({required String entity, required String component}) => {
  'op': 'removeComponent',
  'entity': entity,
  'component': component,
};

/// The `addObject` override for a new child named [name] under the derived
/// entity [parent] (null: an instance root) with [components] (`{type,
/// version, data}` specs) keyed by fresh prefab-local ids.
Map<String, Object?> addObjectOverride({
  required String name,
  required String? parent,
  required List<Map<String, Object?>> components,
  String? id,
}) => {
  'op': 'addObject',
  'entity': id ?? newUuid(),
  'name': name,
  'parent': ?parent,
  'components': {
    for (final c in components)
      (c['id'] as String? ?? newUuid()): {
        'type': c['type'],
        'version': c['version'] ?? 1,
        'data': _copyJson(c['data'] ?? <String, Object?>{}),
      },
  },
};

/// The `addObject` override that re-issues an added entity as it is now
/// — its applied components, minus [without] — under [name] (its current
/// one when null): `setPrefabOverride` replaces the op with the same
/// target, so this is how an added object is renamed or loses a component.
Map<String, Object?> reissuedAddObjectOverride(PrefabExpansion x, ExpandedEntity e, {String? name, String? without}) {
  final original = x.overrides.where((o) => o.op == 'addObject' && o.entity == e.local).firstOrNull;
  return {
    'op': 'addObject',
    'entity': e.local,
    'name': name ?? e.name,
    if (original?.rawOp['parent'] != null) 'parent': original!.rawOp['parent'],
    'components': {
      for (final c in e.components.values)
        if (c.local != null && c.local != without)
          c.local!: {'type': c.type, 'version': c.version, 'data': _copyJson(c.data)},
    },
  };
}

Map<String, Object?> removeObjectOverride(ExpandedEntity e) => {'op': 'removeObject', 'entity': e.local};

/// The index of the override that added [e] (an `addObject`), or null.
int? addObjectIndexOf(PrefabExpansion x, ExpandedEntity e) =>
    x.overrides.where((o) => o.op == 'addObject' && o.entity == e.local).firstOrNull?.index;

/// The index of the `addComponent` override that put [component] on [entity], or null.
int? addComponentIndexOf(PrefabExpansion x, {required String entity, required String component}) => x.overrides
    .where((o) => o.op == 'addComponent' && o.entity == entity && o.component == component)
    .firstOrNull
    ?.index;

/// The index of the `setField`/`unsetField` override on [field] of [component], or null.
int? fieldOverrideIndexOf(
  PrefabExpansion x, {
  required String entity,
  required String component,
  required String field,
}) => x.overrides
    .where(
      (o) =>
          (o.op == 'setField' || o.op == 'unsetField') &&
          o.entity == entity &&
          o.component == component &&
          o.field == field,
    )
    .firstOrNull
    ?.index;

Object? _setIn(Object? node, List<String> parts, Object? value) {
  if (parts.isEmpty) return value;
  final head = parts.first;
  final rest = parts.sublist(1);
  if (node is Map) {
    final m = Map<String, Object?>.from(node);
    m[head] = _setIn(m[head], rest, value);
    return m;
  }
  if (node is List) {
    final i = int.tryParse(head);
    final l = List<Object?>.of(node);
    if (i == null || i < 0 || i > l.length) return l;
    if (i == l.length) {
      l.add(_setIn(null, rest, value));
    } else {
      l[i] = _setIn(l[i], rest, value);
    }
    return l;
  }
  // Nothing there yet: an object with this key (an array index makes an object too — the crate refuses it).
  return <String, Object?>{head: _setIn(null, rest, value)};
}

Object? _copyJson(Object? v) => switch (v) {
  Map() => <String, Object?>{for (final e in v.entries) e.key as String: _copyJson(e.value)},
  List() => <Object?>[for (final e in v) _copyJson(e)],
  _ => v,
};
