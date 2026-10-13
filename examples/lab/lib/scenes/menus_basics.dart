// Stage 7: menus over the game, the basics. A dialog or a page over the
// game pauses it by itself (FynkelView's pauseWhenCovered), and closing
// the last one goes on: the scene has no pause code, it only hears the
// world pause and go on (world.onPausedChanged).

import 'package:flutter/material.dart' hide Color, Material, Transform;
import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class MenusBasicsScene extends LabScene {
  @override
  String get title => 'Menus: the basics';
  @override
  String get step => 'stage 7';
  @override
  String get about =>
      'Tap Menu: a dialog opens over the game, and the cube stops, as the game is paused while anything '
      'covers it. From the dialog, open a page: still paused. Close them both and it goes on. The scene '
      'has no pause code of its own; the log shows the world pausing and going on.';

  late final GameObject cube;

  @override
  void build() {
    cube = add(shape('cube', Mesh.cube(), const Color(0xFF4CA6E0), at: Vec3(0, 0.5, 0)));
    on(world.onPausedChanged, (paused) => log.add(paused ? 'paused' : 'going on'));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    cube.rotate(Vec3.up, time.delta);
    readout.value = 'game time ${time.sinceStart.toStringAsFixed(1)} s';
  }

  @override
  List<Control> get controls => const [];

  @override
  Widget overlay(BuildContext context) => Align(
    alignment: Alignment.topRight,
    child: Padding(
      padding: const EdgeInsets.all(16),
      child: FilledButton(onPressed: () => _menu(context), child: const Text('Menu')),
    ),
  );

  void _menu(BuildContext context) => showDialog<void>(
    context: context,
    builder: (context) => SimpleDialog(
      title: const Text('Paused'),
      children: [
        SimpleDialogOption(onPressed: Navigator.of(context).pop, child: const Text('Resume')),
        SimpleDialogOption(
          onPressed: () => Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => Scaffold(
                appBar: AppBar(title: const Text('A page')),
                body: const Center(child: Text('The game is still paused, behind the dialog.')),
              ),
            ),
          ),
          child: const Text('Open a page'),
        ),
      ],
    ),
  );
}
