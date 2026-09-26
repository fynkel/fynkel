// The Animator Controller's edits (ADR-0032 § 8): every change the
// Animator window or the Inspector makes to a `.controller.json` — a
// state made, renamed, moved or deleted, a transition made or reordered,
// a parameter added or renamed, a layer added — as one undo step on the
// document, saved at once ([AssetEdits], as a material's are).
//
// States and parameters are named, and transitions and conditions name
// them, so a rename rewrites every reference in the same step, and a
// delete takes what named it along. Where each box sits in the graph is
// the document's `editor` member: `{layer: {state: [x, y]}}`.
import 'dart:ui' show Offset;

import '../inspector/asset_edits.dart';

/// Entry's and Any State's places in `editor` (a state is never named so:
/// the window refuses a name that starts with `#`).
const entryKey = '#entry';
const anyKey = '#any';

/// The four parameter types, in Unity's order.
const parameterTypes = ['float', 'int', 'bool', 'trigger'];

/// Where a new layer's Entry, Any State and first state go.
const entryAt = Offset(-240, 0);
const anyAt = Offset(-240, 90);

/// A copy of a JSON value, deep, so an edit never changes what the
/// document holds before it is applied.
Object? _copy(Object? v) => switch (v) {
  Map() => {for (final e in v.entries) e.key as String: _copy(e.value)},
  List() => [for (final x in v) _copy(x)],
  _ => v,
};

Map<String, Object?> _map(Object? v) => v is Map ? {for (final e in v.entries) e.key as String: _copy(e.value)} : {};
List<Map<String, Object?>> _maps(Object? v) => [
  if (v is List)
    for (final x in v)
      if (x is Map) _map(x),
];

/// One controller's edits, by its asset id.
final class ControllerEdits {
  ControllerEdits(this.edits, this.id);

  final AssetEdits edits;
  final String id;

  /// Why the last edit was refused, until the next one.
  String? get lastError => edits.lastError;

  Map<String, Object?>? get root => edits.fields(id);
  bool get canEdit => edits.canEditAsset(id);

  List<Map<String, Object?>> get parameters => _maps(root?['parameters']);
  List<Map<String, Object?>> get layers => _maps(root?['layers']);

  Map<String, Object?>? layer(int i) {
    final l = layers;
    return i >= 0 && i < l.length ? l[i] : null;
  }

  List<Map<String, Object?>> states(int layer) => _maps(this.layer(layer)?['states']);
  List<Map<String, Object?>> transitions(int layer) => _maps(this.layer(layer)?['transitions']);
  String? defaultState(int layer) => this.layer(layer)?['defaultState'] as String?;

  Map<String, Object?>? state(int layer, String name) => states(layer).where((s) => s['name'] == name).firstOrNull;

  Map<String, Object?>? parameter(String name) => parameters.where((p) => p['name'] == name).firstOrNull;

  /// Where [key] (a state, [entryKey] or [anyKey]) sits in [layer]'s graph.
  Offset positionOf(int layer, String key) {
    final name = this.layer(layer)?['name'];
    final at = _map(_map(root?['editor'])[name])[key];
    if (at is List && at.length == 2 && at[0] is num && at[1] is num) {
      return Offset((at[0] as num).toDouble(), (at[1] as num).toDouble());
    }
    if (key == entryKey) return entryAt;
    if (key == anyKey) return anyAt;
    // A state never placed: in rows of four to the right of Entry.
    final i = states(layer).indexWhere((s) => s['name'] == key);
    final n = i < 0 ? 0 : i;
    return Offset((n % 4) * 220.0, (n ~/ 4) * 90.0);
  }

  // ── States ──────────────────────────────────────────────────────────────

  /// A name not used in [taken]: [base], else `base 1`, `base 2` ….
  static String unique(String base, Iterable<Object?> taken, {String sep = ' '}) {
    final used = taken.whereType<String>().toSet();
    if (!used.contains(base)) return base;
    for (var i = 1; ; i++) {
      if (!used.contains('$base$sep$i')) return '$base$sep$i';
    }
  }

  /// Why [name] cannot name a state of [layer] (besides [except], the
  /// state being renamed); null when it can.
  String? stateNameProblem(int layer, String name, {String? except}) {
    if (name.trim().isEmpty) return 'A state needs a name.';
    if (name != name.trim()) return 'A state\'s name cannot start or end with a space.';
    if (name.length > 64) return 'A state\'s name is 64 characters at most.';
    if (name == 'any') return '`any` is Any State\'s name in a transition.';
    if (name.startsWith('#')) return 'A state\'s name cannot start with #.';
    if (name != except && state(layer, name) != null) return 'The layer already has a state named $name.';
    return null;
  }

  /// Unity's Create State: a state named after [motion]'s clip (or
  /// `New State`) at [at]; the layer's first state becomes its default.
  /// Returns the new state's name, or null when refused.
  String? createState(int layer, Offset at, {Map<String, Object?>? motion, String? name}) {
    final l = this.layer(layer);
    if (l == null) return null;
    final base = name ?? _motionName(motion) ?? 'New State';
    final made = unique(base, states(layer).map((s) => s['name']));
    final list = [
      ...states(layer),
      {'name': made, 'motion': ?motion},
    ];
    final ok = edits.edit(id, {
      '/layers/$layer/states': list,
      if (defaultState(layer) == null) '/layers/$layer/defaultState': made,
      ..._placed(layer, {made: at}),
    });
    return ok ? made : null;
  }

  /// A clip's or blend space's name as a state's: `anim/Run` reads `Run`.
  String? _motionName(Map<String, Object?>? motion) {
    final sub = motion?['sub'];
    if (sub is String && sub.isNotEmpty) return sub.split('/').last;
    final asset = motion?['asset'];
    if (asset is! String) return null;
    for (final a in edits.project.assets()) {
      if (a.id == asset) return a.name;
    }
    return null;
  }

  /// Renames a state and every transition, default and place that names it.
  bool renameState(int layer, String from, String to) {
    if (from == to) return true;
    final problem = stateNameProblem(layer, to, except: from);
    if (problem != null) return _refuse(problem);
    final l = this.layer(layer);
    if (l == null || state(layer, from) == null) return false;
    final states = [
      for (final s in this.states(layer)) s['name'] == from ? {...s, 'name': to} : s,
    ];
    final transitions = [
      for (final t in this.transitions(layer)) {...t, if (t['from'] == from) 'from': to, if (t['to'] == from) 'to': to},
    ];
    final editor = _map(root?['editor']);
    final places = _map(editor[l['name']]);
    final place = places.remove(from);
    return edits.edit(id, {
      '/layers/$layer/states': states,
      if (transitions.isNotEmpty) '/layers/$layer/transitions': transitions,
      if (l['defaultState'] == from) '/layers/$layer/defaultState': to,
      if (place != null)
        '/editor': {
          ...editor,
          l['name'] as String: {...places, to: place},
        },
    });
  }

  /// Deletes a state with its transitions; the default moves to the first
  /// state left (none: unset).
  bool deleteState(int layer, String name) {
    final l = this.layer(layer);
    if (l == null || state(layer, name) == null) return false;
    final states = [
      for (final s in this.states(layer))
        if (s['name'] != name) s,
    ];
    final transitions = [
      for (final t in this.transitions(layer))
        if (t['from'] != name && t['to'] != name) t,
    ];
    final editor = _map(root?['editor']);
    final places = _map(editor[l['name']]);
    final hadPlace = places.remove(name) != null;
    return edits.edit(id, {
      '/layers/$layer/states': states,
      if (l['transitions'] != null) '/layers/$layer/transitions': transitions.isEmpty ? unset : transitions,
      if (l['defaultState'] == name) '/layers/$layer/defaultState': states.isEmpty ? unset : states.first['name'],
      if (hadPlace) '/editor': {...editor, l['name'] as String: places},
    });
  }

  /// Unity's Set as Layer Default State.
  bool setDefault(int layer, String name) =>
      state(layer, name) != null && edits.edit(id, {'/layers/$layer/defaultState': name});

  /// Moves boxes in [layer]'s graph (a drag's end: one undo step).
  bool place(int layer, Map<String, Offset> at) => edits.edit(id, _placed(layer, at));

  Map<String, Object?> _placed(int layer, Map<String, Offset> at) {
    final name = this.layer(layer)?['name'];
    if (name is! String || at.isEmpty) return {};
    final editor = _map(root?['editor']);
    return {
      '/editor': {
        ...editor,
        name: {
          ..._map(editor[name]),
          for (final e in at.entries) e.key: [e.value.dx.roundToDouble(), e.value.dy.roundToDouble()],
        },
      },
    };
  }

  /// Sets (or with [unset], removes) one member of a state.
  bool setStateField(int layer, String name, String key, Object? value) {
    final i = states(layer).indexWhere((s) => s['name'] == name);
    if (i < 0) return false;
    if (key == 'name') return renameState(layer, name, value as String);
    return edits.edit(id, {'/layers/$layer/states/$i/$key': value});
  }

  // ── Transitions ─────────────────────────────────────────────────────────

  /// Unity's Make Transition: from a state (or `any`) to [to], last in
  /// order. From a state it has Unity's exit time and duration, so it
  /// fires without conditions; from Any State it waits for one.
  /// Returns the new transition's index, or null.
  int? makeTransition(int layer, String from, String to) {
    if (this.layer(layer) == null || state(layer, to) == null) return null;
    if (from != 'any' && state(layer, from) == null) return null;
    final list = [
      ...transitions(layer),
      {'from': from, 'to': to, if (from != 'any') 'exitTime': 0.75, 'duration': 0.25},
    ];
    return edits.edit(id, {'/layers/$layer/transitions': list}) ? list.length - 1 : null;
  }

  bool deleteTransition(int layer, int index) {
    final list = transitions(layer);
    if (index < 0 || index >= list.length) return false;
    list.removeAt(index);
    return edits.edit(id, {'/layers/$layer/transitions': list.isEmpty ? unset : list});
  }

  /// Moves the transition at [index] one place up ([by] -1) or down among
  /// the transitions out of the same state: the order they are tried in.
  /// Returns its new index, or null.
  int? reorderTransition(int layer, int index, int by) {
    final list = transitions(layer);
    if (index < 0 || index >= list.length) return null;
    final from = list[index]['from'];
    var j = index + by.sign;
    while (j >= 0 && j < list.length && list[j]['from'] != from) {
      j += by.sign;
    }
    if (j < 0 || j >= list.length) return null;
    final t = list[index];
    list[index] = list[j];
    list[j] = t;
    return edits.edit(id, {'/layers/$layer/transitions': list}) ? j : null;
  }

  /// Sets (or with [unset], removes) one member of a transition.
  bool setTransitionField(int layer, int index, String key, Object? value) {
    if (index < 0 || index >= transitions(layer).length) return false;
    return edits.edit(id, {'/layers/$layer/transitions/$index/$key': value});
  }

  /// Replaces a transition's conditions (an empty list removes them).
  bool setConditions(int layer, int index, List<Map<String, Object?>> conditions) =>
      setTransitionField(layer, index, 'conditions', conditions.isEmpty ? unset : conditions);

  /// A new condition on [parameter], as Unity starts one: a float
  /// `greater 0`, an int `equals 0`, a bool `is true`, a trigger alone.
  Map<String, Object?> conditionOn(String parameter) => switch (this.parameter(parameter)?['type']) {
    'float' => {'parameter': parameter, 'greater': 0},
    'int' => {'parameter': parameter, 'equals': 0},
    'bool' => {'parameter': parameter, 'is': true},
    _ => {'parameter': parameter},
  };

  // ── Parameters ──────────────────────────────────────────────────────────

  static final _identifier = RegExp(r'^[A-Za-z_][A-Za-z0-9_]{0,63}$');

  /// Why [name] cannot name a parameter (besides [except]); null when it can.
  String? parameterNameProblem(String name, {String? except}) {
    if (!_identifier.hasMatch(name)) {
      return 'A parameter\'s name is a letter or _, then letters, digits or _ (code names it: setFloat(\'speed\', …)).';
    }
    if (name != except && parameter(name) != null) return 'The controller already has a parameter named $name.';
    return null;
  }

  /// Unity's + ▸ Float / Int / Bool / Trigger. Returns its name, or null.
  String? addParameter(String type) {
    if (!parameterTypes.contains(type)) return null;
    final base = switch (type) {
      'float' => 'newFloat',
      'int' => 'newInt',
      'bool' => 'newBool',
      _ => 'newTrigger',
    };
    final name = unique(base, parameters.map((p) => p['name']), sep: '');
    final list = [
      ...parameters,
      {'name': name, 'type': type},
    ];
    return edits.edit(id, {'/parameters': list}) ? name : null;
  }

  /// Renames a parameter and every condition and speed parameter naming it.
  bool renameParameter(String from, String to) {
    if (from == to) return true;
    final problem = parameterNameProblem(to, except: from);
    if (problem != null) return _refuse(problem);
    if (parameter(from) == null) return false;
    return edits.edit(id, {
      '/parameters': [
        for (final p in parameters) p['name'] == from ? {...p, 'name': to} : p,
      ],
      '/layers': [
        for (final l in layers)
          _mapLayer(
            l,
            (s) => s['speedParameter'] == from ? {...s, 'speedParameter': to} : s,
            (c) => c['parameter'] == from ? {...c, 'parameter': to} : c,
          ),
      ],
    });
  }

  /// Deletes a parameter with every condition and speed parameter naming it.
  bool deleteParameter(String name) {
    if (parameter(name) == null) return false;
    final rest = [
      for (final p in parameters)
        if (p['name'] != name) p,
    ];
    return edits.edit(id, {
      '/parameters': rest.isEmpty ? unset : rest,
      '/layers': [
        for (final l in layers)
          _mapLayer(
            l,
            (s) => s['speedParameter'] == name ? ({...s}..remove('speedParameter')) : s,
            (c) => c['parameter'] == name ? null : c,
          ),
      ],
    });
  }

  /// A parameter's default: a number for a float or an int, a bool for a
  /// bool (a trigger has none).
  bool setParameterDefault(String name, Object value) {
    final i = parameters.indexWhere((p) => p['name'] == name);
    if (i < 0) return false;
    return edits.edit(id, {'/parameters/$i/default': value});
  }

  /// [l] with each state through [state] and each condition through
  /// [condition] (null drops it; a transition left with none loses the list).
  Map<String, Object?> _mapLayer(
    Map<String, Object?> l,
    Map<String, Object?> Function(Map<String, Object?>) state,
    Map<String, Object?>? Function(Map<String, Object?>) condition,
  ) => {
    ...l,
    if (l['states'] != null) 'states': [for (final s in _maps(l['states'])) state(s)],
    if (l['transitions'] != null)
      'transitions': [
        for (final t in _maps(l['transitions']))
          if (t['conditions'] == null)
            t
          else
            () {
              final cs = [for (final c in _maps(t['conditions'])) ?condition(c)];
              return cs.isEmpty ? ({...t}..remove('conditions')) : {...t, 'conditions': cs};
            }(),
      ],
  };

  // ── Layers ──────────────────────────────────────────────────────────────

  /// Unity's + on the Layers tab: `New Layer`, whole weight, override.
  /// Returns its index, or null.
  int? addLayer() {
    final name = unique('New Layer', layers.map((l) => l['name']));
    final list = [
      ...layers,
      {'name': name, 'weight': 1},
    ];
    return edits.edit(id, {'/layers': list}) ? list.length - 1 : null;
  }

  bool renameLayer(int layer, String to) {
    final l = this.layer(layer);
    if (l == null) return false;
    final from = l['name'] as String?;
    if (from == to) return true;
    if (to.trim().isEmpty || to != to.trim() || to.length > 64) {
      return _refuse('A layer needs a name of 1 to 64 characters, with no space at either end.');
    }
    if (layers.any((x) => x['name'] == to)) return _refuse('The controller already has a layer named $to.');
    final editor = _map(root?['editor']);
    final places = editor.remove(from);
    return edits.edit(id, {
      '/layers/$layer/name': to,
      if (places != null) '/editor': {...editor, to: places},
    });
  }

  /// Deletes a layer after the base one (the base layer always stays).
  bool deleteLayer(int layer) {
    if (layer <= 0 || this.layer(layer) == null) return false;
    final name = this.layer(layer)?['name'];
    final list = layers..removeAt(layer);
    final editor = _map(root?['editor']);
    final had = editor.remove(name) != null;
    return edits.edit(id, {'/layers': list, if (had) '/editor': editor});
  }

  /// Sets (or with [unset], removes) a layer's `weight`, `blending` or `root`.
  bool setLayerField(int layer, String key, Object? value) =>
      this.layer(layer) != null && edits.edit(id, {'/layers/$layer/$key': value});

  bool _refuse(String why) {
    edits.refuse(why);
    return false;
  }
}
