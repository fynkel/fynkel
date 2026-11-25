// `dart run fynkel:shader <name>`: a new shader to start from, in
// assets/shaders/<name>.wgsl, and how to use it (https://fynkel.com/shaders/first/). `--unlit`, `--effect` (a screen effect), `--moves` (a
// `vertex` too) and `--light` (its own light) choose what it has.

import 'dart:io';

const _usage = '''
Usage: dart run fynkel:shader <name> [--unlit | --effect] [--moves] [--light] [--dir assets/shaders]

Writes <dir>/<name>.wgsl, a shader to start from:
  (default)  a surface the engine lights, from the material's own
  --unlit    its colour as it is, unlit
  --effect   a screen effect, over the camera's whole picture
  --moves    with a vertex function that moves the mesh
  --light    with its own light function (toon bands)
Every name the engine gives a shader: packages/fynkel/wgsl/fynkel.wgsl.''';

void main(List<String> args) {
  final flags = args.where((a) => a.startsWith('--')).toSet();
  final words = args.where((a) => !a.startsWith('--')).toList();
  var dir = 'assets/shaders';
  final at = args.indexOf('--dir');
  if (at >= 0) {
    if (at + 1 >= args.length) return _fail('--dir needs a folder');
    dir = args[at + 1];
    words.remove(dir);
  }
  flags.remove('--dir');
  final known = {'--unlit', '--effect', '--moves', '--light', '--help'};
  if (flags.contains('--help') || words.length != 1 || !known.containsAll(flags)) {
    stdout.writeln(_usage);
    if (!flags.contains('--help')) exitCode = 64;
    return;
  }
  final name = words.single.endsWith('.wgsl') ? words.single.substring(0, words.single.length - 5) : words.single;
  if (!RegExp(r'^[A-Za-z0-9_\-]+$').hasMatch(name)) return _fail('"$name" isn\'t a file name');
  final effect = flags.contains('--effect'), unlit = flags.contains('--unlit');
  if (effect && (unlit || flags.contains('--moves') || flags.contains('--light'))) {
    return _fail('an effect has only its effect function');
  }
  if (unlit && flags.contains('--light')) return _fail('an unlit shader has no light to be lit by');

  final file = File('$dir/$name.wgsl');
  if (file.existsSync()) return _fail('${file.path} is there already');
  file
    ..parent.createSync(recursive: true)
    ..writeAsStringSync(
      effect
          ? _effect(name)
          : _material(name, unlit: unlit, moves: flags.contains('--moves'), light: flags.contains('--light')),
    );

  final path = '${dir.startsWith('assets/') ? dir.substring(7) : dir}/$name.wgsl';
  stdout.writeln('Wrote ${file.path}.');
  stdout.writeln('');
  stdout.writeln('List $dir/ under flutter: assets: in pubspec.yaml if it isn\'t, then:');
  stdout.writeln('');
  if (effect) {
    stdout.writeln("  final $name = ScreenEffect(await assets.shader('$path'), values: {'strength': 1.0});");
    stdout.writeln('  camera.effects = [$name];');
    stdout.writeln("  $name.set('strength', 0.5);");
  } else {
    stdout.writeln(
      "  final $name = Material(shader: await assets.shader('$path'), values: {'tint': Color(0xFFFFFFFF)}"
      "${flags.contains('--moves') ? ', reach: 0.2' : ''});",
    );
    stdout.writeln("  $name.set('tint', Color(0xFF40C0FF));");
  }
  stdout.writeln('');
  stdout.writeln('Save the file and hot reload to see a change. Every name the engine gives a shader is in');
  stdout.writeln('packages/fynkel/wgsl/fynkel.wgsl; the guide\'s "Shaders" section shows each kind.');
}

void _fail(String message) {
  stderr.writeln(message);
  exitCode = 64;
}

String _material(String name, {required bool unlit, required bool moves, required bool light}) {
  final b = StringBuffer()
    ..writeln('// $name: a material\'s shader. Every name the engine gives it is in')
    ..writeln('// packages/fynkel/wgsl/fynkel.wgsl.')
    ..writeln('')
    ..writeln('// What the game sets by name: material.set(\'tint\', Color(...)).')
    ..writeln('struct Values { tint: vec4<f32>${moves ? ', sway: f32' : ''}${light ? ', bands: f32' : ''} }')
    ..writeln('');
  if (moves) {
    b
      ..writeln('// Moves each vertex: here, the top sways and the foot stays. Give the')
      ..writeln('// material a reach as far as it moves a vertex, in metres.')
      ..writeln('fn vertex(in: VertexInput) -> VertexInput {')
      ..writeln('    var v = in;')
      ..writeln('    v.position.x += sin(frame_time() * 2.0 + in.origin.x) * values.sway * in.local.y;')
      ..writeln('    return v;')
      ..writeln('}')
      ..writeln('');
  }
  if (unlit) {
    b
      ..writeln('// The colour as it is, unlit.')
      ..writeln('fn unlit(in: SurfaceInput) -> vec4<f32> {')
      ..writeln('    return in.color * values.tint;')
      ..writeln('}');
  } else {
    b
      ..writeln('// What the surface is; the engine lights it. standard(in) is the')
      ..writeln('// material as the engine has it: change what you need.')
      ..writeln('fn surface(in: SurfaceInput) -> Surface {')
      ..writeln('    var s = standard(in);')
      ..writeln('    s.color *= values.tint.rgb;')
      ..writeln('    return s;')
      ..writeln('}');
  }
  if (light) {
    b
      ..writeln('')
      ..writeln('// Lights it one sun or lamp at a time, in place of the engine: here in')
      ..writeln('// flat bands. standard_light(in) is the engine\'s own.')
      ..writeln('fn light(in: LightInput) -> vec3<f32> {')
      ..writeln('    let lit = max(dot(in.normal, in.direction), 0.0) * in.shadow;')
      ..writeln('    let steps = max(values.bands, 1.0);')
      ..writeln('    let band = mix(0.3, 1.0, floor(lit * steps + 0.5) / steps);')
      ..writeln('    return in.surface.color * in.color * band / 3.14159265;')
      ..writeln('}');
  }
  return b.toString();
}

String _effect(String name) =>
    '''
// $name: a screen effect, run over the camera's whole picture. Every name
// the engine gives it is in packages/fynkel/wgsl/fynkel.wgsl.

// What the game sets by name: effect.set('strength', 0.5).
struct Values { strength: f32 }

// Each pixel's colour, from the picture's: here, darker at the edges.
fn effect(in: EffectInput) -> vec4<f32> {
    let seen = picture(in.uv).rgb;
    let edge = smoothstep(0.3, 0.8, length(in.uv - 0.5)) * values.strength;
    return vec4<f32>(seen * (1.0 - edge), 1.0);
}
''';
