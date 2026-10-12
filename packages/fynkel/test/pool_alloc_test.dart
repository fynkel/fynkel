// A frame that hands out and releases pooled objects allocates nothing
// (docs/stage-6-fixes.md, PR 9), counted by the VM service's allocation
// profile (see allocations.dart).

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

class _Shot extends GameObject {
  _Shot() : super(components: [MeshRenderer(Mesh.cube(), Material())]);
}

/// Each frame releases the oldest 10 shots and fires 10 more.
class _Gun extends GameObject {
  late final shots = Pool(this, _Shot.new);
  final live = <_Shot>[];

  @override
  void onStart() {
    super.onStart();
    shots.prewarm(200);
    for (var i = 0; i < 100; i++) {
      live.add(shots.get());
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    for (var i = 0; i < 10; i++) {
      shots.release(live[i]);
      live[i] = shots.get(_place);
    }
  }

  static void _place(_Shot shot) => shot.transform = const Transform(position: Vec3(1, 2, 3));
}

void main() {
  final madeEachFrame = allocations();

  test('a frame that releases and gets 10 pooled objects allocates nothing', () async {
    final world = World();
    addTearDown(world.dispose);
    world.add(_Gun());

    // Many frames: now and then, in any world, a count takes in decoding
    // one of the VM service's own answers, some 30,000 lists however many
    // frames ran; a frame's own list each would be 50,000.
    expect(await madeEachFrame(world, 50000), isEmpty);
  });
}
