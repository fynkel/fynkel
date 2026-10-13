import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:image/image.dart' as img;
import 'package:test/test.dart';

import 'golden.dart';
import 'mesh_test.dart' show throwsFormat, throwsRefusal;
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
      final ground = Material(baseColorTexture: grid, uvScale: Vec2(4, 4), roughness: 0.9);
      scene.world.add(
        GameObject(
          transform: Transform(position: Vec3(0, -0.5, 0), scale: Vec3(8, 1, 8)),
          components: [MeshRenderer(Mesh.plane(), ground)],
        ),
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

    expect(() => Texture2D.rgba(2, 2, pixels), throwsRefusal(FynkelStatus.invalidValue, 'a 2 × 2 image has 16 bytes'));
    expect(() => Texture2D.rgba(0, 4, Uint8List(0)), throwsRefusal(FynkelStatus.invalidValue, 'it needs at least one'));
    await expectLater(
      Texture2D.load(Uint8List.fromList('not an image'.codeUnits)),
      throwsFormat("the image isn't a PNG or JPEG"),
    );
    await expectLater(
      Texture2D.load(Uint8List.sublistView(asset('grid.png'), 0, 100)),
      throwsFormat("the PNG couldn't be decoded"),
    );
    await expectLater(Texture2D.load(Uint8List(0)), throwsFormat('the image is empty'));
    final gif = Uint8List.fromList(img.encodeGif(img.Image(width: 1, height: 1)));
    await expectLater(Texture2D.load(gif), throwsFormat('only PNG and JPEG'));
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

    expect(red.dispose, throwsRefusal(FynkelStatus.inUse, 'Texture2D(1 × 1) is still used by Material('));
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
    final object = world.add(GameObject(components: [MeshRenderer(mesh, Material())]));
    expect(mesh.dispose, throwsRefusal(FynkelStatus.inUse));
    expect(mesh.vertexCount, 3, reason: 'still usable');

    object.destroy();
    mesh.dispose();
    mesh.dispose();
    expect(mesh.isDisposed, isTrue);
    expect(() => mesh.vertexCount, throwsStateError);
    expect(() => world.add(GameObject(components: [MeshRenderer(mesh, Material())])), throwsStateError);
    expect(Mesh.cube().dispose, throwsStateError);
  });

  test('a material in use cannot be disposed; once free of objects it can, and so can its textures', () {
    final world = World();
    addTearDown(world.dispose);
    final red = Texture2D.rgba(1, 1, Uint8List.fromList([255, 0, 0, 255]));
    final material = Material(name: 'paint', baseColorTexture: red);
    final other = World();
    addTearDown(other.dispose);
    final object = world.add(GameObject(components: [MeshRenderer(Mesh.cube(), material)]));
    final elsewhere = other.add(GameObject(components: [MeshRenderer(Mesh.cube(), material)]));
    expect(material.dispose, throwsRefusal(FynkelStatus.inUse));
    expect(material.isDisposed, isFalse);

    object.destroy();
    elsewhere.destroy();
    expect(red.dispose, throwsRefusal(FynkelStatus.inUse, 'Texture2D(1 × 1) is still used by Material(\'paint\''));
    material.dispose();
    material.dispose();
    expect(material.isDisposed, isTrue);
    expect(() => world.add(GameObject(components: [MeshRenderer(Mesh.cube(), material)])), throwsStateError);
    expect(() => other.add(GameObject(components: [MeshRenderer(Mesh.cube(), material)])), throwsStateError);
    red.dispose();
    expect(red.isDisposed, isTrue, reason: 'no world has a material that uses it now');
    expect(
      world.add(GameObject(components: [MeshRenderer(Mesh.cube(), Material())])).get<MeshRenderer>()!.material,
      isNotNull,
    );
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

    expect(() => Material(uvScale: Vec2(1, 2e4)), throwsRangeError);
    expect(() => Material(uvOffset: Vec2(double.nan, 0)), throwsRangeError);
    expect(() => Material(alphaCutoff: 1.5), throwsRangeError);
    expect(() => scene.orange.emissiveIntensity = -1, throwsRangeError);
    expect(
      Material(uvScale: Vec2(4, 4), alphaCutoff: 0.5, doubleSided: true).toString(),
      'Material(color: ${Color.white}, metallic: 0.0, roughness: 0.5, uvScale: Vec2(4.0, 4.0), alphaCutoff: 0.5, '
      'doubleSided)',
    );
    expect(Material(name: 'Brick', roughness: 0.8).toString(), "Material('Brick')");
  });

  test('a UV scale and offset pick part of a texture, and a double-sided plane shows its back', () {
    final world = World();
    addTearDown(world.dispose);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 3), rotation: Quat.lookRotation(Vec3.forward)),
        components: [Camera(clearColor: Color.black)],
      ),
    );
    // Unlit: only the emission shows, so colours come through as they are.
    // Red and green halves, 32 texels each, so filtering never mixes them
    // where the test looks.
    final pixels = Uint8List(64 * 64 * 4);
    for (var i = 0; i < 64 * 64; i++) {
      pixels.setAll(i * 4, i % 64 < 32 ? [255, 0, 0, 255] : [0, 255, 0, 255]);
    }
    final halves = Texture2D.rgba(64, 64, pixels);
    final material = Material(color: Color.black, emissive: Color.white, emissiveTexture: halves);
    // Mesh.plane() faces +Y: turned to face the camera, or away from it.
    final facing = Quat.axisAngle(Vec3.right, math.pi / 2);
    final plane = world.add(
      GameObject(
        transform: Transform(rotation: facing, scale: Vec3(2, 2, 2)),
        components: [MeshRenderer(Mesh.plane(), material)],
      ),
    );
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    // Which colour the middle of the view shows.
    String centre() {
      final rgba = view.read(view.frame()).rgba;
      final i = (16 * 32 + 16) * 4;
      final [r, g, b] = rgba.sublist(i, i + 3);
      // The tone mapping desaturates bright colours a little.
      if (r > 200 && g < 50 && b < 50) return 'red';
      if (g > 200 && r < 50 && b < 50) return 'green';
      if (r + g + b == 0) return 'black';
      return '($r, $g, $b)';
    }

    // The right half (green) stretched over the whole plane.
    material
      ..uvScale = Vec2(0.5, 1)
      ..uvOffset = Vec2(0.5, 0);
    expect(centre(), 'green');
    material.uvOffset = Vec2.zero;
    expect(centre(), 'red');

    plane.transform = Transform(rotation: Quat.axisAngle(Vec3.up, math.pi) * facing, scale: Vec3(2, 2, 2));
    expect(centre(), 'black', reason: 'single-sided, its back is not drawn');
    material.doubleSided = true;
    expect(centre(), 'red', reason: 'double-sided, its back shows');
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
