// 'All of it together' (stage 14, phase 3): every system at once in one
// world, drawn, at each quality level. Each part does its work alongside
// the others (the crowd posed, boxes landing with a boom, sparks hitting
// the floor, sounds playing, the road swept again), nothing throws, and
// each part turns off and on again.

import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';
import 'package:lab/scenes/all_together.dart';

Future<Uint8List> files(String asset) async => File('assets/$asset').readAsBytesSync();

void main() {
  for (final quality in Quality.levels) {
    test('every system at once, drawn at ${quality.name}', () async {
      final world = World(source: files);
      addTearDown(world.dispose);
      final scene = world.add(AllTogetherScene(quality: quality.name));
      await scene.loaded;
      final view = HeadlessView(world, width: 160, height: 90);
      addTearDown(view.dispose);
      var pixels = Uint8List(0);
      void frames(int n) {
        for (var i = 0; i < n; i++) {
          world.update(1 / 60);
          // Each one drawn, not replaced by the next before it was.
          if (i % 4 == 0) pixels = view.read(view.frame()).rgba;
        }
      }

      expect(scene.people, hasLength(12));
      final wrist = scene.people.first.find('wrist.r') ?? scene.people.first.children.first;
      final before = wrist.worldTransform.position;
      frames(240);

      expect(world.quality, quality);
      expect(wrist.worldTransform.position, isNot(before), reason: 'the crowd is posed');
      expect(scene.landed, greaterThan(0), reason: 'boxes landed, each with a boom');
      expect(scene.sparkHits, greaterThan(0), reason: 'sparks hit the floor');
      expect(world.audio.stats.voices, greaterThanOrEqualTo(8), reason: 'the bees buzz');
      expect(scene.road.lastSweep, greaterThan(0), reason: 'the road was swept again');
      expect(view.stats.rendered, 60);
      expect(_colours(pixels), greaterThan(50), reason: 'a scene was drawn, not one colour');

      for (final part in allParts) {
        scene.turn(part, false);
        frames(20);
        scene.turn(part, true);
        frames(20);
      }
      await pumpEventQueue();
      expect(scene.log.value.where((l) => l.error).map((l) => l.text), isEmpty);
      scene.destroy();
      world.update(1 / 60);
      view.read(view.frame());
    });
  }
}

/// How many different colours [rgba] has.
int _colours(Uint8List rgba) => {for (var i = 0; i < rgba.length; i += 4) rgba.buffer.asUint32List()[i ~/ 4]}.length;
