// The front page's second film: one scene drawn in four styles, each an
// outline, a material's shader or an effect over the whole picture.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import 'showcase.dart';

/// How long each style stays, in seconds, and all of them.
const styleSeconds = 2.5, looksLength = styleSeconds * 4;

class Looks extends Showcase {
  late final GameObject rogue;
  late final Shader _toon, _palette, _tv;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    _toon = await assets.shader('shaders/toon.wgsl');
    _palette = await assets.shader('shaders/palette.wgsl');
    _tv = await assets.shader('shaders/old_tv.wgsl');
    rogue = add(await assets.instantiate('kaykit/rogue.glb'));
    for (final extra in ['1H_Crossbow', '2H_Crossbow', 'Throwable']) {
      rogue.find(extra)?.active = false;
    }
    knight
      ..position = Vec3(-1.2, 0, 0)
      ..rotation = Quat.axisAngle(Vec3.up, 1.15);
    rogue
      ..position = Vec3(1.2, 0, 0)
      ..rotation = Quat.axisAngle(Vec3.up, -1.15);
    dusk(1);
    _fight(knight.animator!, '2H_Melee_Attack_Chop');
    _fight(rogue.animator!, 'Dualwield_Melee_Attack_Slice');
  }

  /// Swings again and again.
  void _fight(Animator animator, String clip) => animator.crossFade(clip, seconds: 0.2);

  @override
  Future<void> play() async {
    await until(styleSeconds);
    _styled(this, {});
    light.intensity *= 1.8; // a cartoon's lit side is one flat band
    ambient.intensity = 0.5;
    await until(styleSeconds * 2);
    ambient.intensity = 0.6; // the greys between the palette's steps
    camera
      ..pixels = 180
      ..effects = [
        ScreenEffect(
          _palette,
          values: {
            'darkest': const Color(0xFF1F1233),
            'dark': const Color(0xFF6B2D5C),
            'light': const Color(0xFFE8895A),
            'lightest': const Color(0xFFFFE9B8),
          },
        ),
      ];
    await until(styleSeconds * 3);
    light.intensity *= 1.2;
    ambient.intensity = 0.8;
    camera
      ..pixels = null
      ..effects = [
        ScreenEffect(_tv, values: {'lines': 180.0, 'curve': 0.1}),
      ];
  }

  /// Every material in [object] and under it, as a cartoon's.
  void _styled(GameObject object, Map<Material, Material> copies) {
    if (object.get<MeshRenderer>() case final renderer? when renderer.material != null) {
      final plain = renderer.material!;
      renderer.material = copies[plain] ??= plain.copy()
        ..shader = _toon
        ..set('bands', 3.0)
        ..outline = const Outline(width: 0.016, color: Color(0xFF181020));
    }
    for (final child in object.children) {
      _styled(child, copies);
    }
  }

  @override
  void look() {
    final a = 0.25 * math.sin(at / looksLength * 2 * math.pi);
    final from = Vec3(6.6 * math.sin(a), 2.1, 6.6 * math.cos(a));
    eye
      ..position = from
      ..rotation = Quat.lookRotation(Vec3(0, 1.05, 0) - from);
  }
}
