// Stage 11 phase 2: sound in 3D. A bee buzzes in circles around you (the
// camera hears), a radio plays a tune you can walk to, and booms go off
// at places. Every sound is made here in code, so the scene needs no
// files. Best with headphones: left and right are the point.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

import '../lab.dart';

/// A buzz: a sawtooth of 8 harmonics at 160 Hz, wobbling 25 times a
/// second, a second long, so it loops without a seam.
AudioClip buzz() {
  const rate = 48000;
  final samples = Float32List(rate);
  for (var i = 0; i < rate; i++) {
    final t = i / rate;
    var saw = 0.0;
    for (var h = 1; h <= 8; h++) {
      saw += math.sin(2 * math.pi * 160 * h * t) / h;
    }
    samples[i] = 0.2 * saw * (0.75 + 0.25 * math.sin(2 * math.pi * 25 * t));
  }
  return AudioClip.fromSamples(samples, name: 'buzz');
}

/// A tune: eight plucked notes, a quarter of a second each, each dying
/// away before the next, so it loops without a seam.
AudioClip tune() {
  const rate = 48000, note = rate ~/ 4;
  const hertz = [523.3, 659.3, 784.0, 1046.5, 784.0, 659.3, 587.3, 698.5];
  final samples = Float32List(note * hertz.length);
  for (var n = 0; n < hertz.length; n++) {
    for (var i = 0; i < note; i++) {
      final t = i / rate;
      final fade = math.min(1.0, (note - i) / 96);
      final pluck = math.sin(2 * math.pi * hertz[n] * t) + 0.4 * math.sin(4 * math.pi * hertz[n] * t);
      samples[n * note + i] = 0.3 * math.min(1.0, t / 0.003) * math.exp(-9 * t) * pluck * fade;
    }
  }
  return AudioClip.fromSamples(samples, name: 'tune');
}

/// A boom: noise through a falling low pass, dying away over most of a
/// second.
AudioClip boom() {
  const rate = 48000;
  final random = math.Random(3);
  final samples = Float32List(rate * 4 ~/ 5);
  var low = 0.0;
  for (var i = 0; i < samples.length; i++) {
    final t = i / rate;
    final a = 0.02 + 0.3 * math.exp(-6 * t);
    low += (random.nextDouble() * 2 - 1 - low) * a;
    samples[i] = 1.6 * low * math.exp(-4 * t) * math.min(1.0, t / 0.002);
  }
  return AudioClip.fromSamples(samples, name: 'boom');
}

/// Flies its object in a circle of [radius] around [around], [below] it.
class _Circle extends Behaviour {
  _Circle(this.around, {this.radius = 3, this.below = 1.2, this.speed = 0.6, this.angle = 0});

  final GameObject around;
  final double radius, below;
  double speed;
  double angle;

  @override
  void onUpdate() {
    super.onUpdate();
    angle += speed * time.delta;
    final centre = around.worldTransform.position;
    // Below the eye, so it's in sight when it's in front.
    transform = Transform(
      position: centre + Vec3(radius * math.sin(angle), -below, -radius * math.cos(angle)),
      scale: transform.scale,
    );
  }
}

class Sound3dScene extends LabScene {
  @override
  String get title => 'Sound: in 3D';
  @override
  String get step => 'stage 11 phase 2';
  @override
  String get about =>
      'Best with headphones. The camera hears. The bee buzzes in circles around it: left, behind (quieter and duller), '
      'right, in front. The radio plays a tune: walk to it and it gets louder, turn and it moves from ear to ear. '
      'A boom plays at a place (audio.playAt) and stays there. Draw the sources to see each one\'s min and max '
      'distance; set the radio\'s spatial blend to 0 to hear it 2D, the same wherever it is.';

  @override
  Vec3 get eye => Vec3(0, 1.6, 6);
  @override
  Vec3 get target => Vec3(0, 1.2, 0);

  final _clips = <AudioClip>[];
  late final GameObject _camera;
  late final AudioSource _bee, _radio;
  late final _Circle _flight;
  late final AudioClip _boom, _buzz;
  final _swarm = <GameObject>[];
  var _heading = 0.0;

  AudioClip _keep(AudioClip clip) {
    _clips.add(clip);
    return clip;
  }

  @override
  void build() {
    _camera = find('camera')!;
    _heading = 0;
    _look();
    _buzz = _keep(buzz());
    _bee = AudioSource(clip: _buzz, loop: true, spatialBlend: 1, volume: 0.7, maxDistance: 30);
    _flight = _Circle(_camera);
    add(shape('bee', Mesh.sphere(), const Color(0xFFF2C230), size: Vec3.all(0.3), components: [_bee, _flight]));
    _radio = AudioSource(clip: _keep(tune()), loop: true, spatialBlend: 1, minDistance: 1, maxDistance: 40);
    add(
      shape(
        'radio',
        Mesh.cube(),
        const Color(0xFFE0613A),
        at: Vec3(0, 0.4, -8),
        size: Vec3(1, 0.8, 0.5),
        components: [_radio],
      ),
    );
    _boom = _keep(boom());
  }

  /// Puts the camera where it is, facing [_heading] (0 is -Z), level.
  void _look({Vec3? at}) {
    final position = at ?? _camera.transform.position;
    _camera.transform = Transform(position: position, rotation: Quat.axisAngle(Vec3.up, _heading));
  }

  void _walk(double metres) {
    final forward = Vec3(-math.sin(_heading), 0, -math.cos(_heading));
    _look(at: _camera.transform.position + forward * metres);
  }

  void _turn(double degrees) {
    _heading += degrees * math.pi / 180;
    _look();
  }

  /// 31 more bees, so 32 3D voices move each frame: what the swarm costs
  /// is in the readout.
  void _swarmOn(bool on) {
    for (final bee in _swarm) {
      bee.destroy();
    }
    _swarm.clear();
    if (!on) return;
    final random = math.Random(5);
    for (var i = 0; i < Audio.maxVoices - 1; i++) {
      final source = AudioSource(
        clip: _buzz,
        loop: true,
        spatialBlend: 1,
        volume: 0.15,
        pitch: 0.7 + random.nextDouble() * 0.8,
        maxDistance: 30,
      );
      final flight = _Circle(
        _camera,
        radius: 2 + random.nextDouble() * 6,
        below: 0.5 + random.nextDouble() * 1.5,
        speed: (random.nextBool() ? 1 : -1) * (0.3 + random.nextDouble() * 1.5),
        angle: random.nextDouble() * 2 * math.pi,
      );
      _swarm.add(
        add(
          shape('bee $i', Mesh.sphere(), const Color(0xFFD9A520), size: Vec3.all(0.15), components: [source, flight]),
        ),
      );
    }
    log.add('a swarm: ${_swarm.length + 1} bees, each a 3D AudioSource');
  }

  void _boomAt(Vec3 offset, String where) {
    final forward = Vec3(-math.sin(_heading), 0, -math.cos(_heading));
    final right = Vec3(math.cos(_heading), 0, -math.sin(_heading));
    final at = _camera.transform.position + right * offset.x + forward * -offset.z + Vec3(0, offset.y, 0);
    world.audio.playAt(_boom, at, maxDistance: 60);
    log.add('audio.playAt(boom, $where)');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final toRadio = (_radio.gameObject.worldTransform.position - _camera.worldTransform.position).length;
    final stats = world.audio.stats;
    readout.value = [
      stats.device
          ? 'playing through the device at ${stats.rate} Hz'
          : 'no device: ${world.audio.outputError ?? 'not on screen'}',
      'the radio is ${toRadio.toStringAsFixed(1)} m away; you face ${(_heading * 180 / math.pi).round() % 360}°',
      'the radio\'s spatial blend ${_radio.spatialBlend.toStringAsFixed(2)}, ${_radio.rolloffMode.name}',
      '${stats.voices} of ${Audio.maxVoices} voices; sound costs the frame ${stats.frameMilliseconds.toStringAsFixed(3)} ms',
    ].join('\n');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    for (final clip in _clips) {
      clip.dispose();
    }
  }

  @override
  List<Control> get controls => [
    Button('step forward', () => _walk(1)),
    Button('step back', () => _walk(-1)),
    Button('turn left', () => _turn(30)),
    Button('turn right', () => _turn(-30)),
    Button('boom on the left', () => _boomAt(Vec3(-4, 0, 0), 'on the left')),
    Button('boom on the right', () => _boomAt(Vec3(4, 0, 0), 'on the right')),
    Button('boom far away', () => _boomAt(Vec3(0, 0, -40), 'far ahead')),
    Toggle('bee', () => _bee.isPlaying, (on) {
      if (on) {
        _bee.play();
      } else {
        _bee.stop();
      }
    }),
    Toggle('a swarm of 32 bees', () => _swarm.isNotEmpty, _swarmOn),
    Slider('bee speed', () => _flight.speed, (value) => _flight.speed = value, min: 0, max: 4),
    Toggle('radio', () => _radio.isPlaying, (on) {
      if (on) {
        _radio.play();
      } else {
        _radio.stop();
      }
    }),
    Slider('radio spatial blend', () => _radio.spatialBlend, (value) => _radio.spatialBlend = value),
    Toggle('radio fades in a line', () => _radio.rolloffMode == AudioRolloffMode.linear, (on) {
      _radio.rolloffMode = on ? AudioRolloffMode.linear : AudioRolloffMode.logarithmic;
      log.add('radio.rolloffMode = ${_radio.rolloffMode.name}');
    }),
    Toggle('draw the sources', () => world.audio.drawSources, (on) => world.audio.drawSources = on),
    Button('back to the start', () {
      _heading = 0;
      _look(at: eye);
    }),
  ];
}
