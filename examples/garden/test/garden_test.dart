import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:garden/garden.dart';

const width = 320, height = 200;

var _finger = 0;

/// Gives [world]'s input a tap at ([x], [y]), in view pixels, for its next
/// frame.
void tap(World world, double x, double y) {
  final id = ++_finger;
  world.input.simulate
    ..resize(width.toDouble(), height.toDouble())
    ..fingerDown(id, x, y)
    ..fingerUp(id, x, y);
}

void main() {
  late World world;
  late Garden garden;
  setUp(() {
    world = World(source: (asset) async => File('assets/$asset').readAsBytesSync());
    garden = world.add(Garden());
  });
  tearDown(() => world.dispose());

  /// What the engine holds for drawing: [ViewStats.textureBytes] and
  /// [ViewStats.meshBytes] after a frame.
  (int, int) held(HeadlessView view) {
    view.read(view.frame());
    return (view.stats.textureBytes, view.stats.meshBytes);
  }

  test('the garden loads, and a tap on the character turns its head; elsewhere it does nothing', () async {
    await garden.loaded;
    final character = world.first<Character>()!;
    final head = character.find('head')!;
    final facing = head.transform.rotation;
    world.update(0);

    // The sky, top left: nothing turns.
    tap(world, 5, 5);
    world.update(1);
    expect(head.transform.rotation, facing);

    // The character stands in the middle of the view.
    tap(world, width / 2, height / 2);
    world.update(0);
    for (var i = 0; i < 60; i++) {
      world.update(1 / 60);
    }
    // A quarter turn about its own up: the forward goes from -Z to -X.
    expect(
      head.transform.rotation
          .rotate(Vec3.forward)
          .closeTo(Transform(rotation: facing).rotated(Vec3.up, 1.5708).rotation.rotate(Vec3.forward), 1e-3),
      isTrue,
    );

    final view = HeadlessView(world, width: width, height: height);
    addTearDown(view.dispose);
    final frame = view.read(view.frame());
    expect(view.stats.textureBytes, greaterThan(1 << 20), reason: 'the grass and the character texture');
    final shot = Platform.environment['GARDEN_SHOT'];
    if (shot != null) File(shot).writeAsBytesSync(_png(frame.rgba, width, height));
  });

  test('destroying the garden frees everything it loaded, and a new one loads the same', () async {
    final view = HeadlessView(world, width: width, height: height);
    addTearDown(view.dispose);
    garden.destroy();
    // The plane and the cube stay: the engine's own meshes.
    world.add(GameObject(components: [MeshRenderer(Mesh.plane(), Material())]));
    final empty = held(view);

    final first = world.add(Garden());
    await first.loaded;
    final loaded = held(view);
    expect(loaded.$1, greaterThan(empty.$1 + (1 << 20)));

    for (var i = 0; i < 3; i++) {
      world.first<Garden>()!.destroy();
      expect(world.all<Props>(), isEmpty);
      expect(held(view), empty, reason: 'every texture and mesh freed');
      await world.add(Garden()).loaded;
      expect(held(view), loaded);
    }
  });

  test('a drag goes around the character, and scrolling zooms', () async {
    await garden.loaded;
    final camera = world.first<OrbitCamera>()!;
    world.update(0);
    final start = camera.transform.position;
    world.input.simulate
      ..resize(320, 200)
      ..fingerDown(99, 100, 100);
    world.update(0);
    world.input.simulate.fingerMove(99, 200, 100);
    world.update(0);
    world.input.simulate.fingerUp(99, 200, 100);
    final around = camera.transform.position;
    expect((around - start).length, greaterThan(1));
    expect(around.y, closeTo(start.y, 1e-9), reason: 'sideways, at the same height');

    world.input.simulate.scrollBy(0, -200);
    world.update(0);
    const target = OrbitCamera.target;
    expect((camera.transform.position - target).length, lessThan((around - target).length));
  });
}

/// [rgba] as a PNG, unfiltered.
Uint8List _png(Uint8List rgba, int width, int height) {
  final raw = BytesBuilder();
  for (var y = 0; y < height; y++) {
    raw.addByte(0);
    raw.add(Uint8List.sublistView(rgba, y * width * 4, (y + 1) * width * 4));
  }
  Uint8List chunk(String kind, List<int> data) {
    final body = [...kind.codeUnits, ...data];
    final b = ByteData(8 + data.length + 4);
    b.setUint32(0, data.length);
    final out = b.buffer.asUint8List()..setAll(4, body);
    b.setUint32(8 + data.length, _crc(body));
    return out;
  }

  final header = ByteData(13)
    ..setUint32(0, width)
    ..setUint32(4, height)
    ..setUint8(8, 8)
    ..setUint8(9, 6);
  return Uint8List.fromList([
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, //
    ...chunk('IHDR', header.buffer.asUint8List()),
    ...chunk('IDAT', zlib.encode(raw.takeBytes())),
    ...chunk('IEND', const []),
  ]);
}

int _crc(List<int> bytes) {
  var c = 0xFFFFFFFF;
  for (final byte in bytes) {
    c ^= byte;
    for (var k = 0; k < 8; k++) {
      c = c & 1 != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
    }
  }
  return c ^ 0xFFFFFFFF;
}
