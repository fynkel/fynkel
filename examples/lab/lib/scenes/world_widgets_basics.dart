// Stage 7: Flutter widgets in the world. Each cube carries a name tag, a
// WorldWidget the view places where its camera sees the cube, each frame;
// the far one is smaller (shrinkBeyond). One tag has a button: a tap on it
// is the button's, not the game's. A click on a cube pops a "+1" at that
// point, on an empty object that rises and goes.

import 'dart:math' as math;

import 'package:flutter/material.dart' show Alignment;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';
import '../widgets/tags.dart';

/// A cube going round, with its name over it.
class Tagged extends GameObject {
  Tagged(String name, this.radius, this.speed, {bool button = false})
    : super(
        name: name,
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF4CA6E0)))],
      ) {
    addComponent(
      WorldWidget(
        (context) => button ? ButtonTag(name, onPressed: _pressed) : NameTag(name),
        offset: Vec3(0, 0.8, 0),
        alignment: Alignment.bottomCenter,
        shrinkBeyond: 8,
      ),
    );
  }

  final double radius, speed;
  var _angle = 0.0;
  var presses = 0;

  void _pressed() {
    presses++;
    ancestor<LabScene>()!.log.add('$name: its tag\'s button, ×$presses (the game saw no tap)');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _angle += speed * time.delta;
    position = Vec3(radius * math.sin(_angle), 0.5, radius * math.cos(_angle));
  }

  @override
  bool onClick(Finger finger) {
    parent!.add(Pop(position + Vec3(0, 1.2, 0)));
    ancestor<LabScene>()!.log.add('$name: clicked, +1');
    return true;
  }
}

/// "+1" at a point in the world: rises, and goes.
class Pop extends GameObject {
  Pop(Vec3 at)
    : super(
        name: 'pop',
        transform: Transform(position: at),
      ) {
    addComponent(WorldWidget((context) => const PopText('+1')));
  }

  @override
  void onStart() {
    super.onStart();
    moveTo(position + Vec3(0, 1, 0), seconds: 0.8).then((_) => destroy());
  }
}

class WorldWidgetsBasicsScene extends LabScene {
  @override
  String get title => 'Widgets in the world: the basics';
  @override
  String get step => 'stage 7';
  @override
  String get about =>
      'Each cube has a name tag: a Flutter widget (WorldWidget) placed where the camera sees it, each frame. '
      'The far one shrinks with distance. Tap the button on the inner cube\'s tag: the button takes the tap, '
      'not the game. Click a cube: a "+1" pops up at that point. "hide the inner one" makes it inactive, and '
      'its tag goes with it.';

  @override
  Vec3 get eye => Vec3(0, 5, 11);

  late final List<Tagged> cubes;

  @override
  void build() {
    cubes = [add(Tagged('inner', 2, 0.6, button: true)), add(Tagged('middle', 4, -0.4)), add(Tagged('outer', 7, 0.25))];
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = 'the inner tag\'s button: pressed ×${cubes[0].presses}';
  }

  @override
  List<Control> get controls => [Toggle('hide the inner one', () => !cubes[0].active, (on) => cubes[0].active = !on)];
}
