import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'sky_test.dart' show near, pixel;

/// sRGB 0.735 is linear 0.5: bright, but not so bright tone mapping bends
/// it.
const half = 0.7354;

/// A wall glowing [wall], 5 m in front of a camera, and a square in front
/// of it, 1 m nearer, of [glass]: no lights, so each shows its glow alone.
World scene(Material glass, {Color wall = const Color.rgb(half, 0, 0)}) {
  final world = World()..sky = null;
  world.add(
    GameObject(
      transform: Transform(position: Vec3(0, 0, 5)),
      components: [Camera(clearColor: Color.black)],
    ),
  );
  Transform facing(double z, double size) =>
      Transform(position: Vec3(0, 0, z), rotation: Quat.axisAngle(Vec3.right, 90.deg), scale: Vec3.all(size));
  world.add(
    GameObject(
      transform: facing(0, 4),
      components: [MeshRenderer(Mesh.plane(), Material(color: Color.black, emissive: wall))],
    ),
  );
  world.add(GameObject(name: 'glass', transform: facing(1, 1), components: [MeshRenderer(Mesh.plane(), glass)]));
  return world;
}

/// The centre of a frame of [world].
List<int> centre(World world) {
  final view = HeadlessView(world, width: 64, height: 64);
  addTearDown(view.dispose);
  addTearDown(world.dispose);
  return pixel(view.read(view.frame()), 32, 32);
}

/// Linear [v] as an 8-bit sRGB value.
int srgb(double v) => ((v <= 0.0031308 ? v * 12.92 : 1.055 * math.pow(v, 1 / 2.4) - 0.055) * 255).round();

void main() {
  test('opacity is the colour\'s alpha, and below 1 makes a material see-through unless told otherwise', () {
    expect((Material().blending, Material().opacity), (Blending.opaque, 1.0));
    expect(Material(opacity: 1).blending, Blending.opaque);
    final glass = Material(color: const Color.rgb(0.2, 0.4, 0.8), opacity: 0.3);
    expect(
      (glass.blending, glass.opacity, glass.color),
      (Blending.transparent, 0.3, const Color.rgb(0.2, 0.4, 0.8, 0.3)),
    );
    expect(Material(opacity: 0.5, blending: Blending.additive).blending, Blending.additive);
    expect(
      Material(color: const Color.rgb(1, 1, 1, 0.5)).blending,
      Blending.opaque,
      reason: 'only opacity picks for you',
    );
    expect(Material(opacity: 0.5, blending: Blending.opaque).blending, Blending.opaque, reason: 'as told');

    // Changing it later changes the alpha alone.
    final m = Material(color: const Color.rgb(1, 0, 0));
    m.opacity = 0.25;
    expect((m.color, m.blending), (const Color.rgb(1, 0, 0, 0.25), Blending.opaque));
    m.blending = Blending.transparent;
    expect(m.blending, Blending.transparent);
    expect(m.copy().blending, Blending.transparent);
    expect(m.toString(), contains('blending: Blending.transparent'));

    expect(() => Material(opacity: 1.5), throwsRangeError);
    expect(() => m.opacity = double.nan, throwsRangeError);
    expect(m.opacity, 0.25, reason: 'refused, nothing changed');
  });

  test('a see-through material shows what is behind it; an additive one adds its glow', () {
    const green = Color.rgb(0, half, 0);
    final over = centre(scene(Material(color: Color.black, emissive: green, opacity: 0.5)));
    expect(over, near([srgb(0.25), srgb(0.25), 0]), reason: 'half the glass, half the wall');

    final glow = centre(
      scene(Material(color: Color.black, emissive: green, opacity: 0.5, blending: Blending.additive)),
    );
    expect(glow, near([srgb(0.5), srgb(0.25), 0]), reason: 'the wall, and half the glow');

    final solid = centre(scene(Material(color: Color.black, emissive: green)));
    expect(solid, near([0, srgb(0.5), 0]), reason: 'opaque, it hides the wall');
  });
}
