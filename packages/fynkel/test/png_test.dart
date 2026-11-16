// A picture out of HeadlessView (finding 41), read back by the engine's
// own PNG decoder: it checks every chunk's CRC and the zlib checksum.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  test("HeadlessView.png gives a PNG of the frame, one the engine's decoder reads", () async {
    final world = World();
    addTearDown(world.dispose);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 4)),
        components: [Camera()],
      ),
    );
    world.add(GameObject(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A)))]));
    // Wide enough that the picture takes several of the PNG's blocks.
    final view = HeadlessView(world, width: 256, height: 128);
    addTearDown(view.dispose);
    world.update(1 / 60);
    final png = view.png();
    expect(png.sublist(1, 4), 'PNG'.codeUnits);
    final texture = await Texture2D.load(png);
    expect((texture.width, texture.height), (256, 128));
  });

  // Playtest game 1: every 960 × 540 shot was 2 MB, whatever was in it, as
  // the pixels went in uncompressed; macOS's sips made the same one 118 KB.
  test('HeadlessView.png is compressed', () {
    final world = World();
    addTearDown(world.dispose);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 4)),
        components: [Camera()],
      ),
    );
    world.add(GameObject(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A)))]));
    final view = HeadlessView(world, width: 960, height: 540);
    addTearDown(view.dispose);
    world.update(1 / 60);
    final png = view.png();
    expect(png.length, lessThan(300 * 1000), reason: '${png.length} bytes; the pixels alone are ${960 * 540 * 4}');
  });
}
