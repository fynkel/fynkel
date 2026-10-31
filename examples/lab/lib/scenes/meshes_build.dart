// Stage 13 phase 2: building meshes. A row of the shapes with settings,
// remade as the sliders move; a letter B extruded with its two holes; a
// vase turned from a profile on the lathe; and a town of houses built
// with a MeshBuilder (walls and roofs as two parts) and combined into one
// mesh, one object drawn with a draw per material.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class MeshesBuildScene extends LabScene {
  /// The whole page; with [still], nothing turns on its own (for its
  /// golden).
  MeshesBuildScene({this.still = false});

  /// Whether the shapes stay as they're made.
  final bool still;

  @override
  String get title => 'Building meshes';
  @override
  String get step => 'stage 13 phase 2';
  @override
  String get about =>
      'SHAPES: Mesh.box, sphere, cylinder, cone, torus, capsule, disc (a ring) and grid, each made with its '
      'settings; move "segments" and "size" to make them again. '
      'LETTER: a Shape2D outline with two holes, Mesh.extrude made solid. '
      'VASE: a profile turned around Y with Mesh.lathe, its wall thick. '
      'TOWN: houses built a face at a time with a MeshBuilder, walls and roofs as two parts, then combined with '
      'Mesh.combine into one mesh: one object, a draw for each of its two materials.';

  @override
  Vec3 get eye => Vec3(0, 4.6, 8.4);
  @override
  Vec3 get target => Vec3(0, 0.5, 0);

  var segments = 16.0;
  var size = 1.0;
  var turning = true;
  var _seed = 1;

  final _shapes = <GameObject>[];
  late final GameObject letter, vase;
  GameObject? town;

  /// How many houses the town has, and what combining them took.
  var houses = 0;
  var _combineMs = 0.0;

  static const _names = ['box', 'sphere', 'cylinder', 'cone', 'torus', 'capsule', 'ring', 'grid'];
  static const _colors = [
    Color(0xFFE07A5F),
    Color(0xFF81B29A),
    Color(0xFFF2CC8F),
    Color(0xFF3D85C6),
    Color(0xFFC77DFF),
    Color(0xFF90BE6D),
    Color(0xFFF9844A),
    Color(0xFF4D908E),
  ];

  @override
  void build() {
    for (var i = 0; i < _names.length; i++) {
      _shapes.add(
        add(
          GameObject(
            name: _names[i],
            transform: Transform(position: Vec3(-5.25 + i * 1.5, 0.75, -1.6)),
            components: [MeshRenderer(_shape(i), Material(color: _colors[i], roughness: 0.55))],
          ),
        ),
      );
    }
    letter = add(
      GameObject(
        name: 'letter',
        transform: Transform(position: Vec3(-3.4, 0.9, 1.6), rotation: Quat.axisAngle(Vec3.up, 0.35)),
        components: [
          MeshRenderer(Mesh.extrude(letterB(), depth: 0.35), Material(color: const Color(0xFFE63946), roughness: 0.4)),
        ],
      ),
    );
    vase = add(
      GameObject(
        name: 'vase',
        transform: Transform(position: Vec3(-0.4, 0, 1.6)),
        components: [
          MeshRenderer(
            Mesh.lathe(vaseProfile(), segments: 40),
            Material(color: const Color(0xFF2A9D8F), roughness: 0.25, metallic: 0.1),
          ),
        ],
      ),
    );
    makeTown();
  }

  /// Shape [i] with the sliders' settings.
  Mesh _shape(int i) {
    final n = segments.round();
    final s = size;
    return switch (i) {
      0 => Mesh.box(size: Vec3(s, s, s), segmentsX: math.max(1, n ~/ 4), segmentsY: math.max(1, n ~/ 4)),
      1 => Mesh.sphere(radius: s / 2, segments: math.max(3, n), rings: math.max(2, n ~/ 2)),
      2 => Mesh.cylinder(radius: s / 2, height: s, segments: math.max(3, n)),
      3 => Mesh.cone(radius: s / 2, height: s, segments: math.max(3, n)),
      4 => Mesh.torus(radius: s * 0.36, tube: s * 0.14, segments: math.max(3, n), sides: math.max(3, n ~/ 2)),
      5 => Mesh.capsule(radius: s * 0.3, height: s * 1.2, segments: math.max(3, n), rings: math.max(1, n ~/ 4)),
      6 => Mesh.disc(radius: s / 2, inner: s / 4, segments: math.max(3, n)),
      _ => Mesh.grid(width: s, depth: s, segmentsX: math.max(1, n ~/ 2), segmentsZ: math.max(1, n ~/ 2)),
    };
  }

  /// Makes every shape again with the sliders' settings. The old meshes
  /// are let go once nothing reaches them.
  void remake() {
    for (var i = 0; i < _shapes.length; i++) {
      _shapes[i].get<MeshRenderer>()!.mesh = _shape(i);
    }
  }

  /// Builds a town of houses, a few kinds made with a [MeshBuilder] and
  /// placed by [Mesh.combine]: one mesh, walls part 0 and roofs part 1.
  void makeTown() {
    final random = math.Random(_seed);
    final kinds = [
      for (final (w, d, h) in [(0.5, 0.6, 0.4), (0.7, 0.5, 0.35), (0.45, 0.45, 0.6), (0.9, 0.5, 0.3)])
        (house(w, d, h, roof: 0.22 + w * 0.25), w, d),
    ];
    final parts = <CombineInstance>[];
    final watch = Stopwatch()..start();
    for (var row = 0; row < 4; row++) {
      for (var column = 0; column < 4; column++) {
        if (random.nextDouble() < 0.15) continue; // a garden
        final (mesh, _, _) = kinds[random.nextInt(kinds.length)];
        final turn = random.nextInt(4) * math.pi / 2 + (random.nextDouble() - 0.5) * 0.3;
        parts.add(
          CombineInstance(
            mesh,
            position: Vec3(column * 0.95 - 1.42 + random.nextDouble() * 0.15, 0, row * 0.95 - 1.42),
            rotation: Quat.axisAngle(Vec3.up, turn),
          ),
        );
      }
    }
    final mesh = Mesh.combine(parts);
    _combineMs = watch.elapsedMicroseconds / 1000;
    houses = parts.length;
    for (final (kind, _, _) in kinds) {
      kind.dispose();
    }
    town?.destroy();
    town = add(
      GameObject(
        name: 'town',
        transform: Transform(position: Vec3(3.6, 0, 1.4), scale: Vec3.all(0.95)),
        components: [
          MeshRenderer(mesh)
            ..materials = [
              Material(color: const Color(0xFFF1E3C8), roughness: 0.9),
              Material(color: const Color(0xFFB5523B), roughness: 0.7),
            ],
        ],
      ),
    );
    _seed++;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (turning && !still) {
      final spin = Quat.axisAngle(Vec3.up, time.delta * 0.6);
      for (final shape in _shapes) {
        shape.rotation = spin * shape.rotation;
      }
      vase.rotation = spin * vase.rotation;
    }
    final town = this.town!.get<MeshRenderer>()!.mesh!;
    readout.value = [
      'shapes at ${segments.round()} segments: ${[for (final s in _shapes) s.get<MeshRenderer>()!.mesh!.vertexCount].join(', ')} vertices',
      'town: $houses houses in one mesh, ${town.vertexCount} vertices, ${town.submeshCount} parts '
          '(one draw each); combined in ${_combineMs.toStringAsFixed(2)} ms',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Slider(
      'segments',
      () => segments,
      (v) {
        segments = v;
        remake();
      },
      min: 3,
      max: 48,
    ),
    Slider(
      'size',
      () => size,
      (v) {
        size = v;
        remake();
      },
      min: 0.4,
      max: 1.2,
    ),
    Toggle('turning', () => turning, (on) => turning = on),
    Button('a new town', () {
      makeTown();
      log.add('a new town of $houses houses');
    }),
  ];
}

/// A house [width] by [depth], its walls [height] high under a gable roof
/// [roof] high: walls (and gable ends) part 0, the roof part 1, every face
/// flat.
Mesh house(double width, double depth, double height, {required double roof}) {
  final (w, d, h) = (width / 2, depth / 2, height);
  final b = MeshBuilder();
  // The four walls, each counter-clockwise seen from outside.
  b
    ..addQuad(Vec3(-w, 0, d), Vec3(w, 0, d), Vec3(w, h, d), Vec3(-w, h, d))
    ..addQuad(Vec3(w, 0, -d), Vec3(-w, 0, -d), Vec3(-w, h, -d), Vec3(w, h, -d))
    ..addQuad(Vec3(w, 0, d), Vec3(w, 0, -d), Vec3(w, h, -d), Vec3(w, h, d))
    ..addQuad(Vec3(-w, 0, -d), Vec3(-w, 0, d), Vec3(-w, h, d), Vec3(-w, h, -d))
    // The gable ends.
    ..addTriangle(Vec3(-w, h, d), Vec3(w, h, d), Vec3(0, h + roof, d))
    ..addTriangle(Vec3(w, h, -d), Vec3(-w, h, -d), Vec3(0, h + roof, -d));
  // The roof, hanging over the walls a little.
  const over = 0.05;
  final (ew, eh, ed) = (w + over, h - over * roof / w, d + over);
  final top = h + roof;
  b
    ..submesh(1)
    ..addQuad(Vec3(ew, eh, ed), Vec3(ew, eh, -ed), Vec3(0, top, -ed), Vec3(0, top, ed))
    ..addQuad(Vec3(-ew, eh, -ed), Vec3(-ew, eh, ed), Vec3(0, top, ed), Vec3(0, top, -ed))
    // Their undersides, seen from below the eaves.
    ..addQuad(Vec3(0, top - 0.001, ed), Vec3(0, top - 0.001, -ed), Vec3(ew, eh - 0.001, -ed), Vec3(ew, eh - 0.001, ed))
    ..addQuad(
      Vec3(0, top - 0.001, -ed),
      Vec3(0, top - 0.001, ed),
      Vec3(-ew, eh - 0.001, ed),
      Vec3(-ew, eh - 0.001, -ed),
    );
  return b.build();
}

/// A letter B 1.2 high: a straight back and two bowls, each with a hole.
Shape2D letterB() {
  // Points round an arc about (cx, cy) of radius r, from one angle to
  // another, in degrees.
  Iterable<Vec2> arc(double cx, double cy, double r, double from, double to, {int steps = 12}) sync* {
    for (var i = 0; i <= steps; i++) {
      final a = (from + (to - from) * i / steps) * math.pi / 180;
      yield Vec2(cx + r * math.cos(a), cy + r * math.sin(a));
    }
  }

  // Along the bottom from the back, round the lower bowl, in to the
  // waist, round the upper bowl and back along the top.
  final outline = [
    const Vec2(-0.4, -0.6),
    ...arc(0.05, -0.3, 0.3, -90, 90),
    const Vec2(-0.05, 0),
    ...arc(0, 0.3, 0.3, -75, 90),
    const Vec2(-0.4, 0.6),
  ];
  // A D in each bowl.
  List<Vec2> hole(double cx, double cy) => [
    Vec2(-0.2, cy - 0.15),
    ...arc(cx, cy, 0.15, -90, 90, steps: 10),
    Vec2(-0.2, cy + 0.15),
  ];
  return Shape2D(outline, holes: [hole(0.05, -0.3), hole(0, 0.3)]);
}

/// A vase's profile: from the middle of its base out, up its belly and
/// neck to the lip, then back down inside, a wall 4 cm thick.
List<Vec2> vaseProfile() {
  final outside = <Vec2>[const Vec2(0, 0), const Vec2(0.3, 0)];
  for (var i = 1; i <= 20; i++) {
    final y = 1.4 * i / 20;
    // A belly low down, a narrow neck, a lip flaring out.
    final r = 0.3 + 0.18 * math.sin(y / 1.4 * math.pi * 1.1) - 0.12 * y + 0.1 * math.pow(y / 1.4, 6);
    outside.add(Vec2(r, y));
  }
  final inside = [
    for (final p in outside.reversed.skip(1).take(outside.length - 3)) Vec2(math.max(0.02, p.x - 0.04), p.y),
  ];
  // Round the lip and down inside to a floor 4 cm up.
  return [
    ...outside,
    Vec2(outside.last.x - 0.04, outside.last.y),
    ...inside.skip(1),
    const Vec2(0.26, 0.04),
    const Vec2(0, 0.04),
  ];
}
