// Stage 12 phase 1: particles, the basics. A fountain whose settings are
// the page's sliders and switches, a burst that cleans up after itself,
// ten systems of a thousand for the step's time, and a screen of smoke for
// what drawing over and over costs (docs/stage-12.md §6).

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';
import 'particles_look.dart' show smokePuff;

/// `--dart-define=LAB_SMOKE=true`, with LAB_RATES: the screen of smoke from
/// the start, in place of the ten thousand.
const _startSmoke = bool.fromEnvironment('LAB_SMOKE');

class ParticlesBasicsScene extends LabScene {
  @override
  String get title => 'Particles: the basics';
  @override
  String get step => 'stage 12 phase 1';
  @override
  String get about =>
      'The fountain is a ParticleSystem on its own object, as in Unity: a cone that throws particles up, '
      'gravity that brings them down, and a size and a colour over each one\'s life. Its sliders change it as it '
      'plays. In world space the particles stay where they were made while the fountain moves; in local space they '
      'move with it. A burst makes a puff of sparks on an object of its own, which destroys itself when its last '
      'spark is gone. Ten thousand adds ten systems of a thousand, and the readout shows how long the engine takes '
      'to move them all each frame. A screen of smoke fills the view with 200 big see-through puffs: each pixel '
      'drawn over and over, which is what costs a phone most.';

  late final ParticleSystem _fountain;
  late final GameObject _fountainObject;
  late final Material _water, _sparks;
  final _crowd = <GameObject>[];
  GameObject? _smoke;
  Texture2D? _puff;
  var _moving = false, _bursts = 0;
  // The step's times over the last two seconds, and what they came to.
  final _window = <int>[];
  var _shown = (median: 0, slowest: 0);

  @override
  void build() {
    _water = Material(color: const Color(0xFF8FD3FF), blending: Blending.transparent);
    _sparks = Material(color: const Color(0xFFFFB347), blending: Blending.additive);
    _fountain = ParticleSystem(
      startSpeed: const Range(5, 6),
      startSize: const Range(0.12, 0.25),
      startLifetime: const Range(1.2, 1.6),
      gravityModifier: 1,
      simulationSpace: ParticleSpace.world,
      emission: const Emission(rateOverTime: 120),
      shape: ParticleShape.cone(angle: 12.deg, radius: 0.1),
      sizeOverLifetime: (t) => 1 - 0.6 * t,
      colorOverLifetime: ColorGradient.fadeOut(),
      material: _water,
    );
    _fountainObject = add(
      GameObject(
        name: 'fountain',
        components: [_fountain],
        children: [
          shape('basin', Mesh.cylinder(), const Color(0xFF6A7080), at: Vec3(0, -0.1, 0), size: Vec3(0.6, 0.2, 0.6)),
        ],
      ),
    );
    // Measuring on a device (LAB_SCENE=Particles LAB_RATES=true): the ten
    // thousand from the start, their step printed every two seconds.
    if (printRates && !_startSmoke) _setCrowd(true);
    if (_startSmoke) _setSmoke(true);
  }

  void _burst() {
    final n = _bursts++;
    final at = Vec3(math.cos(n * 2.4) * 2.5, 1.5 + (n % 3) * 0.5, math.sin(n * 2.4) * 1.5 - 1);
    final system = ParticleSystem(
      looping: false,
      duration: 1,
      startSpeed: const Range(2, 5),
      startSize: const Range(0.1, 0.2),
      startLifetime: const Range(0.5, 1),
      gravityModifier: 0.4,
      stopAction: StopAction.destroy,
      emission: Emission(rateOverTime: 0, bursts: [Burst(count: const Range(60, 80))]),
      shape: const ParticleShape.sphere(radius: 0.1),
      colorOverLifetime: ColorGradient([
        (0, const Color(0xFFFFFFFF)),
        (0.3, const Color(0xFFFFC040)),
        (1, Color.rgb(1, 0.2, 0, 0)),
      ]),
      material: _sparks,
    );
    add(
      GameObject(
        name: 'burst $n',
        transform: Transform(position: at),
        components: [system],
      ),
    );
    log.add('burst $n: 60 to 80 sparks; StopAction.destroy takes its object away when they are gone');
  }

  void _setCrowd(bool on) {
    if (!on) {
      for (final object in _crowd) {
        object.destroy();
      }
      _crowd.clear();
      return;
    }
    for (var i = 0; i < 10; i++) {
      final angle = i / 10 * 2 * math.pi;
      _crowd.add(
        add(
          GameObject(
            name: 'crowd $i',
            transform: Transform(position: Vec3(math.cos(angle) * 4, 0, math.sin(angle) * 3 - 1)),
            components: [
              ParticleSystem(
                maxParticles: 1000,
                startLifetime: const Range(2),
                startSpeed: const Range(1.5, 2.5),
                startSize: const Range(0.05, 0.1),
                gravityModifier: 0.2,
                emission: const Emission(rateOverTime: 500),
                prewarm: true,
                sizeOverLifetime: (t) => 1 - t,
                colorOverLifetime: ColorGradient.fadeInOut(const Color(0xFFB0FFB0)),
                material: _sparks,
              ),
            ],
          ),
        ),
      );
    }
    _window.clear();
    log.add('ten systems of 1000 particles, prewarmed so they start full');
  }

  /// 200 puffs of smoke, two to three metres across, filling the view
  /// before the camera: see-through, soft where they meet the floor.
  void _setSmoke(bool on) {
    _smoke?.destroy();
    _smoke = null;
    if (!on) return;
    _smoke = add(
      GameObject(
        name: 'smoke screen',
        transform: Transform(position: Vec3(0, 2, 1)),
        components: [
          ParticleSystem(
            maxParticles: 200,
            startLifetime: const Range(6),
            startSpeed: const Range(0, 0.2),
            startSize: const Range(2, 3),
            startRotation: const Range(0, 6.28),
            startColor: const ColorRange(Color(0xFF9AA0A8), Color(0xFFC8CCD2)),
            emission: const Emission(rateOverTime: 200 / 6),
            shape: const ParticleShape.box(size: Vec3(10, 4, 4)),
            prewarm: true,
            colorOverLifetime: ColorGradient.fadeInOut(Color.white),
            softness: 0.5,
            material: Material(baseColorTexture: _puff ??= smokePuff(), blending: Blending.transparent),
          ),
        ],
      ),
    );
    log.add('a screen of smoke: 200 puffs, two to three metres across, over the whole view');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    _puff?.dispose();
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_moving) {
      final t = time.sinceStart;
      _fountainObject.transform = _fountainObject.transform.copyWith(position: Vec3(math.sin(t) * 2, 0, 0));
    }
    _window.add(world.particleStepTime.inMicroseconds);
    if (_window.length == 120) {
      _window.sort();
      _shown = (median: _window[60], slowest: _window.last);
      _window.clear();
      if (printRates) {
        // ignore: avoid_print
        print(
          _smoke != null
              ? '[particles] the screen of smoke: ${_smoke!.get<ParticleSystem>()!.particleCount} puffs'
              : '[particles] ${_crowd.length * 1000} in the ten: step median ${_ms(_shown.median)}, '
                    'slowest ${_ms(_shown.slowest)}',
        );
      }
    }
    var crowd = 0;
    for (final object in _crowd) {
      crowd += object.get<ParticleSystem>()!.particleCount;
    }
    readout.value = [
      'the fountain: ${_fountain.particleCount} particles'
          '${_fountain.isPaused
              ? ', paused'
              : _fountain.isPlaying
              ? ''
              : ', stopped'}',
      if (_crowd.isNotEmpty) 'the ten: $crowd particles',
      if (_smoke case final smoke?) 'the screen of smoke: ${smoke.get<ParticleSystem>()!.particleCount} puffs',
      'the step, over two seconds: ${_ms(_shown.median)} a frame, at most ${_ms(_shown.slowest)}',
    ].join('\n');
  }

  String _ms(int micros) => '${(micros / 1000).toStringAsFixed(2)} ms';

  @override
  List<Control> get controls => [
    Button('burst', _burst),
    Toggle('fountain playing', () => _fountain.isPlaying && !_fountain.isPaused, (on) {
      if (on) {
        _fountain.play();
      } else {
        _fountain.stop();
      }
      log.add(on ? 'fountain: play()' : 'fountain: stop(), and what\'s in the air falls');
    }),
    Toggle('fountain paused', () => _fountain.isPaused, (on) {
      if (on) {
        _fountain.pause();
      } else {
        _fountain.play();
      }
    }),
    Button('clear', () {
      _fountain.clear();
      log.add('fountain: clear()');
    }),
    Toggle('fountain moves', () => _moving, (on) => _moving = on),
    Toggle('world space', () => _fountain.simulationSpace == ParticleSpace.world, (on) {
      _fountain.simulationSpace = on ? ParticleSpace.world : ParticleSpace.local;
      log.add('simulationSpace = ${_fountain.simulationSpace.name}');
    }),
    Toggle('additive', () => _fountain.material == _sparks, (on) => _fountain.material = on ? _sparks : _water),
    Slider(
      'rate (a second)',
      () => _fountain.emission.rateOverTime,
      (v) => _fountain.emission = Emission(rateOverTime: v),
      max: 600,
    ),
    Slider('speed', () => _fountain.startSpeed.min, (v) => _fountain.startSpeed = Range(v, v * 1.2), max: 12),
    Slider('size', () => _fountain.startSize.max, (v) => _fountain.startSize = Range(v / 2, v), min: 0.02, max: 1),
    Slider('gravity', () => _fountain.gravityModifier, (v) => _fountain.gravityModifier = v, min: -0.5, max: 2),
    Slider(
      'spread (degrees)',
      () => _fountain.shape.angle * 180 / math.pi,
      (v) => _fountain.shape = ParticleShape.cone(angle: v.deg, radius: 0.1),
      max: 60,
    ),
    Toggle('ten thousand', () => _crowd.isNotEmpty, _setCrowd),
    Toggle('screen of smoke', () => _smoke != null, _setSmoke),
  ];
}
