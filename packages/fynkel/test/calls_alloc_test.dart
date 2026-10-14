// Calls a game makes every frame allocate nothing: finding a component
// below an object (docs/hardening.md D10), a light's intensity, a
// renderer's colour and a material set each frame (D14), counted by the VM service's
// allocation profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

class _Marker extends Behaviour {}

/// Looks for a [_Marker] below it each frame, at the bottom of a tree.
class _Finder extends GameObject {
  var found = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    if (getInChildren<_Marker>() != null) found++;
  }
}

/// A lamp whose intensity changes each frame, and a cube whose colour does,
/// as a tween's onUpdate sets them (a tween's own easing boxes doubles in
/// the JIT, so it isn't counted here: docs/hardening.md, Raised and left).
class _Pulse extends GameObject {
  _Pulse() : super(components: [Light.point(), MeshRenderer(Mesh.cube(), Material())]);

  var _flip = false;

  @override
  void onUpdate() {
    super.onUpdate();
    _flip = !_flip;
    final light = get<Light>()!, renderer = get<MeshRenderer>()!;
    // A constant each, with no choice between doubles, which the JIT boxes.
    if (_flip) {
      light.intensity = 0.5;
      renderer.color = const Color(0xFFFF0000);
      renderer.material!.metallic = 0.25;
    } else {
      light.intensity = 2.0;
      renderer.color = const Color(0xFF0000FF);
      renderer.material!.metallic = 0.75;
    }
  }
}

void main() {
  final madeEachFrame = allocations();

  test('a frame of common calls allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    final finder = world.add(_Finder());
    var parent = finder as GameObject;
    for (var depth = 0; depth < 6; depth++) {
      for (var i = 0; i < 3; i++) {
        parent.add(GameObject());
      }
      parent = parent.add(GameObject());
    }
    parent.addComponent(_Marker());
    world.add(_Pulse());
    world.update(1 / 60);

    expect(await madeEachFrame(world, 50000), isEmpty);
    expect(finder.found, greaterThan(50000));
  });
}
