// Films the front page's scene: frames to the folder FILM names, as PNGs.
//   FILM=/tmp/film flutter test --timeout none      (30 a second)
//   FILM=/tmp/film EVERY=30 flutter test            (a few, to look at)
// Without FILM, it runs the scene to its end and draws it once.

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:showcase/showcase.dart';

void main() {
  test('the front page', () async {
    final out = Platform.environment['FILM'];
    final every = int.parse(Platform.environment['EVERY'] ?? '1');
    final errors = <Object>[];
    final world = World(source: (name) async => File('../../examples/lab/assets/$name').readAsBytesSync());
    addTearDown(world.dispose);
    final scene = world.add(Showcase());
    await scene.loaded;
    final view = HeadlessView(world, width: 1920, height: 1080);
    addTearDown(view.dispose);
    for (var frame = 0; frame < length * 30; frame++) {
      if (out != null && frame % every == 0) {
        File('$out/${(frame ~/ every).toString().padLeft(4, '0')}.png').writeAsBytesSync(view.png());
      }
      // Two steps a frame: 60 a second, filmed at 30.
      for (var i = 0; i < 2; i++) {
        world.update(1 / 60);
        await Future<void>.delayed(Duration.zero);
      }
    }
    expect(errors, isEmpty);
    view.png();
  }, timeout: Timeout.none);
}
