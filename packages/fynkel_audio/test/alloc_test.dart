// Frames with sound allocate nothing (docs/stage-11.md): a clip looping,
// one-shots starting and ending, sources reading where they are, slow
// motion and the game's pause going on and off, 3D sources moving
// around a camera that turns, with sounds played at places, and music
// crossfading between streamed tracks while groups' volumes change. Counted by the VM
// service's allocation profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';
import 'package:test/test.dart';

import 'allocations.dart';
import 'audio_test.dart' show wav;

/// Plays a short one-shot every few frames, reads its source's place each
/// frame, and turns slow motion and the game's pause on and off.
class _Player extends Behaviour {
  _Player(this.loop, this.shot, this.blip);

  final AudioSource loop, shot;
  final AudioClip blip;
  var frame = 0;
  var heard = 0;
  var ended = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    frame++;
    if (frame % 3 == 0) shot.playOneShot(blip, pitch: 1.5);
    if (frame % 7 == 0) audio.playOneShot(blip, volume: 0.5);
    if (frame % 50 == 0) shot.play();
    if (loop.isPlaying && loop.playTime > 0) heard++;
    // Each a constant: a double chosen by a condition, passed to a setter
    // the JIT doesn't inline, is boxed in the test itself.
    if (frame % 2 == 0) {
      loop.volume = 0.5;
    } else {
      loop.volume = 0.75;
    }
    if (frame % 40 < 20) {
      world.time.scale = 1;
    } else {
      world.time.scale = 0.5;
    }
    // Objects still get their updates while the game is paused.
    world.paused = frame % 90 >= 80;
  }
}

/// Flies its object around in a circle, turns the camera, and plays a
/// sound at a place now and then: 3D sound's frames.
class _Flyer extends Behaviour {
  _Flyer(this.camera, this.blip);

  final GameObject camera;
  final AudioClip blip;
  final at = Vec3(2, 0, -3);
  var frame = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    frame++;
    // Set from a few constant transforms, by the frame: making one with a
    // computed position each frame would be the test's own garbage.
    switch (frame % 4) {
      case 0:
        transform = _places[0];
        camera.transform = _turns[0];
      case 1:
        transform = _places[1];
      case 2:
        transform = _places[2];
        camera.transform = _turns[1];
      default:
        transform = _places[3];
    }
    if (frame % 11 == 0) audio.playAt(blip, at, volume: 0.5);
  }
}

/// Crossfades the music between two streamed tracks now and then, and
/// moves the groups' volumes as a settings screen's sliders would.
class _Mixer extends Behaviour {
  _Mixer(this.tracks);

  final List<AudioClip> tracks;
  var frame = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    frame++;
    final music = audio.music;
    if (frame % 120 == 0) music.crossFade(tracks[(frame ~/ 120) % 2], 0.5);
    if (frame % 500 == 0) music.stop(fadeOut: 0.25);
    if (frame % 500 == 20) music.play(tracks[0], fadeIn: 0.25);
    final effects = audio.group('effects');
    if (frame % 2 == 0) {
      effects.volume = 0.5;
    } else {
      effects.volume = 0.75;
    }
    if (frame % 300 == 0) audio.group('music').fade(0.3, 1);
    if (frame % 300 == 150) audio.group('music').mute = frame % 600 == 150;
  }
}

final _places = [
  Transform(position: Vec3(-5, 0, -2)),
  Transform(position: Vec3(0, 1, -6)),
  Transform(position: Vec3(5, 0, -2)),
  Transform(position: Vec3(0, -1, 3)),
];
final _turns = [Transform(rotation: Quat.axisAngle(Vec3.up, 0.4)), Transform(rotation: Quat.axisAngle(Vec3.up, -0.4))];

void main() {
  final madeEachFrame = allocations();

  test('frames with sounds playing, starting and ending allocate nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final music = AudioClip.fromSamples(Float32List(48000)..fillRange(0, 48000, 0.1), channels: 2);
    final blip = AudioClip.fromSamples(Float32List(2400)..fillRange(0, 2400, 0.2));
    final loop = AudioSource(clip: music, loop: true, followTimeScale: true);
    final shot = AudioSource(clip: blip, playOnAwake: false);
    final player = _Player(loop, shot, blip);
    var ends = 0;
    shot.onEnd = (_) => ends++;
    world.add(GameObject(components: [loop, shot, player]));
    final camera = world.add(GameObject(name: 'camera', components: [Camera()]));
    final hum = AudioSource(clip: music, loop: true, spatialBlend: 1, maxDistance: 20);
    final buzz = AudioSource(clip: blip, loop: true, spatialBlend: 0.5, rolloffMode: AudioRolloffMode.linear);
    world.add(GameObject(name: 'flyer', components: [hum, buzz, _Flyer(camera, blip)]));
    shot.group = world.audio.group('effects');
    final tracks = [
      for (final value in [0.1, 0.2])
        await AudioClip.decode(wav(List.filled(96000, value)), name: 'track.wav', stream: true),
    ];
    world.add(GameObject(name: 'mixer', components: [_Mixer(tracks)]));

    expect(await madeEachFrame(world, 20000), isEmpty);
    expect(player.frame, greaterThan(50000));
    expect(player.heard, greaterThan(0));
    expect(ends, greaterThan(500));
    expect(world.audio.stats.refused, 0);
  });
}
