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

  test('a shader that moves its vertices moves its object, and its reach keeps it in view', () {
    final scene = stage1();
    final world = scene.world;
    addTearDown(world.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    const source = '''
struct Values { by: vec3<f32> }

fn vertex(in: VertexInput) -> VertexInput {
    var v = in;
    v.position += values.by;
    return v;
}

fn unlit(in: SurfaceInput) -> vec4<f32> {
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}
''';
    final shader = Shader(source, name: 'shift.wgsl');
    expect(shader.movesVertices, isTrue);
    expect(Shader(_tint).movesVertices, isFalse);
    final material = Material(shader: shader, reach: 0.5);
    expect(material.reach, 0.5);
    expect(material.copy().reach, 0.5);
    expect(() => Material(reach: -1), throwsRangeError);
    expect(() => material.reach = double.nan, throwsRangeError);
    final cube = scene.cube;
    final renderer = cube.get<MeshRenderer>()!..material = material;
    expect(_centre(view), _near([255, 0, 0]));

    // Moved well away, the middle shows what's behind.
    material.set('by', Vec3(0, 50, 0));
    expect(_centre(view), isNot(_near([255, 0, 0])));

    // Placed away and moved back: in view only with a reach that says so.
    final at = cube.position;
    cube.position = at + Vec3(0, 50, 0);
    material.set('by', Vec3(0, -50, 0));
    expect(_centre(view), isNot(_near([255, 0, 0])), reason: 'culled where its mesh is');
    material.reach = 51;
    expect(_centre(view), _near([255, 0, 0]));
    expect(renderer.material!.reach, 51);
  });

  test('a shader that reads what is behind it shows the scene through its object', () {
    final scene = stage1();
    final world = scene.world;
    addTearDown(world.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    const source = '''
fn unlit(in: SurfaceInput) -> vec4<f32> {
    return vec4<f32>(scene_color(in.screen), 1.0);
}
''';
    final shader = Shader(source, name: 'clear.wgsl');
    expect(shader.readsBehind, isTrue);
    expect(Shader(_tint).readsBehind, isFalse);
    final cube = scene.cube;
    cube.get<MeshRenderer>()!.material = Material(color: const Color(0xFFFF0000));
    final red = _centre(view);
    expect(red[0] > 150 && red[1] < 30, isTrue, reason: 'the red cube is in the middle: $red');
    // Opaque as it is, it's drawn over the scene, which shows through.
    cube.get<MeshRenderer>()!.material = Material(shader: shader);
    expect(_centre(view), isNot(_near(red)));
  });

  test('a shader with its own light lights its object, light by light', () {
    final world = World();
    final scene = stage1(world);
    addTearDown(world.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    const surface = 'fn surface(in: SurfaceInput) -> Surface {\n    return standard(in);\n}\n';
    final standard = Shader('${surface}fn light(in: LightInput) -> vec3<f32> {\n    return standard_light(in);\n}\n');
    final dark = Shader('${surface}fn light(in: LightInput) -> vec3<f32> {\n    return vec3<f32>(0.0);\n}\n');
    expect(standard.hasOwnLighting, isTrue);
    expect(Shader(_tint).hasOwnLighting, isFalse);
    final renderer = scene.cube.get<MeshRenderer>()!;
    renderer.material = Material(color: const Color(0xFFFF0000));
    final lit = _centre(view);
    renderer.material = Material(color: const Color(0xFFFF0000), shader: standard);
    expect(_centre(view), _near(lit));
    // Lit by no sun or lamp: only the ambient light is left.
    renderer.material = Material(color: const Color(0xFFFF0000), shader: dark);
    expect(_centre(view)[0], lessThan(lit[0] - 40));
    expect(
      () => Shader('${surface}fn light(in: LightInput) -> vec4<f32> {\n    return vec4<f32>(1.0);\n}\n'),
      throwsA(isA<ShaderError>().having((e) => e.message, 'message', contains('fn light(in: LightInput)'))),
    );
  });

  test('an outline rings its object in its colour', () {
    final world = World();
    final scene = stage1(world);
    addTearDown(world.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    int green() {
      final rgba = view.read(view.frame()).rgba;
      var n = 0;
      for (var i = 0; i < rgba.length; i += 4) {
        if (rgba[i + 1] > 200 && rgba[i] < 60 && rgba[i + 2] < 60) n++;
      }
      return n;
    }

    final material = Material(color: const Color(0xFFFF0000));
    scene.cube.get<MeshRenderer>()!.material = material;
    expect(green(), 0);
    material.outline = const Outline(width: 0.3, color: Color(0xFF00FF00));
    expect(green(), greaterThan(10));
    expect(material.copy().outline, const Outline(width: 0.3, color: Color(0xFF00FF00)));
    expect(material.toString(), contains('outline'));
    material.outline = null;
    expect(green(), 0);
    expect(() => Material(outline: const Outline(width: -1)), throwsRangeError);
    expect(() => material.outline = const Outline(width: double.nan), throwsRangeError);
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

  test("a camera's screen effects run over its picture, in order, and change with their values", () {
    final world = World();
    stage1(world);
    addTearDown(world.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    final invert = Shader(
      'fn effect(in: EffectInput) -> vec4<f32> {\n    return vec4<f32>(1.0 - picture(in.uv).rgb, 1.0);\n}\n',
      name: 'invert.wgsl',
    );
    final gain = Shader(
      'struct Values { gain: vec3<f32> }\n'
      'fn effect(in: EffectInput) -> vec4<f32> {\n    return vec4<f32>(picture(in.uv).rgb * values.gain, 1.0);\n}\n',
      name: 'gain.wgsl',
    );
    expect(invert.isEffect, isTrue);
    expect(gain.values, ['gain']);
    final camera = world.camera!;
    final before = _centre(view);

    // The orange cube's red alone, then inverted (in linear light): green
    // and blue full, red what's left of the orange's.
    final red = ScreenEffect(gain, values: {'gain': Vec3(1, 0, 0)});
    camera.effects = [red, ScreenEffect(invert)];
    expect(_centre(view), _near([203, 255, 255]));
    expect(camera.toString(), contains('effects: [ScreenEffect(gain.wgsl), ScreenEffect(invert.wgsl)]'));
    // The other way round: red's left of the inverted picture.
    camera.effects = [ScreenEffect(invert), red];
    expect(_centre(view)[1], lessThan(10));
    red.set('gain', Vec3(0, 1, 0));
    expect(_centre(view)[0], lessThan(10), reason: 'a value set shows in every camera with the effect');
    expect(red.value('gain'), Vec3(0, 1, 0));

    // A new version of its code lays its values out again.
    gain.reload(
      'struct Values { pad: f32, gain: vec3<f32> }\n'
      'fn effect(in: EffectInput) -> vec4<f32> {\n    return vec4<f32>(picture(in.uv).rgb * values.gain, 1.0);\n}\n',
    );
    expect(_centre(view)[0], lessThan(10));
    expect(_centre(view)[1], greaterThan(100));

    expect(() => gain.dispose(), throwsStateError, reason: 'a camera uses it');
    camera.effects = [];
    expect(_centre(view), before);
    gain.dispose();

    expect(() => ScreenEffect(Shader(_tint, name: 'tint.wgsl')), throwsArgumentError);
    expect(() => Material(shader: invert), throwsArgumentError);
    expect(() => red.set('nope', 1.0), throwsArgumentError);
    expect(() => ScreenEffect(invert).set('gain', 1.0), throwsArgumentError);

    // One with a texture of its own: red, so only the cube's red is kept.
    final tinted = Shader(
      'var tint: color_texture;\n'
      'fn effect(in: EffectInput) -> vec4<f32> {\n    return sample(tint, in.uv) * picture(in.uv);\n}\n',
      name: 'tinted.wgsl',
    );
    expect(tinted.textures, ['tint']);
    final redTexture = Texture2D.rgba(1, 1, Uint8List.fromList([255, 0, 0, 255]));
    final tint = ScreenEffect(tinted, values: {'tint': redTexture});
    expect(tint.value('tint'), same(redTexture));
    camera.effects = [tint];
    expect(_centre(view), _near([before[0], 0, 0]));
    expect(() => redTexture.dispose(), throwsStateError, reason: 'an effect on a camera uses it');
    tint.set('tint', Texture2D.rgba(1, 1, Uint8List.fromList([0, 255, 0, 255])));
    expect(_centre(view), _near([0, before[1], 0]), reason: 'a texture set shows too');
    expect(() => tint.set('nope', redTexture), throwsArgumentError);
    camera.effects = [];
    redTexture.dispose();

    // An effect goes with its camera, so its shader can be freed.
    camera.effects = [ScreenEffect(invert)];
    camera.gameObject.destroy();
    world.update(0.016);
    invert.dispose();
  });

  // Stage 14's DF7b: a reload that turned a value into a texture of the
  // same name threw halfway, the other worlds, the effects and the old
  // code's freeing left undone.
  test('a reload that turns a value into a texture leaves the value out', () {
    final scene = stage1();
    final world = scene.world;
    addTearDown(world.dispose);
    final other = World();
    addTearDown(other.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    final shader = Shader(
      'struct Values { grain: f32 }\n'
      'fn unlit(in: SurfaceInput) -> vec4<f32> {\n    return vec4<f32>(values.grain, 0.0, 0.0, 1.0);\n}\n',
      name: 'grain.wgsl',
    );
    final material = Material(shader: shader)..set('grain', 1.0);
    scene.cube.get<MeshRenderer>()!.material = material;
    other.add(GameObject(components: [MeshRenderer(Mesh.cube(), material)]));
    other.update(1 / 60);
    final effect = Shader(
      'struct Values { grain: f32 }\n'
      'fn effect(in: EffectInput) -> vec4<f32> {\n    return picture(in.uv) * values.grain;\n}\n',
      name: 'grain_effect.wgsl',
    );
    world.camera!.effects = [
      ScreenEffect(effect, values: {'grain': 1.0}),
    ];
    _centre(view);
    shader.reload(
      'var grain: color_texture;\n'
      'fn unlit(in: SurfaceInput) -> vec4<f32> {\n    return sample(grain, in.uv);\n}\n',
    );
    effect.reload(
      'var grain: color_texture;\n'
      'fn effect(in: EffectInput) -> vec4<f32> {\n    return picture(in.uv) * sample(grain, in.uv);\n}\n',
    );
    expect(shader.textures, ['grain']);
    _centre(view);
    other.update(1 / 60);
    // Given a texture now, it draws with it.
    final green = Texture2D.rgba(1, 1, Uint8List.fromList([0, 255, 0, 255]));
    material.set('grain', green);
    world.camera!.effects = [];
    expect(_centre(view), _near([0, 255, 0]));
  });

  test('an array of values reaches the shader, each in its place', () {
    final world = World();
    final scene = stage1(world);
    addTearDown(world.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    final picked = Shader(
      'struct Values { colors: array<vec4<f32>, 4>, weights: array<f32, 3>, pick: i32 }\n'
      'fn unlit(in: SurfaceInput) -> vec4<f32> {\n'
      '    return values.colors[values.pick] * values.weights[values.pick % 3];\n}\n',
      name: 'picked.wgsl',
    );
    expect(picked.values, ['colors', 'weights', 'pick']);
    final material = Material(
      shader: picked,
      values: {
        'colors': [const Color(0xFFFF0000), const Color(0xFF00FF00), const Color(0xFF0000FF)],
        'weights': [1.0, 1.0, 1.0],
        'pick': 1,
      },
    );
    scene.cube.get<MeshRenderer>()!.material = material;
    final green = _centre(view);
    expect(green[1], greaterThan(200));
    expect(green[0] + green[2], lessThan(80));
    material.set('pick', 2);
    expect(_centre(view)[2], greaterThan(200), reason: 'the third colour');
    material.set('pick', 3);
    expect(_centre(view), [0, 0, 0], reason: 'the fourth, left out, is 0');
    material.set('weights', [1.0, 1.0, 0.0]);
    material.set('pick', 2);
    expect(_centre(view), [0, 0, 0], reason: 'weighed by the third weight');

    expect(() => material.set('weights', [1.0, 1.0, 1.0, 1.0]), throwsArgumentError, reason: 'longer than the array');
    expect(() => material.set('colors', const Color(0xFFFF0000)), throwsArgumentError, reason: 'an array takes a list');
    expect(() => material.set('weights', [Vec3(1, 1, 1)]), throwsArgumentError, reason: 'of its own kind');
  });

  test('shadersReady completes once the views have drawn and every shader is ready', () async {
    final world = World();
    final scene = stage1(world);
    addTearDown(world.dispose);
    final view = HeadlessView(world, width: 32, height: 32);
    addTearDown(view.dispose);
    scene.cube.get<MeshRenderer>()!.material = Material(shader: Shader(_tint, name: 'tint.wgsl'));
    var ready = false;
    final waiting = world.shadersReady().then((_) => ready = true);
    await Future<void>.delayed(const Duration(milliseconds: 40));
    expect(ready, isFalse, reason: 'not before the views have drawn twice');
    for (var i = 0; i < 3; i++) {
      world.update(0.016);
      view.read(view.frame());
    }
    await waiting.timeout(const Duration(seconds: 5));
    expect(ready, isTrue);
  });
}
