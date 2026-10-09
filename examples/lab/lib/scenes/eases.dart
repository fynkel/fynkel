// Step 6: motion, the eases side by side. Each cube goes left to right and
// back for ever with its own ease, all over the same seconds.

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class EasesScene extends LabScene {
  @override
  String get title => 'Motion: eases';
  @override
  String get step => 'step 6';
  @override
  String get about =>
      'Each cube goes from left to right and back for ever: moveTo(..., ease: ..., loops: -1, yoyo: true), all '
      'over the same seconds, so only the ease differs. Top to bottom: linear, sine, quad, cubic, quart, quint, '
      'expo, circ, back, elastic, bounce. Switch between their in, out and in-out kinds, change the seconds, slow '
      'the time scale, or complete them all (one that loops for ever ends at the end of its play).';

  @override
  Vec3 get eye => Vec3(0, 9, 6);
  @override
  Vec3 get target => Vec3(0, 0, 0.4);

  static const _families = [
    'linear',
    'Sine',
    'Quad',
    'Cubic',
    'Quart',
    'Quint',
    'Expo',
    'Circ',
    'Back',
    'Elastic',
    'Bounce',
  ];

  static const _eases = <String, EaseFunction>{
    'inSine': Ease.inSine,
    'outSine': Ease.outSine,
    'inOutSine': Ease.inOutSine,
    'inQuad': Ease.inQuad,
    'outQuad': Ease.outQuad,
    'inOutQuad': Ease.inOutQuad,
    'inCubic': Ease.inCubic,
    'outCubic': Ease.outCubic,
    'inOutCubic': Ease.inOutCubic,
    'inQuart': Ease.inQuart,
    'outQuart': Ease.outQuart,
    'inOutQuart': Ease.inOutQuart,
    'inQuint': Ease.inQuint,
    'outQuint': Ease.outQuint,
    'inOutQuint': Ease.inOutQuint,
    'inExpo': Ease.inExpo,
    'outExpo': Ease.outExpo,
    'inOutExpo': Ease.inOutExpo,
    'inCirc': Ease.inCirc,
    'outCirc': Ease.outCirc,
    'inOutCirc': Ease.inOutCirc,
    'inBack': Ease.inBack,
    'outBack': Ease.outBack,
    'inOutBack': Ease.inOutBack,
    'inElastic': Ease.inElastic,
    'outElastic': Ease.outElastic,
    'inOutElastic': Ease.inOutElastic,
    'inBounce': Ease.inBounce,
    'outBounce': Ease.outBounce,
    'inOutBounce': Ease.inOutBounce,
  };

  final _cubes = <GameObject>[];
  final _motions = <Motion>[];
  var _kind = 'inOut';
  double _seconds = 1.5;

  static double _z(int lane) => -3.5 + lane * 0.7;

  @override
  void build() {
    for (final (i, family) in _families.indexed) {
      final color = Color.rgb(0.3 + 0.06 * i, 0.55, 0.95 - 0.06 * i);
      _cubes.add(add(shape(family, Mesh.cube(), color, at: Vec3(-3, 0.25, _z(i)), size: Vec3(0.4, 0.4, 0.4))));
    }
    _play();
  }

  /// Starts every cube again from the left, with the eases of [_kind].
  void _play() {
    for (final m in _motions) {
      m.stop();
    }
    _motions.clear();
    for (final (i, cube) in _cubes.indexed) {
      final family = _families[i];
      final ease = family == 'linear' ? Ease.linear : _eases['$_kind$family']!;
      cube.position = Vec3(-3, 0.25, _z(i));
      _motions.add(cube.moveTo(Vec3(3, 0.25, _z(i)), seconds: _seconds, ease: ease, loops: -1, yoyo: true));
    }
    log.add('$_kind eases, ${secs(_seconds)} each way');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final running = _motions.where((m) => m.isRunning).length;
    readout.value = [
      'ease: $_kind…   seconds: ${secs(_seconds)}   time scale: ${time.scale.toStringAsFixed(2)}',
      'running: $running of ${_motions.length}',
      'top to bottom: ${_families.join(', ')}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('in', () {
      _kind = 'in';
      _play();
    }),
    Button('out', () {
      _kind = 'out';
      _play();
    }),
    Button('in-out', () {
      _kind = 'inOut';
      _play();
    }),
    Slider('seconds', () => _seconds, (v) => _seconds = v, min: 0.3, max: 4),
    Button('play again with these seconds', _play),
    Slider('time scale', () => time.scale, (v) => time.scale = v, max: 2),
    Button('complete them all', () {
      for (final m in _motions) {
        m.complete();
      }
      log.add('completed: each ends at the end of the play it was in');
    }),
  ];
}
