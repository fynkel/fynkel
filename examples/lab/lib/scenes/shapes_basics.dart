// Stage 6: the built-in shapes. The six meshes every game has, in a row,
// with how many triangles each draws; the quad has a front only, unless
// its material is double-sided.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class ShapesBasicsScene extends LabScene {
  @override
  String get title => 'Shapes: the basics';
  @override
  String get step => 'stage 6';
  @override
  String get about =>
      'The built-in meshes, Unity\'s sizes: a cube and a sphere 1 across, a capsule and a cylinder 2 high (a '
      'default CapsuleCollider\'s shape), a 1 × 1 plane lying down and a 1 × 1 quad standing up. Turn the quad '
      'round: its back isn\'t drawn unless its material is double-sided.';

  @override
  Vec3 get eye => Vec3(0, 3, 8);
  @override
  Vec3 get target => Vec3(0, 0.8, 0);

  late final List<GameObject> shapes;
  late final GameObject quad;
  late final Material quadMaterial = Material(color: const Color(0xFFE05A5A));

  @override
  void build() {
    final made = [
      ('cube', Mesh.cube(), const Color(0xFF4CA6E0), 0.5),
      ('sphere', Mesh.sphere(), const Color(0xFF7BC86C), 0.5),
      ('capsule', Mesh.capsule(), const Color(0xFFFFD040), 1.0),
      ('cylinder', Mesh.cylinder(), const Color(0xFFB07CE0), 1.0),
      ('plane', Mesh.plane(), const Color(0xFFE0A05A), 0.05),
    ];
    shapes = [
      for (final (i, (name, mesh, color, height)) in made.indexed)
        add(shape(name, mesh, color, at: Vec3(i * 1.5 - 4.5, height, 0))),
    ];
    quad = add(
      GameObject(
        name: 'quad',
        transform: Transform(position: Vec3(3, 0.6, 0)),
        components: [MeshRenderer(Mesh.quad(), quadMaterial)],
      ),
    );
    shapes.add(quad);
    readout.value = [for (final s in shapes) '${s.name} ${s.get<MeshRenderer>()!.mesh!.indexCount ~/ 3}'].join('   ');
  }

  @override
  List<Control> get controls => [
    Button('turn them', () {
      for (final s in shapes) {
        s.rotateBy(Vec3.up, math.pi / 2, seconds: 0.6);
      }
    }),
    Button('turn the quad round', () => quad.rotateBy(Vec3.up, math.pi, seconds: 0.6)),
    Toggle('the quad double-sided', () => quadMaterial.doubleSided, (on) {
      quadMaterial.doubleSided = on;
      log.add(on ? 'the quad shows its back too' : 'the quad shows its front only');
    }),
  ];
}
