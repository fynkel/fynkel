// Step 1: components. Behaviours put on and taken off objects while the
// game runs, found by type.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class ComponentsScene extends LabScene {
  @override
  String get title => 'Components';
  @override
  String get step => 'step 1';
  @override
  String get about =>
      'Tap a crate to pick it, then put behaviours on it and take them off. '
      'Each behaviour is its own class: Spin, Bob and Pulse. They stack, and each logs when it loads, starts and goes.';

  final _crates = <GameObject>[];
  GameObject? _picked;
  late final GameObject _marker;

  @override
  void build() {
    for (final (i, color) in [const Color(0xFFE0613A), const Color(0xFF4CA6E0), const Color(0xFF7BC86C)].indexed) {
      _crates.add(
        add(shape('crate ${i + 1}', Mesh.cube(), color, at: Vec3(i * 2.5 - 2.5, 0.5, 0), components: [Pick()])),
      );
    }
    _marker = add(shape('marker', Mesh.sphere(), const Color(0xFFFFD040), size: Vec3(0.25, 0.25, 0.25)));
    pick(_crates.first);
  }

  void pick(GameObject crate) {
    _picked = crate;
    log.add('picked ${crate.name}: ${_names(crate)}');
  }

  String _names(GameObject crate) {
    final names = crate.getAll<Behaviour>().map((b) => '${b.runtimeType}').where((n) => n != 'Pick').toList();
    return names.isEmpty ? 'no behaviours' : names.join(', ');
  }

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    // After every update, so the marker sits on the crate wherever it went.
    if (_picked case final crate?) {
      _marker.transform = Transform(
        position: crate.transform.position + Vec3(0, 1.2, 0),
        scale: Vec3(0.25, 0.25, 0.25),
      );
    }
    readout.value = [
      'Spin ×${world.all<Spin>().length}   Bob ×${world.all<Bob>().length}   Pulse ×${world.all<Pulse>().length}',
      'on ${_picked?.name}: ${_picked == null ? '' : _names(_picked!)}',
    ].join('\n');
  }

  void _put(Behaviour Function() make) {
    final crate = _picked;
    if (crate == null) return;
    crate.addComponent(make());
  }

  void _takeLast() {
    final crate = _picked;
    if (crate == null) return;
    final mine = crate.getAll<LabBehaviour>();
    if (mine.isEmpty) {
      log.add('${crate.name} has nothing to take off');
      return;
    }
    crate.removeComponent(mine.last);
  }

  @override
  List<Control> get controls => [
    Button('+ Spin', () => _put(Spin.new)),
    Button('+ Bob', () => _put(Bob.new)),
    Button('+ Pulse', () => _put(Pulse.new)),
    Button('− last', _takeLast),
    Button('Find all Spins', () {
      final spins = world.all<Spin>();
      log.add('world.all<Spin>(): ${spins.length} → ${spins.map((s) => s.gameObject.name).join(', ')}');
    }),
  ];
}

/// Picks its crate when it's tapped.
class Pick extends Behaviour {
  @override
  bool onTap(Tap tap) {
    gameObject.ancestor<ComponentsScene>()?.pick(gameObject);
    return true;
  }
}

/// A behaviour that logs its life in the scene's log.
abstract class LabBehaviour extends Behaviour {
  void say(String what) => world.first<LabScene>()?.log.add('$runtimeType on ${gameObject.name}: $what');

  @override
  void onLoad() {
    super.onLoad();
    say('onLoad');
  }

  @override
  void onStart() {
    super.onStart();
    say('onStart');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    say('onDestroy');
  }
}

/// Turns its object.
class Spin extends LabBehaviour {
  @override
  void onUpdate() {
    super.onUpdate();
    gameObject.rotate(Vec3.up, 2 * time.delta);
  }
}

/// Moves its object up and down.
class Bob extends LabBehaviour {
  double _was = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    final now = 0.4 * math.sin(time.sinceStart * 3);
    gameObject.translate(Vec3(0, now - _was, 0));
    _was = now;
  }

  @override
  void onDestroy() {
    // Taken off: put the object back where it was. (Not when it's going.)
    if (!gameObject.isDestroyed) gameObject.translate(Vec3(0, -_was, 0));
    super.onDestroy();
  }
}

/// Grows and shrinks its object.
class Pulse extends LabBehaviour {
  @override
  void onUpdate() {
    super.onUpdate();
    final s = 1 + 0.25 * math.sin(time.sinceStart * 5);
    final t = gameObject.transform;
    gameObject.transform = Transform(position: t.position, rotation: t.rotation, scale: Vec3(s, s, s));
  }

  @override
  void onDestroy() {
    if (!gameObject.isDestroyed) {
      final t = gameObject.transform;
      gameObject.transform = Transform(position: t.position, rotation: t.rotation);
    }
    super.onDestroy();
  }
}
