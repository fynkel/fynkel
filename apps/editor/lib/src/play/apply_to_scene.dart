// Apply to Scene (ADR-0021 P7 L4, docs/design/live-play-mode.md decision
// 7): a running component's values written into the open scene document
// on request — one ordinary transaction on the scene's tab, which goes
// dirty and is saved as usual. Only what differs from the scene is
// written (a value the scene leaves out counts as its default); a
// component the scene does not have is added; an entity of a prefab
// instance gets overrides, as editing it outside Play would.
//
// The world keeps native numbers as 32-bit floats, so 0.1 comes back as
// 0.10000000149…, and it renormalises what it reads (a rotation authored
// to six decimals comes back a digit off): two numbers a millionth apart
// are the same value here, and a native number is written as the
// shortest decimal that is its float (0.1).
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../documents/open_document.dart';
import '../hierarchy/hierarchy_tree.dart' show HierarchyNode;
import '../inspector/inspector_model.dart' show topLevelKey;
import '../inspector/prefab_actions.dart' as prefab;

/// One running component to apply: on [entity], the [index]-th component
/// of [type] (the Inspector's slot position), with its live [data].
typedef LiveComponent = ({String entity, String type, int version, int index, Map<String, Object?> data});

/// [id]'s node in [scene]'s tree: an entity of the document, or one of
/// a prefab instance's (whose expansion is fetched when the tree has not
/// needed it yet — a running instance's entities have the derived ids).
HierarchyNode? sceneNodeOf(OpenDocument scene, String id) {
  final tree = scene.tree;
  final known = tree[id];
  if (known != null || id.startsWith('runtime:')) return known;
  for (final n in tree.nodes.values.toList()) {
    if (!n.isPrefabInstance || tree.expansions.containsKey(n.id)) continue;
    scene.expansionOf(n.id);
    final found = tree[id];
    if (found != null) return found;
  }
  return null;
}

/// The commands that write [live] (all of it, or only its top-level key
/// [field]) into [scene]; [defaults] are the type's values for keys the
/// scene leaves out. Throws a [StateError] naming why it cannot.
List<Map<String, Object?>> applyToSceneCommands(
  OpenDocument scene,
  LiveComponent live, {
  required Map<String, Object?> defaults,
  String? field,
}) {
  final node = sceneNodeOf(scene, live.entity);
  if (node == null) throw StateError('It is not in the scene: the game made it.');
  final native = live.type.startsWith('fynkel.');

  // The scene's components of the entity, in order: from the instance's
  // expansion for an entity of a prefab instance, else from the document.
  final String? anchor = node.isDerived ? node.anchor : null;
  String? entityLocal;
  final components = <({String id, String? local, String type, Map<String, Object?> data})>[];
  if (anchor != null) {
    final de = scene.expansionOf(anchor)?.entity(live.entity);
    if (de == null) throw StateError('Its prefab instance did not expand.');
    entityLocal = de.local;
    for (final c in de.components.entries) {
      components.add((id: c.key, local: c.value.local, type: c.value.type, data: c.value.data));
    }
  } else {
    final e = scene.document.entities([live.entity]).entities[live.entity];
    if (e == null) throw StateError('It is not in the scene: the game made it.');
    final map = e['components'];
    if (map is Map) {
      for (final c in map.entries) {
        final v = c.value;
        if (v is! Map) continue;
        final data = v['data'];
        components.add((
          id: c.key as String,
          local: null,
          type: v['type'] as String? ?? '',
          data: data is Map ? data.cast<String, Object?>() : const {},
        ));
      }
    }
  }
  final same = components.where((c) => c.type == live.type).toList();

  Object? written(Object? v) => native ? shortestFloats(v) : v;

  // Not in the scene: the whole component is added — except the Transform
  // the world gives every entity, when it is still the default one.
  if (same.length <= live.index) {
    if (live.type == 'fynkel.Transform' && sameValue(live.data, {...live.data, ...defaults})) return const [];
    final data = {for (final e in live.data.entries) e.key: written(e.value)};
    if (anchor == null) {
      return [Command.addComponent(live.entity, {'type': live.type, 'version': live.version, 'data': data})];
    }
    return [
      Command.setPrefabOverrideOp(
        anchor,
        prefab.addComponentOverride(entity: entityLocal!, type: live.type, version: live.version, data: data),
      ),
    ];
  }

  final c = same[live.index];
  final keys = field == null ? live.data.keys : [topLevelKey(field)];
  final commands = <Map<String, Object?>>[];
  var data = c.data;
  for (final key in keys) {
    if (!live.data.containsKey(key)) continue;
    final now = live.data[key];
    final before = c.data.containsKey(key) ? c.data[key] : defaults[key];
    if (sameValue(now, before)) continue;
    final value = written(now);
    if (anchor == null) {
      commands.add(Command.setField(live.entity, c.id, key, value));
    } else {
      final op = prefab.setFieldOverride(
        entity: entityLocal!,
        component: c.local ?? c.id,
        data: data,
        path: key,
        value: value,
      );
      data = {...data, key: value};
      commands.add(Command.setPrefabOverrideOp(anchor, op));
    }
  }
  return commands;
}

/// Whether [a] and [b] are the same JSON value, numbers a millionth apart
/// (of their size, when above one) counting as the same.
bool sameValue(Object? a, Object? b) {
  if (a is num && b is num) {
    return a == b || (a - b).abs() <= 1e-6 * [1.0, a.abs().toDouble(), b.abs().toDouble()].reduce(math.max);
  }
  if (a is List && b is List) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (!sameValue(a[i], b[i])) return false;
    }
    return true;
  }
  if (a is Map && b is Map) {
    if (a.length != b.length) return false;
    for (final k in a.keys) {
      if (!b.containsKey(k) || !sameValue(a[k], b[k])) return false;
    }
    return true;
  }
  return a == b;
}

/// [v] with every non-integer number replaced by the shortest decimal
/// that reads back as the same 32-bit float (0.10000000149… → 0.1).
Object? shortestFloats(Object? v) => switch (v) {
  double() when v.isFinite && v != v.roundToDouble() => _shortest(v),
  List() => [for (final e in v) shortestFloats(e)],
  Map() => {for (final e in v.entries) e.key: shortestFloats(e.value)},
  _ => v,
};

double _shortest(double v) {
  final f = _f32(v);
  for (var p = 1; p <= 9; p++) {
    final d = double.parse(v.toStringAsPrecision(p));
    if (_f32(d) == f) return d;
  }
  return v;
}

final _scratch = Float32List(1);

double _f32(double v) {
  if (!v.isFinite || v.abs() > 3.4028234663852886e38) return v;
  _scratch[0] = v;
  return _scratch[0];
}
