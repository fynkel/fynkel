import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/widgets.dart' show Offset, Size;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:garden/garden.dart';

const width = 320, height = 200;

/// A frame's input with one tap at ([x], [y]), in view pixels.
FrameInput tap(double x, double y) => FrameInput(
  size: Size(width.toDouble(), height.toDouble()),
  taps: [ViewPoint(x, y, width: width.toDouble(), height: height.toDouble())],
);

void main() {
  late Garden garden;
  setUp(() => garden = Garden((asset) async => File('assets/$asset').readAsBytesSync()));
  tearDown(() => garden.world.dispose());

  test('the garden loads, and a tap on the character turns its head; elsewhere it does nothing', () async {
    final times = await garden.load();
    expect(times.map((t) => t.asset), contains('character.glb'));
    expect(times.last.asset, 'character.glb');
    final head = garden.character!.find('head')!;
    final facing = head.transform.rotation;

    // The bench's cast, from the camera to the character's middle, hits it.
    expect(garden.world.raycast(garden.centreRay)?.object.isPartOf(garden.character!), isTrue);

    // The sky, top left: nothing turns.
    garden.onFrame(1, tap(5, 5));
    expect(head.transform.rotation, facing);

    // The character stands in the middle of the view.
    garden.onFrame(0, tap(width / 2, height / 2));
    for (var i = 0; i < 60; i++) {
      garden.onFrame(1 / 60, FrameInput(size: Size(width.toDouble(), height.toDouble())));
    }
    final turned = head.transform.rotation;
    // A quarter turn about its own up: the forward goes from -Z to -X.
    expect(
      Transform(rotation: turned).forward.closeTo(Transform(rotation: facing).rotated(Vec3.up, 1.5708).forward, 1e-3),
      isTrue,
    );

    final view = HeadlessView(garden.world, width: width, height: height);
    addTearDown(view.dispose);
    final frame = view.read(view.frame());
    expect(view.stats.textureBytes, greaterThan(1 << 20), reason: 'the grass and the character texture');
    final shot = Platform.environment['GARDEN_SHOT'];
    if (shot != null) File(shot).writeAsBytesSync(_png(frame.rgba, width, height));
  });

  test('unloading the character frees everything it brought, and it loads again', () async {
    await garden.load();
    final view = HeadlessView(garden.world, width: width, height: height);
    addTearDown(view.dispose);
    view.read(view.frame());
    final loaded = view.stats;

    garden.unloadCharacter();
    expect(garden.character, isNull);
    view.read(view.frame());
    final unloaded = view.stats;
    // The 1024² texture with its mipmaps, about 5.3 MB, and the meshes.
    var texture = 0;
    for (var side = 1024; side >= 1; side ~/= 2) {
      texture += side * side * 4;
    }
    expect(loaded.textureBytes - unloaded.textureBytes, texture);
    expect(unloaded.meshBytes, lessThan(loaded.meshBytes));

    // Tapping where it was does nothing now.
    garden.onFrame(0, tap(width / 2, height / 2));

    await garden.loadCharacter();
    view.read(view.frame());
    expect(view.stats.textureBytes, loaded.textureBytes);
    expect(view.stats.meshBytes, loaded.meshBytes);
  });

  test('a drag goes around the character, and scrolling zooms', () async {
    final start = garden.camera.transform.position;
    garden.onFrame(0, FrameInput(size: const Size(320, 200), drag: const Offset(100, 0)));
    final around = garden.camera.transform.position;
    expect((around - start).length, greaterThan(1));
    expect(around.y, closeTo(start.y, 1e-9), reason: 'sideways, at the same height');

    garden.onFrame(0, FrameInput(size: const Size(320, 200), scroll: const Offset(0, -200)));
    final target = Vec3(0, 1.3, 0);
    expect((garden.camera.transform.position - target).length, lessThan((around - target).length));
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
