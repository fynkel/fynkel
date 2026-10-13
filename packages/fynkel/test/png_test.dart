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
}
