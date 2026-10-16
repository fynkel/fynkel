// Stage 8, phase 5: see-through surfaces. Tinted glass panes turning
// among cubes, some in front of the glass and some behind it, so the order
// they're drawn in changes as they turn; a pool of water the balls sit in;
// glowing sparks circling above it, adding their light; and a crowd of
// panes for measuring.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// `--dart-define=LAB_CROWD=true` starts the scene with the crowd of
/// panes, for measuring.
const _startCrowd = bool.fromEnvironment('LAB_CROWD');

/// How many panes the crowd has.
const crowdSize = 400;

class SeeThroughScene extends LabScene {
  @override
  String get title => 'Look: glass, water and glows';
  @override
  String get step => 'stage 8';
  @override
  String get about =>
      'Tinted glass turns among cubes, in front of some and behind others; balls sit in a pool of water; sparks '
      'glow above it. The crowd adds $crowdSize panes, all see-through, for measuring.';

  @override
  Vec3 get eye => Vec3(0, 3.5, 9);
  @override
  Vec3 get target => Vec3(0, 0.8, 0);

  late final GameObject _camera;
  final _panes = <GameObject>[];
  final _sparks = <GameObject>[];
  final _crowd = <GameObject>[];
  late final Material _glass;
  var _turning = true, _swinging = true, _angle = 0.0, _swing = 0.0, _fps = '–';

  /// The glass's tints: amber, sea green, rose.
  static const tints = [Color(0xFFFFB347), Color(0xFF4FD1A5), Color(0xFFFF6F91)];

  @override
  void build() {
    world.quality = Quality.levels.where((q) => q.name == _startQuality).firstOrNull;
    _camera = find('camera')!;
    world
      ..sky = Sky.gradient(
        zenith: const Color(0xFF3F7CC8),
        horizon: const Color(0xFFD3E4F0),
        ground: const Color(0xFF6E6A60),
        lighting: 0.6,
      )
      ..fog = const Fog(start: 15, end: 60);
    find('floor')!.active = false;
    add(shape('ground', Mesh.plane(), const Color(0xFF8A8478), size: Vec3(60, 1, 60)));

    // Three panes, each a material of its own tint: one glass, three
    // colours of it.
    _glass = Material(color: tints[0], opacity: 0.35, roughness: 0.1, doubleSided: true);
    for (final (i, tint) in tints.indexed) {
      final pane = _glass.copy()..color = Color.rgb(tint.r, tint.g, tint.b, _glass.opacity);
      _panes.add(
        add(
          GameObject(
            name: 'pane $i',
            transform: Transform(position: Vec3((i - 1) * 2.2, 1.2, 0), scale: Vec3(1.6, 1, 2.2)),
            components: [MeshRenderer(Mesh.plane(), pane)],
          ),
        ),
      );
    }
    // Cubes in a ring around the panes: as they turn, each is in front of
    // the glass, then behind it.
    for (var i = 0; i < 10; i++) {
      final a = i / 10 * 2 * math.pi;
      add(
        shape(
          'cube $i',
          Mesh.cube(),
          Color.rgb(0.25 + 0.7 * (i % 3) / 2, 0.35, 0.9 - 0.7 * (i % 3) / 2),
          at: Vec3(4.6 * math.cos(a), 0.4, 2.2 * math.sin(a)),
          size: Vec3.all(0.8),
        ),
      );
    }

    // The pool: water over a dark basin floor, balls half in it.
    add(shape('basin', Mesh.plane(), const Color(0xFF2B3A40), at: Vec3(0, 0.01, 4.5), size: Vec3(6.6, 1, 3.6)));
    add(
      GameObject(
        name: 'water',
        transform: Transform(position: Vec3(0, 0.35, 4.5), scale: Vec3(6, 1, 3)),
        components: [
          MeshRenderer(Mesh.plane(), Material(color: const Color(0xFF1F6F8B), opacity: 0.55, roughness: 0.05)),
        ],
      ),
    );
    for (var i = 0; i < 4; i++) {
      add(
        GameObject(
          name: 'ball $i',
          transform: Transform(
            position: Vec3(-2.1 + i * 1.4, 0.3, 4.5 + (i.isEven ? -0.5 : 0.5)),
            scale: Vec3.all(0.7),
          ),
          components: [MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFF2E3C6), roughness: 0.4))],
        ),
      );
    }

    // Sparks: their glow alone, added to what's behind.
    final spark = Material(
      color: Color.black,
      emissive: const Color(0xFFFF6A10),
      emissiveIntensity: 0.8,
      blending: Blending.additive,
    );
    for (var i = 0; i < 12; i++) {
      _sparks.add(
        add(
          GameObject(
            name: 'spark $i',
            transform: Transform(scale: Vec3.all(0.18)),
            components: [MeshRenderer(Mesh.sphere(), spark)],
          ),
        ),
      );
    }
    crowd = _startCrowd;
  }

  /// How see-through the glass is: its opacity, 0 to 1.
  double get opacity => _panes.first.get<MeshRenderer>()!.material!.opacity;
  set opacity(double value) {
    for (final pane in _panes) {
      pane.get<MeshRenderer>()!.material!.opacity = value;
    }
  }

  /// Whether the crowd of panes is there.
  bool get crowd => _crowd.isNotEmpty;
  set crowd(bool on) {
    if (on == crowd) return;
    if (!on) {
      for (final pane in _crowd) {
        pane.destroy();
      }
      _crowd.clear();
      return;
    }
    // Small panes in a block behind the cubes, every one see-through, all
    // of one material, so they're one draw call however they're sorted.
    final material = Material(color: tints[1], opacity: 0.25, roughness: 0.3, doubleSided: true);
    final random = math.Random(5);
    for (var i = 0; i < crowdSize; i++) {
      _crowd.add(
        add(
          GameObject(
            name: 'crowd $i',
            transform: Transform(
              position: Vec3(random.nextDouble() * 16 - 8, 0.5 + random.nextDouble() * 3, -3 - random.nextDouble() * 8),
              rotation:
                  Quat.axisAngle(Vec3.right, 90.deg) * Quat.axisAngle(Vec3.forward, random.nextDouble() * math.pi),
              scale: Vec3.all(0.6),
            ),
            components: [MeshRenderer(Mesh.plane(), material)],
          ),
        ),
      );
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final dt = time.unscaledDelta;
    if (_turning) _angle += 0.5 * dt;
    if (_swinging) _swing += 0.2 * dt;
    for (final (i, pane) in _panes.indexed) {
      pane.rotation = Quat.axisAngle(Vec3.up, _angle + i * 0.7) * Quat.axisAngle(Vec3.right, 90.deg);
    }
    for (final (i, spark) in _sparks.indexed) {
      final a = _angle * 1.3 + i / _sparks.length * 2 * math.pi;
      spark.position = Vec3(2.2 * math.cos(a), 1.1 + 0.25 * math.sin(3 * a), 4.5 + 1.1 * math.sin(a));
    }
    // Swinging round the scene, so the panes pass in front of each other.
    final at = Vec3(6 * math.sin(_swing), 3.5, 2 + 7 * math.cos(_swing));
    _camera
      ..position = at
      ..rotation = Quat.lookRotation(target - at);
    final picked = world.quality;
    final drawn = picked ?? Quality.device;
    readout.value =
        'drawn at ${drawn.name}${picked == null ? ' (the device picks)' : ''}   '
        'glass ${(opacity * 100).round()} % opaque${crowd ? '   crowd of $crowdSize' : ''}   $_fps fps';
  }

  @override
  List<Control> get controls => [
    for (final quality in Quality.levels) Button(quality.name, () => world.quality = quality),
    Button("device's", () => world.quality = null),
    Slider('glass opacity', () => opacity, (value) => opacity = value),
    Toggle('turn the glass', () => _turning, (on) => _turning = on),
    Toggle('swing', () => _swinging, (on) => _swinging = on),
    Toggle('crowd of $crowdSize panes', () => crowd, (on) => crowd = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('see-through', s));
}
