// The Animator window's state (ADR-0032 § 8): which Animator Controller it
// shows, the layer in front, what is selected in the graph and a Make
// Transition waiting for its target.
//
// As Unity's window, it follows the selection: an object whose Animator
// names a controller, or a controller selected in the Project, shows that
// controller; anything else leaves it where it was. A state or transition
// picked in the graph goes to the Inspector ([inspect]); every change is
// one undo step on the controller ([ControllerEdits]).
import 'dart:ui' show Offset;

import 'package:flutter/foundation.dart';
import 'package:fynkel/gameplay.dart' as fynkel;

import '../hierarchy/hierarchy_tree.dart';
import '../inspector/asset_edits.dart';
import '../inspector/inspector_model.dart';
import '../shell/editor_selection.dart';
import 'controller_edits.dart';

/// A running animator during Play (ADR-0032 § 8): the state of each layer
/// and the parameters as the last tick left them, and the parameters set
/// by hand (Unity's way to try a transition).
abstract interface class AnimatorLive {
  List<fynkel.AnimatorStateInfo> get layers;
  List<fynkel.AnimatorParameter> get parameters;

  /// Sets [name] to a `double`, an `int` or a `bool`; for a trigger, true
  /// sets it and false resets it.
  void setParameter(String name, Object value);
}

/// A running object's animator ([fynkel.GameObject.animation]).
final class GameObjectLive implements AnimatorLive {
  GameObjectLive(this.object);

  final fynkel.GameObject object;

  @override
  List<fynkel.AnimatorStateInfo> get layers => _read(() => object.animation.layers);

  @override
  List<fynkel.AnimatorParameter> get parameters => _read(() => object.animation.parameters);

  /// An object gone (or its Animator) reads as nothing.
  static List<T> _read<T>(List<T> Function() f) {
    try {
      return f();
    } on StateError {
      return const [];
    }
  }

  @override
  void setParameter(String name, Object value) {
    final a = object.animation;
    final kind = parameters.where((p) => p.name == name).firstOrNull?.kind;
    switch ((kind, value)) {
      case (fynkel.AnimatorParameterKind.trigger, final bool on):
        on ? a.setTrigger(name) : a.resetTrigger(name);
      case (fynkel.AnimatorParameterKind.float, final num v):
        a.setFloat(name, v.toDouble());
      case (fynkel.AnimatorParameterKind.int, final num v):
        a.setInteger(name, v.round());
      case (fynkel.AnimatorParameterKind.bool, final bool v):
        a.setBool(name, v);
      default:
        break;
    }
  }
}

/// A node of the followed object (ADR-0033 § 5): what a layer's mask
/// ticks and an IK goal names.
final class AnimatorNode {
  const AnimatorNode(this.name, this.binding, this.depth, this.bindings);

  final String name;
  final String binding;

  /// 0 for the object itself.
  final int depth;

  /// Its binding and its parents’ bindings up to the object’s — what
  /// [ControllerEdits.maskedIn] reads.
  final List<String> bindings;
}

/// The names under [nodes]' node [i] (the ones after it that are deeper).
List<String> bindingsUnder(List<AnimatorNode> nodes, int i) => [
  for (var k = i + 1; k < nodes.length && nodes[k].depth > nodes[i].depth; k++)
    if (nodes[k].binding.isNotEmpty) nodes[k].binding,
];

final class AnimatorModel extends ChangeNotifier {
  AnimatorModel({required this.edits}) {
    edits.addListener(_onEdits);
  }

  final AssetEdits edits;

  /// Shows a state or a transition in the Inspector (the shell points it
  /// at the Inspector the user last used).
  void Function(InspectTarget target)? inspect;

  String? _id;
  int _layer = 0;
  List<String> _machine = const [];
  String? _state;
  int? _transition;
  String? _linkFrom;

  /// The object the window follows (its Animator names the controller);
  /// null when a controller was picked in the Project.
  String? get object => _object;
  String? _object;

  /// The open scene's hierarchy [object] is in (its nodes, imported models'
  /// included); null when a controller was picked in the Project.
  HierarchyTree? _tree;

  /// Changes when the object's nodes may have (an imported model's nodes
  /// arrive a moment after it is first looked at).
  Listenable? get nodesChanged => _tree;

  /// The object and every node under it, depth first; empty without one.
  List<AnimatorNode> get nodes {
    final tree = _tree;
    final id = _object;
    if (tree == null || id == null) return const [];
    final out = <AnimatorNode>[];
    void walk(String id, int depth, List<String> above) {
      final n = tree[id];
      if (n == null || out.length >= 4000 || depth > 48) return;
      final bindings = [if (n.animationBinding.isNotEmpty) n.animationBinding, ...above];
      out.add(AnimatorNode(n.name, n.animationBinding, depth, bindings));
      for (final c in tree.childrenOf(id)) {
        walk(c, depth + 1, bindings);
      }
    }

    walk(id, 0, const []);
    return out;
  }

  /// During Play, the running animator of the object [object] names (the
  /// shell looks it up in the game's world); null otherwise.
  AnimatorLive? Function(String object)? liveOf;

  /// The running animator the window shows, during Play with an object
  /// followed; null otherwise.
  AnimatorLive? get live => _object == null ? null : liveOf?.call(_object!);

  /// Play started or stopped: the window looks for [live] again.
  void playChanged() => notifyListeners();

  /// The controller shown, or null (nothing picked yet).
  String? get id => _id;
  ControllerEdits? get controller => _id == null ? null : ControllerEdits(edits, _id!, machine: _machine);

  int get layer => _layer;

  /// The sub-state machine the graph shows (ADR-0033 § 4), as the names
  /// down from the layer; empty: the layer's own states.
  List<String> get machine => _machine;

  /// Opens a sub-state machine of the graph shown (a double-click).
  void enterMachine(String name) {
    if (!ControllerEdits.isMachine(controller?.state(_layer, name))) return;
    _openMachine([..._machine, name]);
  }

  /// The path bar: back up to [depth] names (0: the layer).
  void openMachine(int depth) {
    if (depth >= _machine.length) return;
    _openMachine(_machine.sublist(0, depth));
  }

  void _openMachine(List<String> path) {
    _machine = List.unmodifiable(path);
    _state = null;
    _transition = null;
    _linkFrom = null;
    notifyListeners();
  }

  String? get selectedState => _state;
  int? get selectedTransition => _transition;

  /// Make Transition's source, waiting for a click on the target.
  String? get linkFrom => _linkFrom;

  /// Shows controller [id] ([object]: the one whose Animator names it).
  void show(String? id, {String? object}) {
    if (object == null) _tree = null;
    if (id == _id) {
      if (object != _object) {
        _object = object;
        notifyListeners();
      }
      return;
    }
    _object = object;
    _id = id;
    _layer = 0;
    _machine = const [];
    _state = null;
    _transition = null;
    _linkFrom = null;
    notifyListeners();
  }

  /// Follows the Inspector's target: an object with an Animator that names
  /// a controller, a controller asset, or a state of one.
  void follow(InspectTarget? target, InspectorModel? objects) {
    switch (target) {
      case InspectAsset(:final id, sub: null) when _isController(id):
        show(id);
      case InspectAnimator(:final controller):
        if (controller != _id) show(controller);
      case InspectEntities():
        final one = objects?.single;
        if (one == null) return;
        final slot = objects!.slots.where((s) => s.type == 'fynkel.Animator').firstOrNull;
        final ref = slot?.value('controller');
        final asset = ref is Map ? ref['asset'] : null;
        if (asset is String) {
          _tree = objects.tab?.tree;
          show(asset, object: one.id);
        }
      default:
        break;
    }
  }

  bool _isController(String id) {
    for (final a in edits.project.assets()) {
      if (a.id == id) return a.document == 'controller';
    }
    return false;
  }

  set layer(int i) {
    if (i == _layer) return;
    _layer = i;
    _machine = const [];
    _state = null;
    _transition = null;
    _linkFrom = null;
    notifyListeners();
  }

  /// Picks a state (null: nothing) and shows it in the Inspector.
  void selectState(String? name) {
    _state = name;
    _transition = null;
    notifyListeners();
    if (name != null && _id != null) inspect?.call(InspectAnimator(_id!, _layer, state: name, machine: _machine));
  }

  /// Picks a transition by its index in the layer and shows it.
  void selectTransition(int? index) {
    _transition = index;
    _state = null;
    notifyListeners();
    if (index != null && _id != null) {
      inspect?.call(InspectAnimator(_id!, _layer, transition: index, machine: _machine));
    }
  }

  void clearSelection() {
    if (_state == null && _transition == null && _linkFrom == null) return;
    _state = null;
    _transition = null;
    _linkFrom = null;
    notifyListeners();
  }

  /// Unity's Make Transition: the arrow follows the pointer until a state
  /// is clicked ([link]) or it is cancelled.
  void startLink(String from) {
    _linkFrom = from;
    notifyListeners();
  }

  void cancelLink() {
    if (_linkFrom == null) return;
    _linkFrom = null;
    notifyListeners();
  }

  /// Ends Make Transition on [to]: the transition is made and selected.
  bool link(String to) {
    final from = _linkFrom;
    final c = controller;
    _linkFrom = null;
    if (from == null || c == null) {
      notifyListeners();
      return false;
    }
    final i = c.makeTransition(_layer, from, to);
    if (i == null) {
      notifyListeners();
      return false;
    }
    selectTransition(i);
    return true;
  }

  // The edits that move the selection with them.

  /// Create State at [at], playing [motion] (with [tree], a blend tree
  /// holding it, ADR-0033): the new state is selected.
  /// Create Sub-State Machine at [at]: the new machine is selected.
  String? createMachine(Offset at) {
    final name = controller?.createMachine(_layer, at);
    if (name != null) selectState(name);
    return name;
  }

  String? createState(Offset at, {Map<String, Object?>? motion, bool tree = false}) {
    final c = controller;
    final name = tree && motion != null
        ? c?.createTreeState(_layer, at, motion)
        : c?.createState(_layer, at, motion: motion);
    if (name != null) selectState(name);
    return name;
  }

  bool renameState(String from, String to) {
    final ok = controller?.renameState(_layer, from, to) ?? false;
    if (ok && _state == from) selectState(to);
    return ok;
  }

  bool deleteState(String name) {
    final ok = controller?.deleteState(_layer, name) ?? false;
    if (ok) clearSelection();
    return ok;
  }

  bool deleteTransition(int index) {
    final ok = controller?.deleteTransition(_layer, index) ?? false;
    if (ok) clearSelection();
    return ok;
  }

  /// Deletes what is selected (⌫ in the graph).
  bool deleteSelected() => switch ((_state, _transition)) {
    (final s?, _) => deleteState(s),
    (_, final t?) => deleteTransition(t),
    _ => false,
  };

  int? addLayer() {
    final i = controller?.addLayer();
    if (i != null) layer = i;
    return i;
  }

  bool deleteLayer(int i) {
    final ok = controller?.deleteLayer(i) ?? false;
    if (ok && _layer >= i) {
      _layer = i - 1;
      clearSelection();
      notifyListeners();
    }
    return ok;
  }

  void _onEdits() {
    final c = controller;
    if (c == null) return;
    // An undo can take the layer or the selection away.
    final layers = c.layers.length;
    if (layers > 0 && _layer >= layers) _layer = layers - 1;
    // A machine renamed or deleted (or undone away): back to the layer.
    if (_machine.isNotEmpty && c.scope(_layer) == null) {
      _machine = const [];
      _state = null;
      _transition = null;
    }
    if (_state != null && c.state(_layer, _state!) == null) _state = null;
    if (_transition != null && _transition! >= c.transitions(_layer).length) _transition = null;
    notifyListeners();
  }

  @override
  void dispose() {
    edits.removeListener(_onEdits);
    super.dispose();
  }
}
