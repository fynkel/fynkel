// Stage 10, phase 4: a whole style from a shader's own light. The Knight,
// the Rogue and a few shapes lit in flat bands (assets/shaders/toon.wgsl)
// and outlined, beside the same scene lit as the engine lights it.

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

class ToonScene extends LabScene {
  @override
  String get title => 'Look: toon';
  @override
  String get step => 'stage 10';
  @override
  String get about =>
      "A shader's light function lights its materials in place of the engine, one sun or lamp at a time: here in a "
      'few flat bands, with no highlight. Outlines are a material setting. Shadows, fog and the sky stay the '
      "engine's. Turn the style off to see the same scene as the engine lights it.";

  @override
  Vec3 get eye => Vec3(0, 2.2, 5.2);
  @override
  Vec3 get target => Vec3(0, 0.8, 0);

  Shader? toon;
  GameObject? knight, rogue;

  /// Each renderer of the style, with the material it had and its toon one.
  final _styled = <({MeshRenderer renderer, Material plain, Material toon})>[];
  var _on = true, _bands = 3.0, _outlines = true, _width = 0.012, _fps = '–';

  @override
  void build() {
    find('sun')!.get<Light>()!.shadows = true;
    find('floor')!.get<MeshRenderer>()!.material = Material(color: const Color(0xFF8FB070));
    for (final (name, mesh, color, at, size) in [
      ('ball', Mesh.sphere(), const Color(0xFFE05050), Vec3(-1.9, 0.45, 0.2), 0.9),
      ('box', Mesh.cube(), const Color(0xFF5080E0), Vec3(1.9, 0.4, -0.2), 0.8),
      ('pill', Mesh.capsule(), const Color(0xFFF0C040), Vec3(2.9, 0.5, 0.7), 0.5),
    ]) {
      add(shape(name, mesh, color, at: at, size: Vec3(size, size, size)));
    }
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    toon = await assets.shader('shaders/toon.wgsl');
    final (k, r) = await (assets.instantiate('kaykit/knight.glb'), assets.instantiate('kaykit/rogue.glb')).wait;
    add(k)
      ..position = Vec3(-0.6, 0, 0)
      ..rotation = Quat.axisAngle(Vec3.up, 0.3);
    add(r)
      ..position = Vec3(0.7, 0, -0.1)
      ..rotation = Quat.axisAngle(Vec3.up, -0.4);
    for (final extra in ['1H_Sword_Offhand', 'Badge_Shield', 'Rectangle_Shield', 'Spike_Shield', '2H_Sword']) {
      k.find(extra)?.active = false;
    }
    k.animator?.crossFade('Idle', seconds: 0);
    r.animator?.crossFade('Idle', seconds: 0);

    // One toon copy of each material, shared as the plain ones are.
    final copies = <Material, Material>{};
    void style(GameObject o) {
      if (o.get<MeshRenderer>() case final renderer? when renderer.material != null) {
        final plain = renderer.material!;
        final copy = copies[plain] ??= plain.copy(name: '${plain.name} toon')
          ..shader = toon
          ..set('bands', _bands);
        _styled.add((renderer: renderer, plain: plain, toon: copy));
      }
      o.children.forEach(style);
    }

    for (final o in [k, r, find('ball')!, find('box')!, find('pill')!]) {
      style(o);
    }
    _apply();
    knight = k;
    rogue = r;
  }

  void _apply() {
    final outline = _outlines ? Outline(width: _width, color: const Color(0xFF181410)) : null;
    for (final s in _styled) {
      s.toon.outline = outline;
      s.renderer.material = _on ? s.toon : s.plain;
    }
  }

  /// Whether the toon style is on, or the engine's own light.
  bool get styled => _on;
  set styled(bool value) {
    _on = value;
    _apply();
  }

  /// How many bands of light, 1 to 6.
  double get bands => _bands;
  set bands(double value) {
    _bands = value.roundToDouble();
    for (final m in {for (final s in _styled) s.toon}) {
      m.set('bands', _bands);
    }
  }

  /// Whether the style's materials are outlined, and how wide, in metres.
  bool get outlines => _outlines;
  set outlines(bool value) {
    _outlines = value;
    _apply();
  }

  double get width => _width;
  set width(double value) {
    _width = value;
    _apply();
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value =
        '${_on ? 'toon' : "the engine's light"}   ${_bands.round()} bands   '
        'outlines ${_outlines ? '${(_width * 1000).round()} mm' : 'off'}   $_fps fps';
  }

  @override
  List<Control> get controls => [
    Toggle('toon', () => styled, (value) => styled = value),
    Slider('bands', () => bands, (value) => bands = value, min: 1, max: 6),
    Toggle('outlines', () => outlines, (value) => outlines = value),
    Slider('outline width', () => width, (value) => width = value, min: 0.002, max: 0.04),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('toon', s));
}
