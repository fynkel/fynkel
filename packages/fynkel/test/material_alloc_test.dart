// A frame that sets a material's shader values, as the guide's tween does,
// allocates nothing in Dart but the number it boxes to pass one on (stage
// 14's DF7a: each set made about 70 objects, packing every value anew).
// Counted by the VM service's allocation profile (see allocations.dart).

// Counted alone, a file at a time: see dart_test.yaml.
@Tags(['alloc'])
library;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'allocations.dart';

const _source = '''
struct Values { dissolve: f32, tint: vec4<f32>, steps: i32, wind: vec3<f32> }

fn unlit(in: SurfaceInput) -> vec4<f32> {
    return vec4<f32>(values.tint.rgb * (1.0 - values.dissolve) + values.wind * f32(values.steps), 1.0);
}
''';

void main() {
  final madeEachFrame = allocations();

  test('a frame that sets a material\'s values, in two worlds, allocates nothing but the boxed number', () async {
    final world = World(), other = World();
    addTearDown(world.dispose);
    addTearDown(other.dispose);
    final fading = Fading();
    world.add(fading);
    other.add(GameObject(components: [MeshRenderer(Mesh.cube(), fading.material)]));
    final made = await madeEachFrame(world, 2000);
    made.remove('_Double');
    expect(made, isEmpty);
  });
}

/// A cube whose material fades, changes colour, counts and blows each frame.
class Fading extends GameObject {
  final material = Material(shader: Shader(_source, name: 'fading.wgsl'));
  static const _tints = [Color(0xFFFF0000), Color(0xFF00FF00), Color(0xFF0000FF)];
  static const _winds = [Vec3(1, 0, 0), Vec3(0, 1, 0)];
  var _t = 0.0;
  var _frame = 0;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.cube(), material));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _t = (_t + 0.01) % 1;
    _frame++;
    material
      ..set('dissolve', _t)
      ..set('tint', _tints[_frame % 3])
      ..set('steps', _frame % 7)
      ..set('wind', _winds[_frame % 2]);
  }
}
