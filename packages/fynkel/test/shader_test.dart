import 'dart:convert';
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import 'render_test.dart' show stage1;

const _tint = '''
struct Values { tint: vec4<f32>, steps: i32 }
struct PerObject { boost: f32 }
var picture: color_texture;

fn unlit(in: SurfaceInput) -> vec4<f32> {
    let seen = sample(picture, in.uv).rgb;
    return vec4<f32>(values.tint.rgb * seen + per_object.boost + step(1.0, frame_time()) * vec3<f32>(0.0, 0.0, 1.0), 1.0);
}
''';

/// The middle pixel of [view]'s next frame, RGB.
List<int> _centre(HeadlessView view) {
  final rgba = view.read(view.frame()).rgba;
  final at = (16 * 32 + 16) * 4;
  return rgba.sublist(at, at + 3);
}

Matcher _near(List<int> rgb) => predicate<List<int>>(
  (got) => [for (var i = 0; i < 3; i++) (got[i] - rgb[i]).abs() <= 40].every((b) => b),
  'near $rgb',
);

void main() {
  test('a shader says what it takes, and one that does not compile says where', () {
    final shader = Shader(_tint, name: 'tint.wgsl');
    expect(shader.isLit, isFalse);
    expect(shader.values, ['tint', 'steps']);
    expect(shader.textures, ['picture']);
    expect(shader.perObject, ['boost']);
    expect(shader.toString(), "Shader('tint.wgsl')");

    expect(
      () => Shader('fn surface(in: SurfaceInput) -> Surface {\n    return 1.0;\n}\n', name: 'broken.wgsl'),
      throwsA(isA<ShaderError>().having((e) => (e.file, e.line), 'where', ('broken.wgsl', 2))),
    );
  });

  test("a material's values, its texture, an object's own numbers and the time reach its shader", () {
    final scene = stage1();
    final world = scene.world;
    addTearDown(world.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    final white = Texture2D.rgba(1, 1, Uint8List.fromList([255, 255, 255, 255]));
    final shader = Shader(_tint, name: 'tint.wgsl');
    final material = Material(shader: shader, values: {'tint': Color(0xFFFF0000), 'picture': white});
    final renderer = scene.cube.get<MeshRenderer>()!..material = material;
    expect(_centre(view), _near([255, 0, 0]));

    material.set('tint', Color(0xFF00FF00));
    expect(_centre(view), _near([0, 255, 0]));
    expect(material.value('tint'), Color(0xFF00FF00));

    renderer.set('boost', 1);
    expect(renderer.number('boost'), 1);
    // 1 added to each part: past white, which the tone mapping eases.
    expect(_centre(view), _near([193, 250, 193]));
    renderer.set('boost', 0);

    // A frame counts for a third of a second at most.
    for (var i = 0; i < 4; i++) {
      world.update(0.3);
    }
    expect(_centre(view), _near([0, 255, 255]), reason: 'the time is past 1');

    final black = Texture2D.rgba(1, 1, Uint8List.fromList([0, 0, 0, 255]));
    material.set('picture', black);
    expect(_centre(view), _near([0, 0, 255]));
    expect(() => black.dispose(), throwsStateError, reason: 'its material uses it');
    expect(() => shader.dispose(), throwsStateError, reason: 'its material uses it');
  });

  test('wrong names and kinds are refused, saying what the shader has', () {
    final shader = Shader(_tint, name: 'tint.wgsl');
    final material = Material(shader: shader);
    expect(
      () => material.set('glow', 1.0),
      throwsA(isA<ArgumentError>().having((e) => e.message, 'message', contains('tint, steps, picture'))),
    );
    expect(() => material.set('tint', 1.0), throwsArgumentError);
    expect(() => material.set('steps', 1.5), throwsArgumentError, reason: 'an i32');
    material.set('steps', 3);
    expect(() => Material(values: {'tint': 1.0}), throwsArgumentError, reason: 'no shader');
    expect(() => Material().set('tint', 1.0), throwsStateError);
    expect(() => MeshRenderer(Mesh.cube(), material).set('glow', 1), throwsArgumentError);
    expect(() => MeshRenderer(Mesh.cube(), Material()).set('boost', 1), throwsStateError);
  });

  test('a shader loaded as an asset takes its new code when the world reloads its shaders', () async {
    var code = 'fn unlit(in: SurfaceInput) -> vec4<f32> {\n    return vec4<f32>(1.0, 0.0, 0.0, 1.0);\n}\n';
    final world = World(source: (name) async => Uint8List.fromList(utf8.encode(code)));
    final scene = stage1(world);
    addTearDown(world.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    final shader = await world.assets.shader('flat.wgsl');
    expect(identical(await world.assets.shader('flat.wgsl'), shader), isTrue);
    scene.cube.get<MeshRenderer>()!.material = Material(shader: shader);
    expect(_centre(view), _near([255, 0, 0]));

    code = 'fn unlit(in: SurfaceInput) -> vec4<f32> {\n    return vec4<f32>(0.0, 1.0, 0.0, 1.0);\n}\n';
    await world.reloadShaders();
    expect(_centre(view), _near([0, 255, 0]));

    // Code that doesn't compile keeps the old, and says why.
    final errors = <ObjectError>[];
    world.errors.listen(errors.add);
    code = 'fn unlit(in: SurfaceInput) -> vec4<f32> {\n    return 1;\n}\n';
    await world.reloadShaders();
    await Future<void>.delayed(Duration.zero);
    expect(errors.single.error, isA<ShaderError>().having((e) => e.line, 'line', 2));
    expect(_centre(view), _near([0, 255, 0]));
  });
}
