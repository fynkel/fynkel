import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'render_test.dart' show stage1;

/// A flat (unencoded) Radiance .hdr, every pixel 0.5 grey: RGBE 128s.
Uint8List greyHdr(int width, int height) {
  final header = '#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y $height +X $width\n'.codeUnits;
  return Uint8List.fromList([...header, for (var i = 0; i < width * height * 4; i++) 128]);
}

/// The 8-bit sRGB of [c], as a view shows it.
List<int> shown(Color c) => [
  for (final v in [c.r, c.g, c.b]) (v * 255).round(),
];

Matcher near(List<int> want, {int within = 1}) => predicate<List<int>>(
  (got) => [for (var i = 0; i < 3; i++) (got[i] - want[i]).abs() <= within].every((x) => x),
  'within $within of $want',
);

/// In a 64 × 64 view's frame.
List<int> pixel(({int number, Uint8List rgba}) frame, int x, int y) {
  final at = (y * 64 + x) * 4;
  return frame.rgba.sublist(at, at + 3);
}

void main() {
  const blue = Color.rgb(0.2, 0.4, 0.8);

  test('a new world has a daylight sky, blue above and pale at the horizon, that lights it', () {
    final world = World();
    addTearDown(world.dispose);
    expect(world.sky, same(Sky.daylight), reason: 'as a new scene in Unity has a sky');
    world.add(GameObject(transform: Transform.lookAt(Vec3(0, 0.3, 0), Vec3(0, 0.6, -1)), components: [Camera()]));
    final view = HeadlessView(world, width: 64, height: 64);
    addTearDown(view.dispose);
    final frame = view.read(view.frame());
    final (top, low) = (pixel(frame, 32, 0), pixel(frame, 32, 63));
    expect(top[2], greaterThan(top[0] + 40), reason: 'blue overhead: $top');
    expect(low.reduce(math.min), greaterThan(top.reduce(math.min)), reason: 'paler toward the horizon: $low, $top');

    // What the sun misses is lit by the sky, not black.
    world.add(GameObject(components: [MeshRenderer(Mesh.cube(), Material(color: Color.white))]));
    world.camera!.gameObject.transform = Transform.lookAt(Vec3(0, 0, 3), Vec3.zero);
    final lit = pixel(view.read(view.frame()), 32, 32);
    expect(lit.reduce(math.min), greaterThan(60), reason: 'a white cube with no light but the sky: $lit');

    world.sky = null;
    final dark = pixel(view.read(view.frame()), 32, 32);
    expect(dark.reduce(math.max), lessThan(10), reason: 'and none without it: $dark');
  });

  test('a sky is behind everything unless the camera leaves it out', () {
    final world = stage1().world;
    final view = HeadlessView(world, width: 64, height: 64);
    addTearDown(view.dispose);
    addTearDown(world.dispose);

    final sky = Sky.gradient(zenith: blue, horizon: blue, ground: blue);
    world.sky = sky;
    expect(world.sky, same(sky));
    expect(pixel(view.read(view.frame()), 0, 0), near(shown(blue)), reason: 'its colours as picked');

    final camera = world.camera!;
    expect(camera.showSky, isTrue);
    camera.showSky = false;
    final clear = shown(camera.clearColor);
    expect(pixel(view.read(view.frame()), 0, 0), near(clear), reason: 'the clear colour without it');
    camera.showSky = true;

    world.sky = Sky.color(blue);
    expect(pixel(view.read(view.frame()), 0, 0), near(shown(blue)), reason: 'a plain sky');
    world.sky = null;
    expect(pixel(view.read(view.frame()), 0, 0), near(clear));

    expect(() => Sky.gradient(zenith: blue, horizon: blue, lighting: -1), throwsRangeError);
    expect(() => Sky.color(blue, lighting: double.nan), throwsRangeError);
  });

  test('a sky image is loaded from an .hdr, by itself or as an asset, and lights the world', () async {
    final image = await SkyImage.load(greyHdr(32, 16));
    expect((image.width, image.height), (32, 16));
    await expectLater(
      SkyImage.load(Uint8List.fromList([0x89, 0x50, 0x4E, 0x47, 13, 10, 26, 10, 0, 0])),
      throwsA(isA<FormatException>().having((e) => e.message, 'message', contains('a sky is a Radiance .hdr'))),
    );

    final world = World(source: (name) async => greyHdr(32, 16));
    stage1(world);
    final view = HeadlessView(world, width: 64, height: 64);
    addTearDown(view.dispose);
    addTearDown(world.dispose);
    final unlit = pixel(view.read(view.frame()), 32, 32);

    final loaded = await world.assets.skyImage('sky.hdr');
    expect(await world.assets.skyImage('sky.hdr'), same(loaded), reason: 'loaded once');
    await world.assets.preload(['sky.hdr']);
    world.sky = Sky.image(loaded, intensity: 1, rotation: 90.deg);
    final frame = view.read(view.frame());
    final corner = pixel(frame, 0, 0);
    expect(corner[0] == corner[1] && corner[1] == corner[2] && corner[0] > 100, isTrue, reason: 'grey: $corner');
    final lit = pixel(frame, 32, 32);
    expect(lit[0] > unlit[0] && lit[2] > unlit[2], isTrue, reason: 'the sky lights the cube: $unlit → $lit');
    expect(view.stats.skyBytes, greaterThan(0));

    image.dispose();
    expect(() => Sky.image(image), throwsStateError);
    expect(() => Sky.image(loaded, intensity: -1), throwsRangeError);
  });

  test('fog fades the world into its colour, or the sky\'s horizon', () {
    final world = stage1().world;
    final view = HeadlessView(world, width: 64, height: 64);
    addTearDown(view.dispose);
    addTearDown(world.dispose);
    const red = Color.rgb(0.8, 0.1, 0.1);

    expect(world.fog, isNull);
    world.fog = const Fog(color: red, start: 0, end: 0.5);
    expect(pixel(view.read(view.frame()), 32, 32), near(shown(red)), reason: 'the cube, 4 m away, all fog');
    world
      ..sky = Sky.gradient(zenith: blue, horizon: blue)
      ..fog = const Fog(start: 0, end: 0.5);
    expect(pixel(view.read(view.frame()), 32, 32), near(shown(blue)), reason: 'the sky\'s horizon');
    world.fog = null;
    final cube = pixel(view.read(view.frame()), 32, 32);
    expect(cube[0], greaterThan(cube[2]), reason: 'the orange cube again');

    expect(() => world.fog = const Fog(start: 5, end: 5), throwsArgumentError);
    expect(world.fog, isNull, reason: 'refused, nothing changed');
  });
}
