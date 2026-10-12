// Stage 6: an object's own colour. Five cubes share one material; each
// renderer's color changes that cube alone, as Unity's
// MaterialPropertyBlock does, and Material.copy gives one a material of
// its own.

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

/// A cube that flashes white when clicked: its renderer's own colour,
/// back to the material's after.
class Flasher extends GameObject {
  Flasher(String name, Vec3 at, Material shared)
    : super(
        name: name,
        transform: Transform(position: at, scale: Vec3(0.8, 0.8, 0.8)),
        components: [MeshRenderer(Mesh.cube(), shared)],
      );

  MeshRenderer get renderer => get<MeshRenderer>()!;

  @override
  bool onClick(Finger finger) {
    renderer.color = Color.white;
    colorTo(renderer.material!.color, seconds: 0.3, onComplete: () => renderer.color = null);
    ancestor<LabScene>()!.log.add('$name: flashed, its material untouched');
    return true;
  }
}

class ColourBasicsScene extends LabScene {
  @override
  String get title => 'Colour: the basics';
  @override
  String get step => 'stage 6';
  @override
  String get about =>
      'The five cubes share one material. Click one: it flashes white on its own (renderer.color), and the '
      'others don\'t. "tint the middle one" gives it a colour of its own; "change the shared colour" changes '
      'the material, so every cube without its own colour follows. "a material of its own" copies the material '
      '(Material.copy) for the last cube.';

  @override
  Vec3 get eye => Vec3(0, 3, 7);
  @override
  Vec3 get target => Vec3(0, 0.5, 0);

  final shared = Material(name: 'shared', color: const Color(0xFF4CA6E0));
  late final List<Flasher> cubes;
  var _shade = 0;

  static const _shades = [Color(0xFF4CA6E0), Color(0xFF7BC86C), Color(0xFFB07CE0)];

  @override
  void build() {
    cubes = [for (var i = 0; i < 5; i++) add(Flasher('cube $i', Vec3((i - 2) * 1.3, 0.5, 0), shared))];
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = [
      for (final cube in cubes)
        '${cube.name}: ${cube.renderer.material == shared ? 'shared' : 'own'} material'
            '${cube.renderer.color == null ? '' : ', own colour'}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Toggle('tint the middle one', () => cubes[2].renderer.color != null, (on) {
      cubes[2].renderer.color = on ? const Color(0xFFE05A5A) : null;
      log.add(on ? 'cube 2: renderer.color = red' : 'cube 2: renderer.color = null, the material\'s again');
    }),
    Button('change the shared colour', () {
      shared.color = _shades[++_shade % _shades.length];
      log.add('shared.color changed: every cube without its own colour follows');
    }),
    Toggle('a material of its own', () => cubes[4].renderer.material != shared, (on) {
      cubes[4].renderer.material = on ? (shared.copy(name: 'cube 4\'s')..color = const Color(0xFFFFD040)) : shared;
      log.add(on ? 'cube 4: shared.copy(), made yellow' : 'cube 4: back to the shared material');
    }),
  ];
}
