// Frames with sound allocate nothing (docs/stage-11.md): a clip looping,
// one-shots starting and ending, sources reading where they are, slow
// motion and the game's pause going on and off. Counted by the VM
// service's allocation profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';
import 'package:test/test.dart';

import 'allocations.dart';

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

    expect(await madeEachFrame(world, 20000), isEmpty);
    expect(player.frame, greaterThan(50000));
    expect(player.heard, greaterThan(0));
    expect(ends, greaterThan(500));
    expect(world.audio.stats.refused, 0);
  });
}
