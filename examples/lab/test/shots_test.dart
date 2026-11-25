// Draws the documentation's pictures (website/public/shots) from the lab's
// scenes, as PNGs in the folder SHOTS names. Without SHOTS, it does nothing.
//   SHOTS=/tmp/shots flutter test test/shots_test.dart
// Then, for each: cwebp -q 82 <name>.png -o ../../website/public/shots/<name>.webp

import 'dart:io';
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:lab/scenes.dart';

/// The scenes drawn: the picture's name, how much closer than the scene's
/// own camera, how far up or down it looks (radians), and the seconds
/// played first.
const shots = <String, (String, double, double, double)>{
  'Animation: skinned': ('skinned', 1.2, 0, 1),
  'Animation: a crowd': ('crowd', 4, -0.02, 2),
  'Building meshes': ('building-meshes', 1.45, -0.01, 1),
  'Character controller: a course': ('character', 1.5, -0.02, 1),
  'Joints: a bridge': ('joints', 1.6, 0.02, 2),
  'Look: everything': ('look', 1, 0, 2),
  'Look: glass, water and glows': ('see-through', 1, 0, 2),
  'Look: glow and contact': ('glow', 1.4, 0, 2),
  'Look: lamps': ('lamps', 1, 0, 2),
  'Look: movement': ('movement', 1.3, 0, 2),
  'Look: screen effects': ('effects', 1, 0, 2),
  'Look: shaders': ('shaders', 1.7, 0.02, 2),
  'Look: shadows': ('shadows', 1, 0, 2),
  'Look: sky and fog': ('sky', 1, 0, 2),
  'Look: toon': ('toon', 1.3, 0, 2),
  "Look: what's behind": ('behind', 1.1, -0.02, 2),
  'Meshes that change': ('meshes-that-change', 1.05, 0, 2),
  'Particles: the look': ('particles', 1.5, 0.03, 3),
  'Particles and the world': ('particles-world', 1.8, 0.03, 3),
  'Shapes: the basics': ('shapes', 1.8, 0, 1),
  'Splines': ('splines', 1.35, 0, 2),
  'Text in the world': ('flat-text', 1.5, 0, 1),
};

void main() {
  final out = Platform.environment['SHOTS'];
  if (out == null) return;
  Directory(out).createSync(recursive: true);
  for (final make in scenes) {
    final shot = shots[make().title];
    if (shot == null) continue;
    final (name, closer, up, seconds) = shot;
    test(name, () async {
      final world = World(source: (name) async => File('assets/$name').readAsBytesSync());
      addTearDown(world.dispose);
      final scene = world.add(make());
      await scene.loaded;
      world.quality = Quality.high;
      if (world.sky == null && !const {'toon', 'lamps', 'effects', 'particles', 'particles-world'}.contains(name)) {
        // The lab's plain scenes are on a dark floor with no sky: by day here.
        world.sky = Sky.daylight;
        for (final light in scene.getAllInChildren<Light>()) {
          if (light.gameObject.name == 'ambient') light.intensity = 0;
          if (light.intensity == 2.5) light.intensity = 1.6;
          light.shadows = true;
        }
        for (final renderer in scene.getAllInChildren<MeshRenderer>()) {
          final scale = renderer.gameObject.lossyScale;
          if (scale.x >= 10 && scale.z >= 10) {
            renderer.material = Material(color: Color.rgb(0.28, 0.36, 0.2), roughness: 0.9);
            renderer.gameObject.localScale = Vec3(400, scale.y, 400);
          }
        }
      }
      final view = HeadlessView(world, width: 1600, height: 800);
      addTearDown(view.dispose);
      for (var i = 0; i < seconds * 60; i++) {
        world.update(1 / 60);
        await Future<void>.delayed(Duration.zero);
      }
      final camera = world.camera!;
      camera.fovY = 2 * math.atan(math.tan(camera.fovY / 2) / closer);
      camera.gameObject.rotate(Vec3.right, -up * 2);
      File('$out/$name.png').writeAsBytesSync(view.png());
    });
  }
}
