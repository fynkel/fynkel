// 3D text: a font from a file's bytes or the world's assets, and text made
// solid from it, laid out in lines and aligned on the origin.

import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'golden.dart';
import 'mesh_building_test.dart' show boundsOf;

final _roboto = File.fromUri(Directory.current.uri.resolve('../../tests/assets/fonts/Roboto-Regular.ttf'));

void main() {
  late Font font;
  setUpAll(() => font = Font(_roboto.readAsBytesSync()));

  test('a file that isn\'t a font is refused, saying so', () {
    expect(
      () => Font(Uint8List.fromList([1, 2, 3, 4])),
      throwsA(isA<FormatException>().having((e) => e.message, 'message', contains('font'))),
    );
  });

  test('text is a slab as deep as asked, its letters standing on the origin', () {
    final hi = Mesh.text('Hi', font: font, size: 2, depth: 0.4, align: TextAlignment.left);
    expect(hi.indexCount, greaterThan(0));
    final (lo, top) = boundsOf(hi);
    expect(lo.z, closeTo(-0.2, 1e-5));
    expect(top.z, closeTo(0.2, 1e-5));
    expect(lo.y, closeTo(0, 1e-4), reason: 'an H stands on the line');
    expect(top.y, inInclusiveRange(1.3, 1.6), reason: 'capitals are about 0.7 of the size');
  });

  test('each line goes across the origin as aligned', () {
    final width = font.measure('HOH').x;
    double left(TextAlignment align) => boundsOf(Mesh.text('HOH', font: font, align: align)).$1.x;
    final start = left(TextAlignment.left);
    expect(start, inInclusiveRange(0, 0.15));
    expect(left(TextAlignment.center), closeTo(start - width / 2, 1e-5));
    expect(left(TextAlignment.right), closeTo(start - width, 1e-5));
  });

  test('lines are the font\'s line height apart, times the spacing', () {
    final one = font.measure('x').y, two = font.measure('xx\nx');
    expect(two.y, closeTo(2 * one, 1e-5));
    expect(two.x, closeTo(font.measure('xx').x, 1e-5), reason: 'the widest line');
    expect(font.measure('x\nx', lineSpacing: 2).y, closeTo(4 * one, 1e-5));
    expect(font.measure('x', size: 3).y, closeTo(3 * one, 1e-5));
    final foot = boundsOf(Mesh.text('x\nx', font: font, lineSpacing: 1.5)).$1.y;
    expect(foot, closeTo(-1.5 * one, 1e-4));
  });

  test('spaces make an empty mesh; a size of 0 is refused', () {
    expect(Mesh.text('  ', font: font).indexCount, 0);
    expect(() => Mesh.text('a', font: font, size: 0), throwsArgumentError);
  });

  test('a world loads a font from its assets, shared by all who ask', () async {
    var reads = 0;
    final world = World(
      source: (name) async {
        reads++;
        return _roboto.readAsBytes();
      },
    );
    addTearDown(world.dispose);
    final a = world.assets.font('fonts/Roboto-Regular.ttf');
    final b = world.assets.font('fonts/Roboto-Regular.ttf');
    expect(identical(await a, await b), isTrue);
    expect(reads, 1);
    expect(Mesh.text('ok', font: await a).indexCount, greaterThan(0));
  });

  test('text in a scene', () {
    final world = World();
    addTearDown(world.dispose);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0.6, 4), rotation: Quat.lookRotation(Vec3(0, -0.85, -4))),
        components: [Camera(bloom: null, ambientOcclusion: null)],
      ),
    );
    world.add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.6))),
        components: [Light.sun()],
      ),
    );
    world.add(GameObject(components: [Light.ambient(intensity: 0.2)]));
    world.add(
      GameObject(
        transform: Transform(rotation: Quat.axisAngle(Vec3.up, -20 * math.pi / 180)),
        components: [
          MeshRenderer(
            Mesh.text('Fynkel\n3D text', font: font, size: 0.9, depth: 0.25),
            Material(color: Color(0xFFE0613A)),
          ),
        ],
      ),
    );
    final view = HeadlessView(world, width: 256, height: 256);
    addTearDown(view.dispose);
    expectGolden('text', view.read(view.frame()).rgba, 256, 256);
  });
}
