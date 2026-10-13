// Stage 7: the app's theme in the game. The lab's themes are grown from
// one colour (palette.dart); the scene takes its sky, floor, cubes and
// light from world.theme and follows it when it changes, so the switch
// that turns the lab light or dark turns the game with it.

import 'package:flutter/material.dart' show ThemeData, ThemeMode;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';
import '../palette.dart' as palette;

class ThemeBasicsScene extends LabScene {
  @override
  String get title => 'Theme: light and dark';
  @override
  String get step => 'stage 7';
  @override
  String get about =>
      'The lab\'s light and dark themes are grown from one colour. The scene takes its colours from '
      'world.theme: the sky is the theme\'s surface, the floor its outlineVariant, the cubes its primary, secondary and tertiary. Flip the '
      '"dark" switch: the whole lab changes, and the game fades with it.';

  late final Camera camera = find('camera')!.get<Camera>()!;
  late final Light sun = find('sun')!.get<Light>()!;
  late final Material floor = find('floor')!.get<MeshRenderer>()!.material!;
  final cubes = [for (var i = 0; i < 3; i++) Material(name: 'cube $i')];

  @override
  void build() {
    for (final (i, material) in cubes.indexed) {
      add(
        GameObject(
          name: 'cube $i',
          transform: Transform(position: Vec3((i - 1) * 1.6, 0.5, 0)),
          components: [MeshRenderer(Mesh.cube(), material)],
        ),
      );
    }
    wear(world.theme);
    on(world.onThemeChanged, wear);
  }

  /// Takes the scene's colours from [theme].
  void wear(ThemeData theme) {
    final scheme = theme.colorScheme;
    camera.clearColor = scheme.surface.toFynkel();
    floor.color = scheme.outlineVariant.toFynkel();
    for (final (i, c) in [scheme.primary, scheme.secondary, scheme.tertiary].indexed) {
      cubes[i].color = c.toFynkel();
    }
    sun.intensity = world.isDark ? 2.5 : 2;
    readout.value = '${world.isDark ? 'dark' : 'light'} · sky ${camera.clearColor}';
  }

  @override
  List<Control> get controls => [
    Toggle(
      'dark',
      () => palette.themeMode.value == ThemeMode.dark,
      (on) => palette.themeMode.value = on ? ThemeMode.dark : ThemeMode.light,
    ),
  ];
}
