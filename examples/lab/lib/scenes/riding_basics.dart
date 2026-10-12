// Stage 6: riding a platform. A CharacterController stands on whatever
// is under it (ground) and goes along when that moves or turns, in the
// same frame: a platform moved by a tween, as Unity's moving platforms.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';
import 'character_basics.dart';

class RidingBasicsScene extends LabScene {
  @override
  String get title => 'Riding a platform';
  @override
  String get step => 'stage 6';
  @override
  String get about =>
      'The orange platform slides to and fro and the purple one turns, both by tweens. Walk onto one (WASD or '
      'the arrows, space to jump): the player rides along, as its controller\'s moveWithGround says. Its '
      'ground is what it stands on, and groundVelocity how fast that goes.';

  @override
  Vec3 get eye => Vec3(0, 7, 11);
  @override
  Vec3 get target => Vec3(0, 0.5, 0);

  late final Player player;
  late final GameObject slider, turner;

  @override
  void build() {
    add(floorCollider());
    slider = add(block('sliding platform', Vec3(-3, 0, 0), Vec3(2.5, 0.5, 2.5)));
    slider.get<MeshRenderer>()!.color = const Color(0xFFE0A05A);
    turner = add(block('turning platform', Vec3(3, 0, 0), Vec3(3, 0.5, 3)));
    turner.get<MeshRenderer>()!.color = const Color(0xFFB07CE0);
    player = add(Player()..transform = Transform(position: Vec3(-3, 1.6, 0)));
  }

  @override
  void onStart() {
    super.onStart();
    slider.moveBy(Vec3(0, 0, -3), seconds: 2, ease: Ease.inOutSine, loops: -1, yoyo: true);
    turner.rotateBy(Vec3.up, 2 * math.pi, seconds: 6, ease: Ease.linear, loops: -1);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final c = player.controller;
    readout.value = [
      'ground: ${c.ground?.gameObject.name ?? 'nothing'}',
      'groundVelocity: ${c.groundVelocity.length.toStringAsFixed(2)} m/s',
      'moveWithGround: ${c.moveWithGround}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Toggle('ride along', () => player.controller.moveWithGround, (on) {
      player.controller.moveWithGround = on;
      log.add('moveWithGround = $on');
    }),
    Button('onto the sliding one', () {
      player.position = slider.position + Vec3(0, 1.4, 0);
      log.add('put on the sliding platform');
    }),
  ];
}
