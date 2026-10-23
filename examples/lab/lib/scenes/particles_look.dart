// Stage 12 phase 2: particles, the look. Fire, smoke, sparks, rain and a
// magic swirl, each a ParticleSystem or two using what phase 2 brought:
// texture sheets, stretched and flat particles, velocity, drag, force,
// noise, colour by speed, soft and lit particles, and a material's own
// shader. Every texture is made here in code, so the scene needs no files.
// Each effect is seeded, so it looks the same every time: its golden is in
// test/particles_look_test.dart.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// A square texture, [size] pixels across, white, its alpha [alpha] of each
/// pixel's centre: x and y from 0 to 1, from the top left.
Texture2D _alphaTexture(int size, double Function(double x, double y) alpha) {
  final pixels = Uint8List(size * size * 4);
  for (var y = 0; y < size; y++) {
    for (var x = 0; x < size; x++) {
      final i = (y * size + x) * 4;
      pixels
        ..[i] = 255
        ..[i + 1] = 255
        ..[i + 2] = 255
        ..[i + 3] = (alpha((x + 0.5) / size, (y + 0.5) / size).clamp(0.0, 1.0) * 255).round();
    }
  }
  return Texture2D.rgba(size, size, pixels);
}

double _from(double x, double y) => math.sqrt((x - 0.5) * (x - 0.5) + (y - 0.5) * (y - 0.5)) * 2;

/// A flame's sheet: 4 × 4 frames, 64 pixels each, a flame that flickers and
/// burns down from the first to the last.
Texture2D flameSheet() => _alphaTexture(256, (x, y) {
  final frame = (y * 4).floor() * 4 + (x * 4).floor();
  final u = (x * 4) % 1, v = (y * 4) % 1;
  // Narrower towards the top, swaying a little more each frame.
  final up = 1 - v;
  final sway = math.sin(up * 5 + frame * 1.3) * 0.08 * up;
  // A teardrop: widest a third of the way up, to a point at the top.
  final width = (0.4 - frame * 0.012) * math.pow(math.sin(math.pi * math.pow(math.min(up * 1.05, 1), 0.6)), 0.8);
  final across = (u - 0.5 - sway).abs() / math.max(width, 1e-3);
  return math.pow(math.max(0, 1 - across), 0.8).toDouble();
});

/// A puff of smoke: soft, round, and lumpy.
Texture2D smokePuff() => _alphaTexture(64, (x, y) {
  final lumps = 0.75 + 0.25 * math.sin(x * 17 + 1) * math.sin(y * 13 + 2);
  return math.pow(math.max(0, 1 - _from(x, y)), 1.5) * lumps;
});

/// A soft dot, for sparks and rain.
Texture2D softDot() => _alphaTexture(32, (x, y) => math.pow(math.max(0, 1 - _from(x, y)), 2).toDouble());

/// A ring, for rain's splashes.
Texture2D ring() => _alphaTexture(64, (x, y) {
  final d = (_from(x, y) - 0.8) / 0.1;
  return math.exp(-d * d);
});

/// The swirl's shader: a glow from its middle out, its hue its own (its
/// random number) moving on as it ages.
const swirlShader = '''
fn unlit(in: SurfaceInput) -> vec4<f32> {
    let glow = pow(saturate(1.0 - length(in.uv - vec2<f32>(0.5)) * 2.0), 2.0);
    let hue = particle_random() * 0.35 + 0.5 + particle_age() * 0.5;
    let rgb = 0.5 + 0.5 * cos(6.2832 * (hue + vec3<f32>(0.0, 0.33, 0.67)));
    return vec4<f32>(rgb * 3.0, glow * in.color.a);
}
''';

/// The five effects, by name, in the order they stand.
const particleEffects = ['fire', 'smoke', 'swirl', 'sparks', 'rain'];

class ParticlesLookScene extends LabScene {
  /// The whole page; with [only], one effect, close up (for its golden).
  ParticlesLookScene({this.only}) : assert(only == null || particleEffects.contains(only));

  /// The one effect it shows, or all of them.
  final String? only;

  @override
  String get title => 'Particles: the look';
  @override
  String get step => 'stage 12 phase 2';
  @override
  String get about =>
      'Five effects, each a few lines of a ParticleSystem. '
      'FIRE: an additive flame from a 4 × 4 TextureSheet, a ForceOverLifetime up, a little ParticleNoise, '
      'and a ColorGradient from white to red. '
      'SMOKE: lit puffs that rise (VelocityOverLifetime), grow, turn (rotationOverLifetime) and drift in the noise; '
      'soft, so they fade where they meet the floor instead of cutting a line. '
      'SWIRL: a material with its own shader, coloured by particle_random() and particle_age(); '
      'VelocityOverLifetime curves take each one round as it rises. '
      'SPARKS: ParticleRenderMode.stretched, slowed by LimitVelocity\'s drag, white when fast and red when slow '
      '(ColorBySpeed), smaller as they slow (SizeBySpeed). '
      'RAIN: stretched drops falling fast, and splashes lying flat on the floor (ParticleRenderMode.horizontal). '
      'The switches turn soft and lit particles and the noise off and on.';

  @override
  Vec3 get eye => switch (only) {
    null => Vec3(0, 2.4, 7),
    final name => _at(name) + Vec3(0, 1.2, 2.6),
  };
  @override
  Vec3 get target => switch (only) {
    null => Vec3(0, 1.3, 0),
    final name => _at(name) + Vec3(0, 1.1, 0),
  };

  static Vec3 _at(String effect) => Vec3((particleEffects.indexOf(effect) - 2) * 2.4, 0, 0);

  final _textures = <Texture2D>[];
  Shader? _shader;

  /// Each effect's systems, by its name.
  final systems = <String, List<ParticleSystem>>{};

  // The step's times over the last two seconds, and what they came to.
  final _window = <int>[];
  var _shown = (median: 0, slowest: 0);
  var _soft = true, _noise = true, _fps = '–';

  Texture2D _keep(Texture2D texture) {
    _textures.add(texture);
    return texture;
  }

  @override
  void build() {
    if (_startQuality.isNotEmpty) world.quality = Quality.levels.firstWhere((q) => q.name == _startQuality);
    for (final name in particleEffects) {
      if (only != null && only != name) continue;
      final at = _at(name);
      final made = switch (name) {
        'fire' => _fire(),
        'smoke' => _smoke(),
        'swirl' => _swirl(),
        'sparks' => _sparks(),
        _ => _rain(),
      };
      systems[name] = [for (final (_, system) in made) system];
      for (final (offset, system) in made) {
        add(
          GameObject(
            name: name,
            transform: Transform(position: at + offset),
            components: [system],
          ),
        );
      }
      if (name != 'rain') {
        add(
          shape(
            '$name base',
            Mesh.cylinder(),
            const Color(0xFF2A2D33),
            at: at + Vec3(0, 0.1, 0),
            size: Vec3(0.5, 0.2, 0.5),
          ),
        );
      }
    }
  }

  List<(Vec3, ParticleSystem)> _fire() => [
    (
      Vec3(0, 0.45, 0),
      ParticleSystem(
        randomSeed: 1,
        startLifetime: const Range(0.7, 1),
        startSpeed: const Range(0.3, 0.6),
        startSize: const Range(0.8, 1.2),
        startRotation: const Range(-0.3, 0.3),
        emission: const Emission(rateOverTime: 40),
        shape: ParticleShape.cone(angle: 10.deg, radius: 0.15),
        forceOverLifetime: const ParticleVector(y: ParticleCurve(2)),
        noise: const ParticleNoise(strength: ParticleCurve(0.4), frequency: 1.5, scrollSpeed: 1),
        sizeOverLifetime: (t) => t < 0.2 ? 0.5 + 2.5 * t : 1 - 0.7 * (t - 0.2) / 0.8,
        colorOverLifetime: ColorGradient([
          (0, Color.rgb(1, 1, 0.8, 0)),
          (0.1, const Color(0xFFFFE9A0)),
          (0.4, const Color(0xFFFF8A20)),
          (1, Color.rgb(0.6, 0.05, 0, 0)),
        ]),
        textureSheet: TextureSheet(columns: 4, rows: 4),
        softness: 0.3,
        material: Material(baseColorTexture: _keep(flameSheet()), blending: Blending.additive),
      ),
    ),
  ];

  List<(Vec3, ParticleSystem)> _smoke() => [
    (
      Vec3(0, 0.2, 0),
      ParticleSystem(
        randomSeed: 2,
        startLifetime: const Range(3, 4),
        startSpeed: const Range(0.1, 0.3),
        startSize: const Range(0.4, 0.6),
        startRotation: const Range(0, 6.28),
        startColor: const ColorRange(Color(0xFF8A8F99), Color(0xFFC0C4CC)),
        simulationSpace: ParticleSpace.world,
        emission: const Emission(rateOverTime: 12),
        shape: const ParticleShape.sphere(radius: 0.2),
        velocityOverLifetime: const ParticleVector(y: ParticleCurve.between(0.5, 0.8)),
        rotationOverLifetime: const ParticleCurve.between(-0.6, 0.6),
        noise: const ParticleNoise(strength: ParticleCurve(0.3), frequency: 0.8, scrollSpeed: 0.3),
        sizeOverLifetime: (t) => 1 + 2.5 * t,
        colorOverLifetime: ColorGradient.fadeInOut(Color.white),
        lit: true,
        softness: 0.6,
        material: Material(baseColorTexture: _keep(smokePuff()), blending: Blending.transparent, roughness: 1),
      ),
    ),
  ];

  List<(Vec3, ParticleSystem)> _swirl() {
    final shader = _shader ??= Shader(swirlShader, name: 'swirl.wgsl');
    // Round and round, twice in a life, as it rises.
    double round(double t, double phase) => math.sin((t * 2 + phase) * 2 * math.pi) * 1.6;
    return [
      (
        Vec3(0, 0.3, 0),
        ParticleSystem(
          randomSeed: 3,
          startLifetime: const Range(2.5),
          startSpeed: const Range(0),
          startSize: const Range(0.15, 0.3),
          emission: const Emission(rateOverTime: 60),
          shape: const ParticleShape.sphere(radius: 0.1),
          velocityOverLifetime: ParticleVector(
            x: ParticleCurve.curve((t) => round(t, 0.25)),
            y: const ParticleCurve.between(0.6, 1),
            z: ParticleCurve.curve((t) => round(t, 0)),
          ),
          noise: const ParticleNoise(strength: ParticleCurve(0.2), frequency: 2),
          sizeOverLifetime: (t) => math.sin(t * math.pi),
          colorOverLifetime: ColorGradient.fadeInOut(Color.white),
          material: Material(shader: shader, blending: Blending.additive),
        ),
      ),
    ];
  }

  List<(Vec3, ParticleSystem)> _sparks() {
    final dot = _keep(softDot());
    return [
      (
        Vec3(0, 0.25, 0),
        ParticleSystem(
          randomSeed: 4,
          startLifetime: const Range(0.8, 1.4),
          startSpeed: const Range(4, 7),
          startSize: const Range(0.04, 0.07),
          gravityModifier: 0.8,
          emission: const Emission(rateOverTime: 80),
          shape: ParticleShape.cone(angle: 30.deg, radius: 0.05),
          limitVelocity: const LimitVelocity(limit: ParticleCurve(100), drag: ParticleCurve(1.5)),
          colorBySpeed: ColorBySpeed(
            ColorGradient([(0, const Color(0xFFB02000)), (0.5, const Color(0xFFFF9020)), (1, const Color(0xFFFFF4D0))]),
            maxSpeed: 5,
          ),
          sizeBySpeed: SizeBySpeed((t) => 0.5 + 0.5 * t, maxSpeed: 5),
          colorOverLifetime: ColorGradient.fadeOut(),
          renderMode: const ParticleRenderMode.stretched(lengthScale: 1, speedScale: 0.06),
          material: Material(baseColorTexture: dot, blending: Blending.additive),
        ),
      ),
    ];
  }

  List<(Vec3, ParticleSystem)> _rain() {
    final dot = _keep(softDot());
    const area = Vec3(1.8, 0, 1.8);
    return [
      (
        Vec3(0, 4, 0),
        ParticleSystem(
          randomSeed: 5,
          startLifetime: const Range(0.42),
          startSpeed: const Range(0),
          startSize: const Range(0.03),
          startColor: Color.rgb(0.7, 0.8, 1, 0.6),
          gravityModifier: 2,
          emission: const Emission(rateOverTime: 150),
          shape: const ParticleShape.box(size: area),
          velocityOverLifetime: const ParticleVector(y: ParticleCurve(-8)),
          renderMode: const ParticleRenderMode.stretched(lengthScale: 1, speedScale: 0.04),
          material: Material(baseColorTexture: dot, blending: Blending.transparent),
        ),
      ),
      (
        Vec3(0, 0.02, 0),
        ParticleSystem(
          randomSeed: 6,
          startLifetime: const Range(0.4),
          startSpeed: const Range(0),
          startSize: const Range(0.25, 0.4),
          startColor: Color.rgb(0.8, 0.9, 1, 0.8),
          emission: const Emission(rateOverTime: 40),
          shape: const ParticleShape.box(size: area),
          sizeOverLifetime: (t) => 0.2 + 0.8 * t,
          colorOverLifetime: ColorGradient.fadeOut(),
          renderMode: ParticleRenderMode.horizontal,
          material: Material(baseColorTexture: _keep(ring()), blending: Blending.transparent),
        ),
      ),
    ];
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _window.add(world.particleStepTime.inMicroseconds);
    if (_window.length == 120) {
      _window.sort();
      _shown = (median: _window[60], slowest: _window.last);
      _window.clear();
      if (printRates) {
        // ignore: avoid_print
        print('[particles] the look: step median ${_ms(_shown.median)}, slowest ${_ms(_shown.slowest)}');
      }
    }
    readout.value = [
      [
        for (final MapEntry(key: name, value: list) in systems.entries)
          '$name ${list.fold(0, (n, s) => n + s.particleCount)}',
      ].join(', '),
      'the step, over two seconds: ${_ms(_shown.median)} a frame, at most ${_ms(_shown.slowest)}   $_fps fps',
    ].join('\n');
  }

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('particles', s));

  String _ms(int micros) => '${(micros / 1000).toStringAsFixed(2)} ms';

  @override
  void onDestroy() {
    super.onDestroy();
    // Textures made in code are the scene's to free.
    for (final texture in _textures) {
      texture.dispose();
    }
  }

  @override
  List<Control> get controls => [
    for (final name in systems.keys)
      Toggle(name, () => systems[name]!.first.isEmitting, (on) {
        for (final system in systems[name]!) {
          on ? system.play() : system.stop();
        }
      }),
    Toggle('soft particles', () => _soft, (on) {
      _soft = on;
      systems['fire']?.first.softness = on ? 0.3 : 0;
      systems['smoke']?.first.softness = on ? 0.6 : 0;
      log.add(on ? 'softness on: they fade where they meet the floor' : 'softness 0: a hard line at the floor');
    }),
    if (systems['smoke'] case final smoke?)
      Toggle('lit smoke', () => smoke.first.lit, (on) {
        smoke.first.lit = on;
        log.add(on ? 'smoke lit by the sun' : 'smoke unlit, as it is');
      }),
    Toggle('noise', () => _noise, (on) {
      _noise = on;
      systems['fire']?.first.noise = on
          ? const ParticleNoise(strength: ParticleCurve(0.4), frequency: 1.5, scrollSpeed: 1)
          : null;
      systems['smoke']?.first.noise = on
          ? const ParticleNoise(strength: ParticleCurve(0.3), frequency: 0.8, scrollSpeed: 0.3)
          : null;
      systems['swirl']?.first.noise = on ? const ParticleNoise(strength: ParticleCurve(0.2), frequency: 2) : null;
    }),
  ];
}
