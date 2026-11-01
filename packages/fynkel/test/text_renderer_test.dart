// Flat text in the scene (stage 15 phase 2): a TextRenderer draws its text
// from its font's atlas of distances, crisp, an outline round it if asked,
// hidden behind what's solid unless it's on top, facing as it's told.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

final _roboto = File.fromUri(Directory.current.uri.resolve('../../tests/assets/fonts/Roboto-Regular.ttf'));

void main() {
  late Font font;
  setUpAll(() => font = Font(_roboto.readAsBytesSync()));

  late World world;
  late HeadlessView view;
  setUp(() {
    world = World();
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 5)),
        components: [Camera(clearColor: const Color.rgb(0.1, 0.1, 0.3))],
      ),
    );
    view = HeadlessView(world, width: 96, height: 96);
  });
  tearDown(() {
    view.dispose();
    world.dispose();
  });

  /// How many pixels are near [color]: graded as the camera does, so not
  /// exactly it.
  int pixels(Color color) {
    final image = view.read(view.frame());
    final (r, g, b) = ((color.r * 255).round(), (color.g * 255).round(), (color.b * 255).round());
    var n = 0;
    for (var i = 0; i < image.rgba.length; i += 4) {
      final p = image.rgba;
      if ((p[i] - r).abs() < 56 && (p[i + 1] - g).abs() < 56 && (p[i + 2] - b).abs() < 56) n++;
    }
    return n;
  }

  test('a text is drawn in its colour, and changing it changes what is drawn', () {
    final text = TextRenderer('HI', font: font, size: 2, color: const Color.rgb(1, 1, 0));
    world.add(GameObject(components: [text]));
    final yellow = pixels(const Color.rgb(1, 1, 0));
    expect(yellow, greaterThan(100));
    text.text = 'I';
    expect(text.text, 'I');
    expect(pixels(const Color.rgb(1, 1, 0)), lessThan(yellow * 2 ~/ 3), reason: 'an H fewer');
    text.color = const Color.rgb(0, 1, 0);
    expect(pixels(const Color.rgb(0, 1, 0)), greaterThan(20));
    text.enabled = false;
    expect(pixels(const Color.rgb(0, 1, 0)), 0, reason: 'off, not drawn');
  });

  test('an outline goes round it in its colour', () {
    final text = TextRenderer('O', font: font, size: 2, outline: 0.8, outlineColor: const Color.rgb(1, 0, 0));
    world.add(GameObject(components: [text]));
    expect(pixels(const Color.rgb(1, 0, 0)), greaterThan(50));
    text.outline = 0;
    expect(pixels(const Color.rgb(1, 0, 0)), 0);
  });

  test('a shadow shows under it in its colour, offset as asked', () {
    final text = TextRenderer('I', font: font, size: 3, shadowColor: const Color.rgb(1, 0, 0));
    world.add(GameObject(components: [text]));
    expect(text.shadowOffset, Vec2(0.05, -0.05));
    final small = pixels(const Color.rgb(1, 0, 0));
    expect(small, greaterThan(5), reason: 'its edge shows past the letter');
    text.shadowOffset = Vec2(0.15, -0.1);
    expect(pixels(const Color.rgb(1, 0, 0)), greaterThan(small * 2));
    expect(() => text.shadowSoftness = 2, throwsArgumentError);
    text.shadowColor = const Color.rgb(1, 0, 0, 0);
    expect(pixels(const Color.rgb(1, 0, 0)), 0, reason: 'none, its alpha 0');
  });

  test('a wall hides it unless it is on top', () {
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 2), scale: Vec3(4, 4, 0.1)),
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color.rgb(0, 0, 0)))],
      ),
    );
    final text = TextRenderer('HI', font: font, size: 2);
    world.add(GameObject(components: [text]));
    expect(pixels(Color.white), 0);
    text.onTop = true;
    expect(pixels(Color.white), greaterThan(100));
  });

  test('it faces the camera when told to', () {
    final text = TextRenderer('HI', font: font, size: 2);
    world.add(
      GameObject(
        transform: Transform(rotation: Quat.axisAngle(Vec3(0, 1, 0), 1.5708)),
        components: [text],
      ),
    );
    expect(pixels(Color.white), 0, reason: 'edge on, as its object is turned');
    text.facing = Facing.camera;
    expect(pixels(Color.white), greaterThan(100));
  });

  test('settings that can\'t be are refused, keeping the old', () {
    final text = TextRenderer('x', font: font);
    world.add(GameObject(components: [text]));
    expect(() => text.outline = 2, throwsArgumentError);
    expect(text.outline, 0);
    expect(() => text.size = -1, throwsArgumentError);
    expect(() => TextRenderer('x', font: font, softness: -1), throwsArgumentError);
  });

  test('any letters go, the font\'s or not, even a broken pair', () {
    final text = TextRenderer('é', font: font, size: 2);
    world.add(GameObject(components: [text]));
    for (final s in ['café ±½', '→ 😀 ok', 'broken \uD800 pair', 'Ω\n€', '']) {
      text.text = s;
      expect(text.text, s);
    }
    text.text = 'ÉÉ';
    expect(pixels(Color.white), greaterThan(100), reason: 'two-byte letters drawn');
  });

  test('a font serves texts in two worlds', () {
    final other = World();
    addTearDown(other.dispose);
    world.add(GameObject(components: [TextRenderer('a', font: font)]));
    other.add(GameObject(components: [TextRenderer('b', font: font)]));
    expect(pixels(Color.white), greaterThan(0));
  });
}
