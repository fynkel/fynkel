// Stage 10, phase 4: screen effects, shaders run over the camera's whole
// picture, in order (assets/shaders/): an old TV, a four-colour palette
// and a hit flash, over the pixel look or the view's own size. Phase 4b:
// a colour grade from a table made here, and a scanner's pulse that reads
// the scene's depth.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

class EffectsScene extends LabScene {
  @override
  String get title => 'Look: screen effects';
  @override
  String get step => 'stage 10';
  @override
  String get about =>
      "A camera's effects are shaders run over its finished picture, in order, each over what the one before made: "
      'an old TV, four colours as an old handheld, a red flash when hit. Swap the TV and the palette to see the '
      'order matter. The colour grade looks each colour up in a table (teal shadows, warm light); the scanner '
      "reads how far the scene is at each pixel. They work at every quality level; a frame without any pays "
      'nothing for them.';

  @override
  Vec3 get eye => Vec3(0, 2.4, 5.6);
  @override
  Vec3 get target => Vec3(0, 0.7, 0);

  ScreenEffect? tv, palette, hit, grade, scanner;
  GameObject? rogue;
  var _tv = true, _palette = false, _tvFirst = false, _pixels = false, _grade = false, _scan = false;
  var _hits = 0, _fps = '–';

  Camera get camera => find('camera')!.get<Camera>()!;

  @override
  void build() {
    find('sun')!.get<Light>()!.shadows = true;
    find('floor')!.get<MeshRenderer>()!.material = Material(color: const Color(0xFF6A8A5A));
    for (final (i, color) in [
      const Color(0xFFE05050),
      const Color(0xFF50A0E0),
      const Color(0xFFF0C040),
      const Color(0xFFA060D0),
    ].indexed) {
      add(shape('box $i', Mesh.cube(), color, at: Vec3(-2.4 + i * 1.6, 0.35, -1.6), size: Vec3(0.7, 0.7, 0.7)));
    }
    add(shape('ball', Mesh.sphere(), const Color(0xFFF0F0F0), at: Vec3(1.6, 0.4, 0.6), size: Vec3(0.8, 0.8, 0.8)));
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    final (tvShader, paletteShader, hitShader, gradeShader, scannerShader, r) = await (
      assets.shader('shaders/old_tv.wgsl'),
      assets.shader('shaders/palette.wgsl'),
      assets.shader('shaders/hit.wgsl'),
      assets.shader('shaders/grade.wgsl'),
      assets.shader('shaders/scanner.wgsl'),
      assets.instantiate('kaykit/rogue.glb'),
    ).wait;
    grade = ScreenEffect(gradeShader, values: {'table': tealAndOrange(), 'amount': 1.0});
    scanner = ScreenEffect(scannerShader, values: {'color': const Color(0xFF40E0FF), 'speed': 6.0, 'every': 3.0});
    tv = ScreenEffect(tvShader, values: {'lines': 160.0, 'curve': 0.25});
    palette = ScreenEffect(
      paletteShader,
      values: {
        'darkest': const Color(0xFF0F380F),
        'dark': const Color(0xFF306230),
        'light': const Color(0xFF8BAC0F),
        'lightest': const Color(0xFFE0F8D0),
      },
    );
    hit = ScreenEffect(hitShader, values: {'amount': 0.0});
    add(r)
      ..position = Vec3(-0.3, 0, 0.2)
      ..rotation = Quat.axisAngle(Vec3.up, 0.3);
    r.animator?.crossFade('Idle', duration: 0);
    rogue = r;
    _apply();
  }

  /// The camera's effects as the switches say: the palette and the TV in
  /// the order asked, the hit flash last, there while it's flashing.
  void _apply() {
    final (tv, palette, hit, grade, scanner) = (this.tv, this.palette, this.hit, this.grade, this.scanner);
    if (tv == null || palette == null || hit == null || grade == null || scanner == null) return;
    final both = [if (_palette) palette, if (_tv) tv];
    camera
      ..effects = [
        if (_scan) scanner,
        if (_grade) grade,
        ...(_tvFirst ? both.reversed : both),
        if ((hit.value('amount')! as double) > 0) hit,
      ]
      ..pixels = _pixels ? 160 : null;
  }

  /// A colour grade's table, 16 tiles of 16 × 16 side by side (red across
  /// a tile, green down it, blue from tile to tile): shadows towards teal,
  /// light towards orange, a little more contrast.
  static Texture2D tealAndOrange() {
    const n = 16;
    final pixels = Uint8List(n * n * n * 4);
    for (var g = 0; g < n; g++) {
      for (var x = 0; x < n * n; x++) {
        final c = [x % n / (n - 1), g / (n - 1), x ~/ n / (n - 1)];
        final l = 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
        final shade = l < 0.5 ? [-0.10, 0.02, 0.12] : [0.14, 0.04, -0.10];
        final by = (l - 0.5).abs() * 2;
        final at = (g * n * n + x) * 4;
        for (var i = 0; i < 3; i++) {
          final graded = ((c[i] + shade[i] * by - 0.5) * 1.1 + 0.5).clamp(0.0, 1.0);
          pixels[at + i] = (graded * 255).round();
        }
        pixels[at + 3] = 255;
      }
    }
    return Texture2D.rgba(n * n, n, pixels);
  }

  bool get oldTv => _tv;
  set oldTv(bool on) {
    _tv = on;
    _apply();
  }

  bool get fourColours => _palette;
  set fourColours(bool on) {
    _palette = on;
    _apply();
  }

  /// Whether the TV runs before the palette, rather than after it.
  bool get tvFirst => _tvFirst;
  set tvFirst(bool on) {
    _tvFirst = on;
    _apply();
  }

  bool get colourGrade => _grade;
  set colourGrade(bool on) {
    _grade = on;
    _apply();
  }

  bool get scanning => _scan;
  set scanning(bool on) {
    _scan = on;
    _apply();
  }

  /// Whether the camera has the pixel look, 160 pixels high.
  bool get pixelLook => _pixels;
  set pixelLook(bool on) {
    _pixels = on;
    _apply();
  }

  /// A hit: the flash comes and fades over half a second.
  void strike() {
    final hit = this.hit;
    if (hit == null) return;
    _hits++;
    hit.set('amount', 1.0);
    _apply();
    tween(1.0, 0.0, seconds: 0.5, ease: Ease.outQuad, onUpdate: (v) => hit.set('amount', v), onComplete: _apply);
    log.add('hit');
  }

  @override
  void onUpdate() {
    super.onUpdate();
    // The camera circles slowly, so the effects are seen over a moving
    // picture.
    final a = time.sinceStart * 0.15;
    final eye = Vec3(5.6 * math.sin(a), 2.4, 5.6 * math.cos(a));
    find('camera')!
      ..position = eye
      ..rotation = Quat.lookRotation(target - eye);
    readout.value =
        '${camera.effects.isEmpty ? 'no effects' : camera.effects.map((e) => e.shader.name.split('/').last).join(' → ')}'
        '   hits $_hits   $_fps fps';
  }

  @override
  List<Control> get controls => [
    Button('hit', strike),
    Toggle('old TV', () => oldTv, (on) => oldTv = on),
    Toggle('four colours', () => fourColours, (on) => fourColours = on),
    Toggle('TV before the palette', () => tvFirst, (on) => tvFirst = on),
    Toggle('pixel look', () => pixelLook, (on) => pixelLook = on),
    Toggle('colour grade', () => colourGrade, (on) => colourGrade = on),
    Toggle('scanner', () => scanning, (on) => scanning = on),
  ];

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('effects', s));
}
