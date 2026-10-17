// Stage 9, phase 2: skinned models. KayKit's Knight and Rogue, whose
// meshes bend with their skeletons, playing the same clip; the Rogue holds
// a loose sword put in its hand. They can walk round in a ring, so the
// bent meshes are seen from every side, their shadows bending with them.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// The clips the buttons play, as KayKit names them.
const _clips = ['Idle', 'Walking_A', 'Running_A', '1H_Melee_Attack_Chop', 'Cheer', 'Jump_Full_Short'];

class SkinnedScene extends LabScene {
  @override
  String get title => 'Animation: skinned';
  @override
  String get step => 'stage 9';
  @override
  String get about =>
      "The Knight's and the Rogue's meshes bend with their skeletons: elbows, knees, the cape. The Rogue holds a "
      'loose sword, put in its hand: it follows the hand. Walk them round to see every side, and their shadows.';

  @override
  Vec3 get eye => Vec3(0, 1.9, 4.6);
  @override
  Vec3 get target => Vec3(0, 0.9, 0);

  /// The Knight and the Rogue, once loaded.
  List<GameObject> get people => _people;

  /// The loose sword in the Rogue's hand, once loaded.
  GameObject? get sword => _sword;

  final _people = <GameObject>[];
  final _animators = <Animator>[];
  GameObject? _sword;
  var _clip = 'Idle', _marching = false, _angle = 0.0, _fps = '–';

  @override
  void build() {
    find('sun')!.get<Light>()!.shadows = true;
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    final knight = add(await assets.instantiate('kaykit/knight.glb'))..position = Vec3(-1.1, 0, 0);
    final rogue = add(await assets.instantiate('kaykit/rogue.glb'))..position = Vec3(1.1, 0, 0);
    // The files carry every weapon the pack has, all shown: keep a sword
    // and a shield for the Knight, nothing for the Rogue.
    for (final extra in ['1H_Sword_Offhand', 'Badge_Shield', 'Rectangle_Shield', 'Spike_Shield', '2H_Sword']) {
      knight.find(extra)?.active = false;
    }
    for (final extra in ['Knife_Offhand', '1H_Crossbow', '2H_Crossbow', 'Knife', 'Throwable']) {
      rogue.find(extra)?.active = false;
    }
    _sword = (await assets.instantiate('kaykit/sword.glb'))..name = 'loose sword';
    rogue.find('handslot.r')!.add(_sword!);

    _people.addAll([knight, rogue]);
    for (final person in _people) {
      _animators.add(person.animator!..play(_clip));
    }
  }

  void _play(String clip) {
    _clip = clip;
    final once = clip == '1H_Melee_Attack_Chop' || clip == 'Jump_Full_Short' || clip == 'Cheer';
    for (final animator in _animators) {
      animator
        ..crossFade(clip, duration: 0.2, loop: !once)
        ..onEnd = once ? (_) => _play(_marching ? 'Walking_A' : 'Idle') : null;
    }
  }

  set marching(bool on) {
    _marching = on;
    _play(on ? 'Walking_A' : 'Idle');
    if (!on) {
      _people[0]
        ..position = Vec3(-1.1, 0, 0)
        ..rotation = Quat.identity;
      _people[1]
        ..position = Vec3(1.1, 0, 0)
        ..rotation = Quat.identity;
    }
  }

  set holding(bool on) => _sword?.active = on;

  @override
  void onUpdate() {
    super.onUpdate();
    if (_marching && _people.length == 2) {
      // Round a ring of 1.6 m at the walk's pace, about 1.1 m/s.
      _angle += time.delta * 1.1 / 1.6;
      for (var i = 0; i < 2; i++) {
        final a = _angle + i * math.pi;
        _people[i]
          ..position = Vec3(1.6 * math.cos(a), 0, 1.6 * math.sin(a))
          // Facing along the ring: KayKit's characters face +z.
          ..rotation = Quat.axisAngle(Vec3.up, -a);
      }
    }
    readout.value = 'playing $_clip   $_fps fps';
  }

  @override
  List<Control> get controls => [
    for (final clip in _clips) Button(clip, () => _play(clip)),
    Toggle('walk round', () => _marching, (on) => marching = on),
    Toggle('the Rogue holds the sword', () => _sword?.active ?? false, (on) => holding = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('skinned', s));
}
