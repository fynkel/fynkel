// Stage 9, phase 4: a crowd. 1, 10, 30 or 100 of KayKit's Knights and
// Rogues in a square, each on its own clip from its own time, so no two
// move alike. The readout says what the animation costs a frame: from the
// start of the updates to the end of the posing (the crowd's own update
// does nothing), the median of the last second. Turned off, each holds
// where it was and the animation costs nothing, to compare with.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/extend.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// How many the buttons ask for.
const crowdSizes = [1, 10, 30, 100];

/// The space between two of them in the square, in metres.
const _gap = 1.5;

class CrowdScene extends LabScene {
  /// A crowd of [count], animated if [animated].
  CrowdScene({this._count = 10, this._animated = true});

  @override
  String get title => 'Animation: a crowd';
  @override
  String get step => 'stage 9';
  @override
  String get about =>
      'Up to a hundred skinned characters, each on its own clip from its own time. The readout says what the '
      'animation costs a frame (the clips sampled and blended, the joints posed); turned off, each holds where it was, '
      'to compare with. Their meshes bend on the GPU.';

  @override
  Vec3 get eye => Vec3(0, 9, 15);
  @override
  Vec3 get target => Vec3(0, 0, -1);

  /// The characters, once loaded.
  List<GameObject> get people => _people;

  final _people = <GameObject>[];
  int _count;
  bool _animated;
  var _fps = '–';
  _Timing? _timing;
  // The count being made, so a second press waits for the first.
  Future<void>? _making;

  @override
  void build() {
    find('sun')!.get<Light>()!.shadows = true;
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    _timing = world.system(_Timing.new);
    await _make();
  }

  /// How many there are.
  int get count => _count;

  /// Makes [value] of them.
  Future<void> setCount(int value) async {
    _count = value;
    while (_making != null) {
      await _making;
    }
    if (_people.length != _count) await _make();
  }

  Future<void> _make() async {
    final done = _making = _grow();
    await done;
    _making = null;
  }

  Future<void> _grow() async {
    while (_people.length > _count) {
      _people.removeLast().destroy();
    }
    final columns = math.max(1, math.sqrt(_count).ceil());
    for (var i = 0; i < _count; i++) {
      final at = Vec3((i % columns - (columns - 1) / 2) * _gap, 0, (i ~/ columns - (columns - 1) / 2) * _gap);
      if (i < _people.length) {
        _people[i].position = at;
        continue;
      }
      final knight = i.isEven;
      final made = await assets.instantiate(knight ? 'kaykit/knight.glb' : 'kaykit/rogue.glb');
      // Closed while it loaded.
      if (isDestroyed) return;
      final person = add(made)..position = at;
      // The files carry every weapon the pack has: a sword and a shield
      // for the Knights, nothing for the Rogues.
      for (final extra in knight ? _knightExtras : _rogueExtras) {
        person.find(extra)?.active = false;
      }
      _people.add(person);
      _start(person, i);
    }
  }

  // A clip and a time of its own, from the character's place: the same
  // crowd every time.
  void _start(GameObject person, int i) {
    final animator = person.animator!;
    if (!_animated) return;
    final random = math.Random(i);
    final clips = animator.clipNames.toList();
    animator
      ..play(clips[random.nextInt(clips.length)])
      ..time = random.nextDouble() * animator.length;
  }

  /// Whether they move: if not, each holds where it was.
  bool get animated => _animated;
  set animated(bool on) {
    _animated = on;
    for (var i = 0; i < _people.length; i++) {
      on ? _start(_people[i], i) : _people[i].animator!.stop();
    }
  }

  /// The animation's time a frame, in milliseconds: the median of the
  /// last second.
  double get animationMs => _timing?.median ?? 0;

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = '${_people.length} characters   animation ${animationMs.toStringAsFixed(2)} ms   $_fps fps';
  }

  @override
  List<Control> get controls => [
    for (final n in crowdSizes) Button('$n', () => setCount(n)),
    Toggle('animated', () => _animated, (on) => animated = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('crowd', s));
}

const _knightExtras = ['1H_Sword_Offhand', 'Badge_Shield', 'Rectangle_Shield', 'Spike_Shield', '2H_Sword'];
const _rogueExtras = ['Knife_Offhand', '1H_Crossbow', '2H_Crossbow', 'Knife', 'Throwable'];

/// Times the updates and the posing that follows them, each frame: the
/// median of the last 60.
class _Timing extends WorldSystem {
  _Timing(super.world);

  final _clock = Stopwatch()..start();
  final _times = List<int>.filled(60, 0);
  final _sorted = List<int>.filled(60, 0);
  var _start = 0, _next = 0;

  /// In milliseconds.
  double median = 0;

  @override
  void beforeUpdate() {
    super.beforeUpdate();
    _start = _clock.elapsedMicroseconds;
  }

  @override
  void afterUpdate() {
    super.afterUpdate();
    _times[_next++] = _clock.elapsedMicroseconds - _start;
    if (_next == _times.length) {
      _next = 0;
      _sorted
        ..setAll(0, _times)
        ..sort();
      median = _sorted[_sorted.length ~/ 2] / 1000;
    }
  }
}
