// Stage 7: rebinding a key, the basics. The button over the game asks for
// a key (keyCodeOf turns Flutter's into the game's) and binds 'jump' to
// it. Binding again changes the same action, so the cube's code, which
// asks input.action('jump') each frame, needs nothing new.

import 'package:flutter/material.dart' hide Color, Material, Transform;
import 'package:flutter/services.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

class RebindingBasicsScene extends LabScene {
  @override
  String get title => 'Rebinding: the basics';
  @override
  String get step => 'stage 7';
  @override
  String get about =>
      'Space makes the cube hop. Tap the button at the top right and press another key: now that one '
      'does. The game pauses while the dialog asks; Escape keeps the key it had.';

  late final GameObject cube;
  late final InputAction jump;
  final key = ValueNotifier(KeyCode.space);
  double _rise = 0;

  @override
  void build() {
    cube = add(shape('cube', Mesh.cube(), const Color(0xFF4CA6E0), at: Vec3(0, 0.5, 0)));
    jump = input.bindAction('jump', [key.value]);
    on(key.changed, (_) => input.bindAction('jump', [key.value]));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (jump.wasPressed && cube.position.y <= 0.5) {
      _rise = 5;
      log.add('hop (${key.value.label})');
    }
    _rise -= 15 * time.delta;
    final y = cube.position.y + _rise * time.delta;
    cube.position = Vec3(0, y < 0.5 ? 0.5 : y, 0);
    readout.value = "'jump' is ${jump.buttons.map((b) => b is KeyCode ? b.label : '$b').join(', ')}";
  }

  @override
  List<Control> get controls => const [];

  @override
  Widget overlay(BuildContext context) => Align(
    alignment: Alignment.topRight,
    child: Padding(
      padding: const EdgeInsets.all(16),
      child: ValueListenableBuilder(
        valueListenable: key,
        builder: (context, now, _) => FilledButton(onPressed: () => _ask(context), child: Text('Jump: ${now.label}')),
      ),
    ),
  );

  Future<void> _ask(BuildContext context) async {
    final chosen = await showDialog<KeyCode>(
      context: context,
      builder: (context) => Focus(
        autofocus: true,
        onKeyEvent: (_, event) {
          if (event is KeyDownEvent) {
            Navigator.of(context).pop(event.logicalKey == LogicalKeyboardKey.escape ? null : keyCodeOf(event));
          }
          return KeyEventResult.handled;
        },
        child: const AlertDialog(title: Text('Press a key for jump')),
      ),
    );
    if (chosen != null) key.value = chosen;
  }
}
