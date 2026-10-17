// Stage 9, phases 2 and 3: skinned models, and blending. KayKit's Knight
// and Rogue, whose meshes bend with their skeletons; the Rogue holds a
// loose sword put in its hand. They go round a ring at the speed asked
// for, idle, walk and run blended by it so their feet keep pace, and can
// wave with the right arm on a layer over the rest. A chop, a cheer or a
// jump fades in from whatever they're doing and back out to the blend.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// The clips the buttons play once, as KayKit names them.
const _once = ['1H_Melee_Attack_Chop', 'Cheer', 'Jump_Full_Short'];

/// The speed blend: standing, walking and running, at the speeds (m/s)
/// their feet go.
const _moves = ['Idle', 'Walking_A', 'Running_A'];
const _speeds = [0.0, 1.1, 3.4];

/// The radius of the ring they go round, in metres.
const _ring = 1.6;

class SkinnedScene extends LabScene {
  @override
  String get title => 'Animation: skinned';
  @override
  String get step => 'stage 9';
  @override
  String get about =>
      "The Knight's and the Rogue's meshes bend with their skeletons: elbows, knees, the cape. The Rogue holds a "
      'loose sword, put in its hand: it follows the hand. The speed blends idle, walk and run, so their feet keep '
      'pace round the ring; the wave is a layer on the right arm, over the rest. A chop, a cheer or a jump fades in '
      "from whatever they're doing, and back.";

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
  // The speed asked for, in m/s; a clip playing once, if one is; whether
  // they wave.
  var _speed = 0.0, _angle = 0.0, _fps = '–';
  String? _playing;
  var _waving = false;

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
      final animator = person.animator!
        ..blend('move', _moves, thresholds: _speeds, at: 0, duration: 0)
        ..onEnd = (_) => _playing = null;
      // The right arm from the shoulder down, over whatever plays.
      animator.layer(1, mask: 'upperarm.r');
      _animators.add(animator);
    }
  }

  /// Plays [clip] once, fading in from whatever they're doing; then back
  /// to the blend.
  void playOnce(String clip) {
    _playing = clip;
    for (final animator in _animators) {
      animator.crossFade(clip, duration: 0.2, loop: false);
    }
  }

  /// How fast they go round the ring, in m/s: 0 to stand.
  double get speed => _speed;
  set speed(double value) {
    _speed = value;
    if (value == 0) {
      _people[0]
        ..position = Vec3(-1.1, 0, 0)
        ..rotation = Quat.identity;
      _people[1]
        ..position = Vec3(1.1, 0, 0)
        ..rotation = Quat.identity;
    }
  }

  /// Whether they wave with the right arm.
  bool get waving => _waving;
  set waving(bool on) {
    _waving = on;
    for (final animator in _animators) {
      final arm = animator.layer(1);
      on ? arm.crossFade('Cheer', duration: 0.3) : arm.stop(duration: 0.3);
    }
  }

  set holding(bool on) => _sword?.active = on;

  @override
  void onUpdate() {
    super.onUpdate();
    if (_people.length < 2) return;
    // The blend each frame, unless a clip plays once: it fades back when
    // that ends.
    if (_playing == null) {
      for (final animator in _animators) {
        animator.blend('move', _moves, thresholds: _speeds, at: _speed, duration: 0.3);
      }
    }
    if (_speed > 0) {
      _angle += time.delta * _speed / _ring;
      for (var i = 0; i < 2; i++) {
        final a = _angle + i * math.pi;
        _people[i]
          ..position = Vec3(_ring * math.cos(a), 0, _ring * math.sin(a))
          // Facing along the ring: KayKit's characters face +z.
          ..rotation = Quat.axisAngle(Vec3.up, -a);
      }
    }
    readout.value = 'playing ${_animators[0].current}   ${_speed.toStringAsFixed(1)} m/s   $_fps fps';
  }

  @override
  List<Control> get controls => [
    Slider('speed (m/s)', () => _speed, (v) => speed = v, max: 3.4),
    for (final clip in _once) Button(clip, () => playOnce(clip)),
    Toggle('wave with the right arm', () => _waving, (on) => waving = on),
    Toggle('the Rogue holds the sword', () => _sword?.active ?? false, (on) => holding = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('skinned', s));
}
