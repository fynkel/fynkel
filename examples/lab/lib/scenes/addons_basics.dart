// Stage 6: what a package adds, through package:fynkel/extend.dart. A
// tiny add-on, a WorldSystem, counts the components put on and taken off
// objects; an invisible button, a box collider with nothing drawn, takes
// clicks, as physics makes colliders tappable through the same seam.

import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

/// The add-on: counts every component put on or taken off an object.
class ComponentCounter extends WorldSystem {
  ComponentCounter(super.world);

  var added = 0;
  var removed = 0;

  @override
  void componentAdded(GameObject object, Component component) {
    super.componentAdded(object, component);
    added++;
  }

  @override
  void componentRemoved(GameObject object, Component component) {
    super.componentRemoved(object, component);
    removed++;
  }
}

/// A box with a collider and nothing drawn: clicks find it all the same.
class InvisibleButton extends GameObject {
  InvisibleButton()
    : super(
        name: 'invisible button',
        transform: Transform(position: Vec3(0, 1, 0)),
        components: [BoxCollider(size: Vec3(1.5, 1.5, 1.5))],
      );

  var clicks = 0;

  @override
  bool onClick(Finger finger) {
    clicks++;
    ancestor<AddonsBasicsScene>()!.log.add('the invisible button: clicked');
    return true;
  }
}

class AddonsBasicsScene extends LabScene {
  @override
  String get title => 'Add-ons: the basics';
  @override
  String get step => 'stage 6';
  @override
  String get about =>
      'A package adds to Fynkel with a WorldSystem (package:fynkel/extend.dart). This scene\'s add-on counts '
      'every component put on or taken off an object: add and remove cubes (a renderer and a collider each). '
      'In the middle is an invisible button, a box collider with nothing drawn: click there. Physics makes '
      'colliders tappable through the same seam; "draw colliders" shows it.';

  @override
  Vec3 get eye => Vec3(0, 4, 8);
  @override
  Vec3 get target => Vec3(0, 0.5, 0);

  late final ComponentCounter counter;
  late final InvisibleButton button;
  final cubes = <GameObject>[];

  @override
  void build() {
    counter = world.system(ComponentCounter.new);
    button = add(InvisibleButton());
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = [
      'components added: ${counter.added}',
      'components removed: ${counter.removed}',
      'invisible button clicks: ${button.clicks}',
    ].join('\n');
  }

  void addCube() {
    final x = -3 + (cubes.length % 7).toDouble();
    cubes.add(
      add(
        shape(
          'cube ${cubes.length}',
          Mesh.cube(),
          const Color(0xFF4CA6E0),
          at: Vec3(x, 0.3, 2),
          size: Vec3(0.6, 0.6, 0.6),
          components: [BoxCollider()],
        ),
      ),
    );
    log.add('added a cube: 2 components');
  }

  void removeCube() {
    if (cubes.isEmpty) return;
    cubes.removeLast().destroy();
    log.add('destroyed a cube: 2 components gone');
  }

  @override
  List<Control> get controls => [
    Button('add a cube', addCube),
    Button('remove a cube', removeCube),
    Toggle('draw colliders', () => physics.drawColliders, (on) => physics.drawColliders = on),
    Toggle('colliders take taps', () => physics.pointerHitsColliders, (on) {
      physics.pointerHitsColliders = on;
      log.add(on ? 'colliders take taps again' : 'only what\'s drawn takes taps now');
    }),
  ];
}
