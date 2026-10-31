// Stage 11 phase 3: music and the mix. A game's sound settings: master,
// music and effects sliders over music that crossfades between two
// tracks, streamed from their files, while effects tick in their own
// group. The tracks are ours, made by tool/music.py.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';

import '../lab.dart';

/// `--dart-define=LAB_SOUND=true`: every 30 seconds the scene prints its
/// sound's stats and crossfades to the other track, for a long run on a
/// device (stage 11's "10 minutes without a gap").
const _report = bool.fromEnvironment('LAB_SOUND');

/// A tick: a short click of a high tone, as a footstep or a menu's.
AudioClip tick() {
  const rate = 48000;
  final samples = Float32List(rate ~/ 10);
  for (var i = 0; i < samples.length; i++) {
    final t = i / rate;
    samples[i] = 0.5 * math.sin(2 * math.pi * 1800 * t) * math.exp(-60 * t) * math.min(1.0, t / 0.001);
  }
  return AudioClip.fromSamples(samples, name: 'tick');
}

class SoundMixScene extends LabScene {
  @override
  String get title => 'Sound: music and the mix';
  @override
  String get step => 'stage 11 phase 3';
  @override
  String get about =>
      'A sound settings screen. The music crossfades between a calm track and a chase (each streamed from its '
      'file, decoded a second ahead); a tick plays twice a second in the effects group. Move the master, music '
      'and effects sliders, mute a group, duck the music under a voice, or fade it out and in.';

  @override
  Vec3 get eye => Vec3(0, 2.5, 7);

  AudioClip? _calm, _chase;
  late final AudioClip _tick;
  late final AudioGroup _music, _effects;
  late final GameObject _ticker;
  var _ticking = true, _since = 0.0, _ducked = false, _running = 0.0, _reported = 0.0;

  @override
  void build() {
    _music = world.audio.group('music');
    _effects = world.audio.group('effects');
    _tick = tick();
    _ticker = add(shape('ticker', Mesh.sphere(), const Color(0xFF5BC0EB), at: Vec3(0, 0.6, 0), size: Vec3.all(0.6)));
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    _calm = await assets.audio('music/calm.ogg');
    _chase = await assets.audio('music/chase.ogg');
    world.audio.music.play(_calm!, fadeIn: 2);
    log.add('music.play(calm, fadeIn: 2): ${_calm!.streamed ? 'streamed' : 'decoded'}, ${secs(_calm!.length)}');
  }

  void _crossFade() {
    final (calm, chase) = (_calm, _chase);
    if (calm == null || chase == null) return;
    final next = identical(world.audio.music.clip, calm) ? chase : calm;
    world.audio.music.crossFade(next, 2);
    log.add('music.crossFade(${next.name.split('/').last}, 2)');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (_ticking) {
      _since += time.delta;
      if (_since >= 0.5) {
        _since -= 0.5;
        world.audio.playOneShot(_tick, group: _effects, volume: 0.6);
        // The ball jumps with each tick, so you see what you should hear.
        _ticker.position = Vec3(0, 0.9, 0);
      } else {
        _ticker.position = Vec3(0, 0.6 + 0.3 * math.max(0, 1 - _since * 8), 0);
      }
    }
    final music = world.audio.music;
    final stats = world.audio.stats;
    _running += time.unscaledDelta;
    if (_report && _running - _reported >= 30) {
      _reported = _running;
      print(
        'LAB_SOUND ${_running.round()} s: starved ${stats.starved}, underruns ${stats.underruns}, '
        'device ${stats.device}, voices ${stats.voices}, '
        'music ${music.clip?.name} streamed ${music.clip?.streamed}',
      );
      _crossFade();
    }
    readout.value = [
      stats.device
          ? 'playing through the device at ${stats.rate} Hz'
          : 'no device: ${world.audio.outputError ?? 'not on screen'}',
      'music: ${music.clip?.name.split('/').last ?? 'nothing'} at ${music.playTime.toStringAsFixed(1)} s'
          '${music.clip?.streamed ?? false ? ', streamed' : ''}',
      'master ${world.audio.volume.toStringAsFixed(2)}, $_music, $_effects',
      'starved callbacks: ${stats.starved} (a gap would count here)',
      '${stats.voices} of ${Audio.maxVoices} voices; sound costs the frame ${stats.frameMilliseconds.toStringAsFixed(3)} ms',
    ].join('\n');
  }

  @override
  void onDestroy() {
    super.onDestroy();
    // The groups and the music are the world's: leave them as found.
    world.audio.music.stop();
    _music
      ..volume = 1
      ..mute = false;
    _effects
      ..volume = 1
      ..mute = false;
    world.audio.volume = 1;
    _tick.dispose();
  }

  @override
  List<Control> get controls => [
    Slider('master volume', () => world.audio.volume, (value) => world.audio.volume = value),
    Slider('music', () => _music.volume, (value) => _music.volume = value),
    Slider('effects', () => _effects.volume, (value) => _effects.volume = value),
    Toggle('mute music', () => _music.mute, (on) => _music.mute = on),
    Toggle('mute effects', () => _effects.mute, (on) => _effects.mute = on),
    Button('crossfade to the other track', _crossFade),
    Toggle('duck the music (as under a voice)', () => _ducked, (on) {
      _ducked = on;
      _music.fade(on ? 0.25 : 1, 0.5);
      log.add('music group fades to ${on ? 0.25 : 1} over 0.5 s');
    }),
    Button('music: fade out', () {
      world.audio.music.stop(fadeOut: 2);
      log.add('music.stop(fadeOut: 2)');
    }),
    Button('music: fade in the calm track', () {
      final calm = _calm;
      if (calm == null) return;
      world.audio.music.play(calm, fadeIn: 2);
      log.add('music.play(calm, fadeIn: 2)');
    }),
    Toggle('ticks', () => _ticking, (on) => _ticking = on),
  ];
}
