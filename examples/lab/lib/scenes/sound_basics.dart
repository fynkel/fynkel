// Stage 11 phase 1: sound, the basics. Three bells, each an AudioSource
// on its own object, a hum that loops, and a click that belongs to no
// object. Every sound is made here in code (AudioClip.fromSamples), so the
// scene needs no files.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

import '../lab.dart';

/// A bell's note: a sine and its octave, struck and dying away.
AudioClip bell(double hertz, {double seconds = 1.2}) {
  final rate = 48000, frames = (rate * seconds).round();
  final samples = Float32List(frames);
  for (var i = 0; i < frames; i++) {
    final t = i / rate;
    final strike = math.min(1.0, t / 0.004);
    final ring = math.sin(2 * math.pi * hertz * t) + 0.3 * math.sin(4 * math.pi * hertz * t);
    samples[i] = 0.35 * strike * math.exp(-4 * t) * ring;
  }
  return AudioClip.fromSamples(samples, rate: rate, name: 'bell ${hertz.round()} Hz');
}

/// A low hum, a second long and a whole number of waves of each of its
/// notes, so it loops without a seam.
AudioClip hum() {
  const rate = 48000;
  final samples = Float32List(rate);
  for (var i = 0; i < rate; i++) {
    final t = i / rate;
    samples[i] = 0.12 * math.sin(2 * math.pi * 110 * t) + 0.06 * math.sin(2 * math.pi * 165 * t);
  }
  return AudioClip.fromSamples(samples, name: 'hum');
}

/// A short click: noise, gone in a hundredth of a second.
AudioClip click() {
  final random = math.Random(7);
  final samples = Float32List(480);
  for (var i = 0; i < samples.length; i++) {
    samples[i] = 0.4 * (random.nextDouble() * 2 - 1) * math.exp(-i / 80);
  }
  return AudioClip.fromSamples(samples, name: 'click');
}

class SoundBasicsScene extends LabScene {
  @override
  String get title => 'Sound: the basics';
  @override
  String get step => 'stage 11 phase 1';
  @override
  String get about =>
      'Each bell is an object with an AudioSource: ring one, or all three at once. Chime plays one-shots over '
      'the cube\'s own bell, a little higher each time. The hum loops; pause the game and it holds where it '
      'is, while the click, which ignores the pause, still plays. In slow motion the hum goes lower and slower, '
      'as it follows the time scale. The readout shows the voices the mixer has.';

  final _clips = <AudioClip>[];
  late final AudioClip _chime, _click;
  late final List<(GameObject, AudioSource)> _bells;
  late final AudioSource _hum;
  var _chimes = 0;

  AudioClip _keep(AudioClip clip) {
    _clips.add(clip);
    return clip;
  }

  @override
  void build() {
    final notes = [
      (261.6, const Color(0xFFE0613A)),
      (329.6, const Color(0xFFE0C23A)),
      (392.0, const Color(0xFF3A8EE0)),
    ];
    _bells = [
      for (final (i, (hertz, colour)) in notes.indexed)
        () {
          final source = AudioSource(clip: _keep(bell(hertz)), playOnAwake: false);
          final object = add(
            shape(
              ['cube', 'ball', 'cylinder'][i],
              [Mesh.cube(), Mesh.sphere(), Mesh.cylinder()][i],
              colour,
              at: Vec3(i * 2.0 - 2, 0.6, 0),
              components: [source],
            ),
          );
          source.onEnd = (clip) => log.add('${object.name}: ${clip.name} ended');
          return (object, source);
        }(),
    ];
    _chime = _keep(bell(523.3, seconds: 0.8));
    _click = _keep(click());
    _hum = AudioSource(clip: _keep(hum()), loop: true, volume: 0.6, followTimeScale: true);
    add(GameObject(name: 'hum', components: [_hum]));
    on(world.onPausedChanged, (paused) => log.add(paused ? 'paused: the hum holds' : 'going on'));
  }

  void _ring(int i) {
    final (object, source) = _bells[i];
    source.play();
    log.add('${object.name}: play()');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    // Each bell swells as it sounds, and settles when it's done.
    for (final (object, source) in _bells) {
      final swell = source.isPlaying ? 0.25 * math.exp(-3 * source.playTime) : 0.0;
      object.transform = object.transform.copyWith(scale: Vec3.one * (1 + swell));
    }
    final stats = world.audio.stats;
    readout.value = [
      stats.device
          ? 'playing through the device at ${stats.rate} Hz'
          : 'no device: ${world.audio.outputError ?? 'not on screen'}',
      '${stats.voices} of ${Audio.maxVoices} voices; ${stats.started} started, ${stats.stolen} gave way',
      'the hum is at ${secs(_hum.playTime)}${_hum.isPlaying ? '' : ' (held)'}',
      if (stats.underruns > 0) '${stats.underruns} underruns',
    ].join('\n');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    // A clip made in code is the scene's to free; one from assets.audio
    // would be freed with the last object that asked for it.
    for (final clip in _clips) {
      clip.dispose();
    }
  }

  @override
  List<Control> get controls => [
    Button('ring the cube', () => _ring(0)),
    Button('ring the ball', () => _ring(1)),
    Button('ring the cylinder', () => _ring(2)),
    Button('ring all three', () {
      for (var i = 0; i < 3; i++) {
        _ring(i);
      }
    }),
    Button('chime (one-shot)', () {
      final pitch = math.pow(2, (_chimes++ % 8) / 12).toDouble();
      _bells[0].$2.playOneShot(_chime, pitch: pitch);
      log.add('cube: playOneShot(chime, pitch: ${pitch.toStringAsFixed(2)})');
    }),
    Button('click (ignores the pause)', () {
      world.audio.playOneShot(_click, ignorePause: true);
      log.add('audio.playOneShot(click, ignorePause: true)');
    }),
    Toggle('hum', () => _hum.isPlaying || _hum.playTime > 0, (on) {
      if (on) {
        _hum.play();
      } else {
        _hum.stop();
      }
      log.add(on ? 'hum: play()' : 'hum: stop()');
    }),
    Toggle('game paused', () => world.paused, (on) => world.paused = on),
    Toggle('slow motion', () => time.scale < 1, (on) {
      time.scale = on ? 0.5 : 1;
      log.add('time.scale = ${time.scale}: the hum follows it, the bells don\'t');
    }),
    Slider('hum pitch', () => _hum.pitch, (value) => _hum.pitch = value, min: 0.5, max: 2),
    Slider('master volume', () => world.audio.volume, (value) => world.audio.volume = value),
  ];
}
