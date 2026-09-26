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
//
// A state's motion may be a blend tree (ADR-0033): `{blendTree: {type,
// parameters, children: [{motion, threshold | position, timeScale,
// parameter}]}}`, nested. A tree is found by its state and the path of
// child indices down to it ([] is the state's own); its edits keep the
// shape the type needs (a threshold in 1D, a position in 2D, a weight
// parameter in Direct), and a tree always holds a motion (the document
// refuses an empty one), so a new tree starts with the clip it was made
// from.
import 'dart:ui' show Offset;

import '../inspector/asset_edits.dart';
import 'blend_tree_weights.dart';

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

  /// A new state playing a 1D blend tree on the first float parameter,
  /// holding [first] (Unity's Create State ▸ From New Blend Tree; a tree
  /// is never empty, so it starts with a motion).
  String? createTreeState(int layer, Offset at, Map<String, Object?> first) =>
      createState(layer, at, motion: newTree('1d', first: first), name: 'Blend Tree');

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

  // ── Blend trees (ADR-0033) ──────────────────────────────────────────────

  /// The float parameters, in order.
  List<String> get floats => [
    for (final p in parameters)
      if (p['type'] == 'float') p['name'] as String,
  ];

  /// A tree of [type] on the first float parameters, [first] its one
  /// child at the origin.
  Map<String, Object?> newTree(String type, {required Map<String, Object?> first}) => {
    'blendTree': {
      'type': type,
      if (treeParameterCount(type) > 0) 'parameters': floats.take(treeParameterCount(type)).toList(),
      'children': [
        _shaped({'motion': first, 'threshold': 0, 'position': const [0, 0]}, type),
      ],
    },
  };

  /// The pointer of the motion at [path] in [state] of [layer].
  String? motionPointer(int layer, String state, List<int> path) {
    final i = states(layer).indexWhere((s) => s['name'] == state);
    if (i < 0) return null;
    return ['/layers/$layer/states/$i/motion', for (final k in path) '/blendTree/children/$k/motion'].join();
  }

  /// The motion at [path] (a clip's ref, a tree, or null).
  Map<String, Object?>? motionAt(int layer, String state, List<int> path) {
    Object? m = this.state(layer, state)?['motion'];
    for (final k in path) {
      final tree = m is Map ? m['blendTree'] : null;
      final children = tree is Map ? tree['children'] : null;
      if (children is! List || k < 0 || k >= children.length) return null;
      final child = children[k];
      m = child is Map ? child['motion'] : null;
    }
    return m is Map ? _map(m) : null;
  }

  /// The tree at [path]: `{type, parameters, children}`; null when the
  /// motion there is not a tree.
  Map<String, Object?>? treeAt(int layer, String state, List<int> path) {
    final t = motionAt(layer, state, path)?['blendTree'];
    return t is Map ? _map(t) : null;
  }

  List<Map<String, Object?>> treeChildren(int layer, String state, List<int> path) =>
      _maps(treeAt(layer, state, path)?['children']);

  /// Replaces the tree at [path] (one undo step).
  bool _setTree(int layer, String state, List<int> path, Map<String, Object?> tree) {
    final at = motionPointer(layer, state, path);
    return at != null && edits.edit(id, {'$at/blendTree': tree});
  }

  /// Makes the state's motion a blend tree: a 1D tree on the first float
  /// parameter holding the motion it had, else [first] (Unity's Create
  /// new BlendTree in State).
  bool makeTree(int layer, String state, {Map<String, Object?>? first}) {
    final old = motionAt(layer, state, const []);
    if (old != null && old['blendTree'] != null) return true;
    final motion = old ?? first;
    if (motion == null) return _refuse('A blend tree starts with a motion: give the state a clip first.');
    return setStateField(layer, state, 'motion', newTree('1d', first: motion));
  }

  /// A child's place as the type reads it: its position (2D), else its
  /// threshold on x (1D; Direct has neither: 0, 0).
  static (double, double) placeOf(Map<String, Object?> child) {
    final p = child['position'];
    if (p is List && p.length == 2 && p[0] is num && p[1] is num) {
      return ((p[0] as num).toDouble(), (p[1] as num).toDouble());
    }
    final t = child['threshold'];
    return (t is num ? t.toDouble() : 0, 0);
  }

  /// [child] with the member [type] needs, from what it had: a threshold
  /// from a position's x, a position from a threshold, a weight parameter.
  Map<String, Object?> _shaped(Map<String, Object?> child, String type) {
    final (x, y) = placeOf(child);
    final c = {...child}
      ..remove('threshold')
      ..remove('position');
    if (type == '1d') c['threshold'] = x;
    if (treeIs2d(type)) c['position'] = [x, y];
    if (type == 'direct') {
      c['parameter'] ??= floats.firstOrNull;
      if (c['parameter'] == null) c.remove('parameter');
    } else {
      c.remove('parameter');
    }
    return c;
  }

  /// Changes the tree's type: its parameters to the count the type reads
  /// (kept where they fit, else the next floats), each child reshaped.
  bool setTreeType(int layer, String state, List<int> path, String type) {
    final tree = treeAt(layer, state, path);
    if (tree == null || !treeTypes.contains(type)) return false;
    final n = treeParameterCount(type);
    final parameters = [for (final p in (tree['parameters'] as List?) ?? const []) '$p'].take(n).toList();
    for (final f in floats) {
      if (parameters.length >= n) break;
      if (!parameters.contains(f)) parameters.add(f);
    }
    final next = {
      ...tree,
      'type': type,
      'parameters': parameters,
      'children': [for (final c in _maps(tree['children'])) _shaped(c, type)],
    };
    if (parameters.isEmpty) next.remove('parameters');
    return _setTree(layer, state, path, next);
  }

  /// Sets the tree's [index]th parameter.
  bool setTreeParameter(int layer, String state, List<int> path, int index, String name) {
    final tree = treeAt(layer, state, path);
    if (tree == null) return false;
    final list = [for (final p in (tree['parameters'] as List?) ?? const []) '$p'];
    while (list.length <= index) {
      list.add(name);
    }
    list[index] = name;
    return _setTree(layer, state, path, {...tree, 'parameters': list});
  }

  /// A place for a new child of a tree of [type] that no child takes: 1D
  /// one past the last threshold; 2D the first free spot of the unit
  /// cross, then further out along +X.
  static (double, double) freePlace(String type, List<Map<String, Object?>> children) {
    final taken = [for (final c in children) placeOf(c)];
    if (type == '1d') {
      if (taken.isEmpty) return (0, 0);
      return (taken.map((p) => p.$1).reduce((a, b) => a > b ? a : b) + 1, 0);
    }
    const cross = [(0.0, 0.0), (0.0, 1.0), (0.0, -1.0), (-1.0, 0.0), (1.0, 0.0)];
    for (final p in cross) {
      if (!taken.contains(p)) return p;
    }
    for (var x = 2.0; ; x++) {
      if (!taken.contains((x, 0.0))) return (x, 0);
    }
  }

  /// Unity's + ▸ Add Motion ([motion], a clip or blend space) and
  /// + ▸ New Blend Tree ([tree]: a 1D tree holding [motion]): a child at a
  /// free place. Returns its index, or null.
  int? addChild(int layer, String state, List<int> path, {required Map<String, Object?> motion, bool tree = false}) {
    final t = treeAt(layer, state, path);
    if (t == null) return null;
    final type = t['type'] as String? ?? '1d';
    final children = _maps(t['children']);
    if (children.length >= 32) {
      _refuse('A blend tree has 32 children at most.');
      return null;
    }
    final (x, y) = freePlace(type, children);
    final child = _shaped({
      'motion': tree ? newTree('1d', first: motion) : motion,
      'threshold': x,
      'position': [x, y],
    }, type);
    final list = [...children, child];
    return _setTree(layer, state, path, {...t, 'children': list}) ? list.length - 1 : null;
  }

  bool removeChild(int layer, String state, List<int> path, int index) {
    final t = treeAt(layer, state, path);
    final children = _maps(t?['children']);
    if (t == null || index < 0 || index >= children.length) return false;
    if (children.length == 1) return _refuse('A blend tree keeps one motion at least: remove the tree instead.');
    children.removeAt(index);
    return _setTree(layer, state, path, {...t, 'children': children});
  }

  /// Moves a child one place up ([by] -1) or down: its row in the list.
  int? moveChild(int layer, String state, List<int> path, int index, int by) {
    final t = treeAt(layer, state, path);
    final children = _maps(t?['children']);
    final j = index + by.sign;
    if (t == null || index < 0 || index >= children.length || j < 0 || j >= children.length) return null;
    final c = children[index];
    children[index] = children[j];
    children[j] = c;
    return _setTree(layer, state, path, {...t, 'children': children}) ? j : null;
  }

  /// Sets (or with [unset], removes) one member of a child: `motion`,
  /// `threshold`, `position`, `timeScale` or `parameter`.
  bool setChildField(int layer, String state, List<int> path, int index, String key, Object? value) {
    final at = motionPointer(layer, state, path);
    if (at == null || index < 0 || index >= treeChildren(layer, state, path).length) return false;
    return edits.edit(id, {'$at/blendTree/children/$index/$key': value});
  }

  /// Moves children on the map (a drag's end: one undo step): 1D sets the
  /// threshold to x, 2D the position. Rounded to thousandths.
  bool placeChildren(int layer, String state, List<int> path, Map<int, (double, double)> at) {
    final t = treeAt(layer, state, path);
    if (t == null || at.isEmpty) return false;
    final type = t['type'] as String? ?? '1d';
    double round(double v) => (v * 1000).roundToDouble() / 1000;
    final children = [
      for (final (i, c) in _maps(t['children']).indexed)
        if (at[i] case (final x, final y))
          {
            ...c,
            if (type == '1d') 'threshold': round(x) else 'position': [round(x), round(y)],
          }
        else
          c,
    ];
    return _setTree(layer, state, path, {...t, 'children': children});
  }

  /// Unity's Compute Positions: each child's place from what [value] reads
  /// of its clip (a child it has nothing for keeps its place). Two children
  /// that would land on one place are refused, as the document would be.
  bool computePlaces(int layer, String state, List<int> path, (double, double)? Function(int child) value) {
    final t = treeAt(layer, state, path);
    if (t == null) return false;
    final children = _maps(t['children']);
    final at = <int, (double, double)>{
      for (var i = 0; i < children.length; i++)
        i: ?value(i),
    };
    if (at.isEmpty) return _refuse('None of the children plays a clip that travels: the places are kept.');
    double r(double v) => (v * 1000).roundToDouble() / 1000;
    final places = [
      for (var i = 0; i < children.length; i++)
        switch (at[i] ?? placeOf(children[i])) {
          (final x, final y) => (r(x), t['type'] == '1d' ? 0.0 : r(y)),
        },
    ];
    if (places.toSet().length != places.length) {
      return _refuse('Two children would be at one place (their clips travel alike): the places are kept.');
    }
    return placeChildren(layer, state, path, at);
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
            (s) => _renameInTrees(s['speedParameter'] == from ? {...s, 'speedParameter': to} : s, from, to),
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

  /// [s] with every tree under its motion naming [from] naming [to].
  static Map<String, Object?> _renameInTrees(Map<String, Object?> s, String from, String to) {
    Object? motion(Object? m) {
      if (m is! Map || m['blendTree'] is! Map) return m;
      final t = _map(m['blendTree']);
      return {
        ..._map(m),
        'blendTree': {
          ...t,
          if (t['parameters'] is List) 'parameters': [for (final p in t['parameters'] as List) p == from ? to : p],
          if (t['children'] is List)
            'children': [
              for (final c in _maps(t['children']))
                {
                  ...c,
                  if (c['parameter'] == from) 'parameter': to,
                  if (c.containsKey('motion')) 'motion': motion(c['motion']),
                },
            ],
        },
      };
    }

    return s.containsKey('motion') ? {...s, 'motion': motion(s['motion'])} : s;
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
