import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:image/image.dart' as img;
import 'package:test/test.dart';

import 'golden.dart';
import 'mesh_test.dart' show throwsFynkel;
import 'render_test.dart' show stage1;

final _assets = Directory.current.uri.resolve('../../tests/assets/');
Uint8List asset(String name) => File.fromUri(_assets.resolve(name)).readAsBytesSync();

void main() {
  test('the test card renders to the same golden image as the Rust test, from PNG and from JPEG', () async {
    // The JPEG is lossy, so it's only compared with the PNG, loosely.
    final frames = <Uint8List>[];
    for (final file in ['grid.png', 'grid.jpg']) {
      final scene = stage1();
      addTearDown(scene.world.dispose);
      final grid = await Texture2D.load(asset(file));
      expect((grid.width, grid.height), (128, 128));
      scene.orange
        ..color = Color.white
        ..baseColorTexture = grid;
      final ground = Material(baseColorTexture: grid, uvScale: 4, roughness: 0.9);
      scene.world.spawn(
        Transform(position: Vec3(0, -0.5, 0), scale: Vec3(8, 1, 8)),
        MeshRenderer(Mesh.plane(), ground),
      );
      final view = HeadlessView(scene.world, width: 256, height: 160);
      addTearDown(view.dispose);
      frames.add(view.read(view.frame()).rgba);

      final mips = [for (var i = 0; i < 8; i++) math.pow(128 >> i, 2).toInt() * 4].reduce((a, b) => a + b);
      expect(view.stats.textureBytes, mips, reason: 'one 128² texture, sRGB only, with its mipmaps');
      expect(view.stats.meshBytes, greaterThan(0));
    }
    expectGolden('textured_scene', frames[0], 256, 160);
    var far = 0;
    for (var i = 0; i < frames[0].length; i++) {
      if ((frames[0][i] - frames[1][i]).abs() > 24) far++;
    }
    expect(far / frames[0].length, lessThan(0.01), reason: 'the JPEG looks like the PNG');
  });

  test('a texture from pixels, and images that cannot be used', () async {
    final pixels = Uint8List.fromList([255, 0, 0, 255, 0, 0, 255, 255]);
    final texture = Texture2D.rgba(2, 1, pixels);
    expect((texture.width, texture.height), (2, 1));
    expect(texture.toString(), 'Texture2D(2 × 1)');

    expect(() => Texture2D.rgba(2, 2, pixels), throwsFynkel(FynkelStatus.invalidValue, 'a 2 × 2 image has 16 bytes'));
    expect(() => Texture2D.rgba(0, 4, Uint8List(0)), throwsFynkel(FynkelStatus.invalidValue, 'it needs at least one'));
    await expectLater(
      Texture2D.load(Uint8List.fromList('not an image'.codeUnits)),
      throwsFynkel(FynkelStatus.invalidValue, "the image isn't a PNG or JPEG"),
    );
    await expectLater(
      Texture2D.load(Uint8List.sublistView(asset('grid.png'), 0, 100)),
      throwsFynkel(FynkelStatus.invalidValue, "the PNG couldn't be decoded"),
    );
    await expectLater(Texture2D.load(Uint8List(0)), throwsFynkel(FynkelStatus.invalidValue, 'the image is empty'));
    final gif = Uint8List.fromList(img.encodeGif(img.Image(width: 1, height: 1)));
    await expectLater(Texture2D.load(gif), throwsFynkel(FynkelStatus.invalidValue, 'only PNG and JPEG'));
  });

  test('a texture in use cannot be disposed; once free of materials it can', () {
    final scene = stage1();
    addTearDown(scene.world.dispose);
    final view = HeadlessView(scene.world, width: 16, height: 16);
    addTearDown(view.dispose);
    final red = Texture2D.rgba(1, 1, Uint8List.fromList([255, 0, 0, 255]));
    scene.orange.baseColorTexture = red;
    view.read(view.frame());
    expect(view.stats.textureBytes, 4);

    expect(red.dispose, throwsFynkel(FynkelStatus.inUse, 'Texture2D(1 × 1) is still used by Material('));
    expect(red.isDisposed, isFalse);
    scene.orange.baseColorTexture = null;
    view.read(view.frame());
    expect(view.stats.textureBytes, 4, reason: 'the world still has it, so the view keeps its copy');
    red.dispose();
    view.read(view.frame());
    expect(view.stats.textureBytes, 0, reason: 'gone from the world, so gone from the GPU');
    red.dispose();
    expect(red.isDisposed, isTrue);
    expect(() => scene.orange.baseColorTexture = red, throwsStateError);
    expect(() => Material(normalTexture: red), throwsStateError);
  });

  test('a mesh in use cannot be disposed; a built-in shape never can', () {
    final world = World();
    addTearDown(world.dispose);
    final mesh = Mesh.custom(positions: const [Vec3(0, 0, 0), Vec3(1, 0, 0), Vec3(0, 1, 0)], indices: const [0, 1, 2]);
    final object = world.spawn(MeshRenderer(mesh, Material()));
    expect(mesh.dispose, throwsFynkel(FynkelStatus.inUse));
    expect(mesh.vertexCount, 3, reason: 'still usable');

    object.despawn();
    mesh.dispose();
    mesh.dispose();
    expect(mesh.isDisposed, isTrue);
    expect(() => mesh.vertexCount, throwsStateError);
    expect(() => world.spawn(MeshRenderer(mesh, Material())), throwsStateError);
    expect(Mesh.cube().dispose, throwsStateError);
  });

  test('the new material values reach the picture', () {
    final scene = stage1();
    addTearDown(scene.world.dispose);
    final view = HeadlessView(scene.world, width: 32, height: 32);
    addTearDown(view.dispose);
    int centreRed() => view.read(view.frame()).rgba[(16 * 32 + 16) * 4];

    scene.orange.color = Color.black;
    final dark = centreRed();
    scene.orange
      ..emissive = Color.rgb(1, 0, 0)
      ..emissiveIntensity = 0.5;
    expect(centreRed(), greaterThan(dark + 100), reason: 'it glows red');
    scene.orange.emissiveIntensity = 0;
    expect(centreRed(), dark);

    expect(() => Material(uvScale: 0), throwsRangeError);
    expect(() => Material(alphaCutoff: 1.5), throwsRangeError);
    expect(() => scene.orange.emissiveIntensity = -1, throwsRangeError);
    expect(
      Material(uvScale: 4, alphaCutoff: 0.5).toString(),
      'Material(color: ${Color.white}, metallic: 0.0, roughness: 0.5, uvScale: 4.0, alphaCutoff: 0.5)',
    );
  });

  test('Texture2D.load decodes off the calling isolate, so it never stalls the UI', () async {
    // A 4096² photo-like image: smooth colour with noise in the low bits,
    // about 30 MB as PNG.
    final random = math.Random(1);
    final big = img.Image(width: 4096, height: 4096);
    for (final p in big) {
      final (u, v) = (p.x / 4096, p.y / 4096);
      int wave(double f, double phase) =>
          (127 + 100 * math.sin(f * u + phase) * math.cos(f * v - phase)).round() + random.nextInt(16);
      p.setRgb(wave(9, 0), wave(13, 1), wave(7, 2));
    }
    final png = img.encodePng(big, level: 1);

    // The first call into the engine loads its library, once per program
    // (an app does it at its first World); not what's measured here.
    Texture2D.rgba(1, 1, Uint8List(4)).dispose();

    // A 1 ms timer on this isolate: the longest gap between its ticks
    // (and from the last tick to the end) is how long the UI would have
    // been frozen.
    var longest = Duration.zero;
    var last = DateTime.now();
    void tick([_]) {
      final now = DateTime.now();
      if (now.difference(last) > longest) longest = now.difference(last);
      last = now;
    }

    final ticker = Timer.periodic(const Duration(milliseconds: 1), tick);
    final watch = Stopwatch()..start();
    final texture = await Texture2D.load(png);
    final took = watch.elapsed;
    tick();
    ticker.cancel();
    expect((texture.width, texture.height), (4096, 4096));
    // For the test to mean anything the decode must take a while, and the
    // UI must have kept ticking through it. Decoding here would freeze it
    // for about the whole load, so the bound is a share of the load
    // rather than a frame's 16 ms, which a busy CI machine can miss
    // while the timer alone is late.
    expect(took, greaterThan(const Duration(milliseconds: 40)), reason: 'too quick to tell');
    expect(longest, lessThan(took * 0.5), reason: 'the UI froze for $longest of a $took load');
    print('Texture2D.load of ${png.length ~/ 1024} KiB: $took, the UI waited at most $longest');
  });
}
