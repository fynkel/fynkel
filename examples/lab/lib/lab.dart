// What every scene of the lab has: a stage (camera, sun, floor), the
// controls the page shows for it, a readout and a log of what happened.

import 'package:flutter/foundation.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// A scene of the lab: one feature, to try by hand. Subclasses add their
/// objects in [build] and say what can be done with [controls].
abstract class LabScene extends GameObject {
  LabScene() : super(name: 'scene');

  /// The scene's name in the list, and which step of docs/stage-4.md it
  /// shows.
  String get title;
  String get step;

  /// What to try, in a sentence or two.
  String get about;

  /// What the page shows as buttons, switches and sliders.
  List<Control> get controls;

  /// What happened, newest last: the scene's own events, and what goes to
  /// [World.errors].
  final log = Log();

  /// A few lines the scene keeps up to date, such as the clock's values.
  final readout = ValueNotifier('');

  /// Where the camera stands and looks.
  Vec3 get eye => Vec3(0, 4, 9);
  Vec3 get target => Vec3(0, 0.5, 0);

  /// Adds the scene's own objects.
  void build();

  @override
  void onLoad() {
    super.onLoad();
    world.errors.listen((e) => log.add('error in ${e.gameObject?.name ?? e.gameObject}: ${e.error}', error: true));
    add(
      GameObject(
        name: 'camera',
        transform: Transform(position: eye, rotation: Quat.lookRotation(target - eye)),
        components: [Camera(clearColor: const Color(0xFF20242C))],
      ),
    );
    add(
      GameObject(
        name: 'sun',
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.5))),
        components: [Light.sun(intensity: 2.5)],
      ),
    );
    add(GameObject(name: 'ambient', components: [Light.ambient(intensity: 0.5)]));
    add(
      GameObject(
        name: 'floor',
        transform: Transform(scale: Vec3(20, 1, 20)),
        components: [MeshRenderer(Mesh.plane(), Material(color: const Color(0xFF3A3F4A)))],
      ),
    );
    build();
  }

  /// A box collider under the floor, 20 × 20 like it, for physics scenes.
  GameObject floorCollider() => GameObject(
    name: 'floor collider',
    transform: Transform(position: Vec3(0, -0.5, 0)),
    components: [BoxCollider(size: Vec3(20, 1, 20))],
  );

  /// A new object with a [mesh] in [color]: the scenes' building block.
  GameObject shape(
    String name,
    Mesh mesh,
    Color color, {
    Vec3 at = Vec3.zero,
    Vec3 size = Vec3.one,
    Iterable<Component> components = const [],
    Iterable<GameObject> children = const [],
  }) => GameObject(
    name: name,
    transform: Transform(position: at, scale: size),
    components: [
      MeshRenderer(mesh, Material(color: color)),
      ...components,
    ],
    children: children,
  );
}

/// One line of a scene's [LabScene.log].
typedef LogLine = ({String text, bool error});

/// A scene's log: the last 200 lines.
class Log extends ValueNotifier<List<LogLine>> {
  Log() : super(const []);

  void add(String text, {bool error = false}) {
    final lines = [...value, (text: text, error: error)];
    value = lines.length > 200 ? lines.sublist(lines.length - 200) : lines;
  }

  void clear() => value = const [];
}

/// Something the page shows for a scene to drive it with.
sealed class Control {
  const Control(this.label);
  final String label;
}

/// A button: [onPressed] when it's pressed.
final class Button extends Control {
  const Button(super.label, this.onPressed);
  final void Function() onPressed;
}

/// A switch, showing [value] and calling [onChanged].
final class Toggle extends Control {
  const Toggle(super.label, this.value, this.onChanged);
  final bool Function() value;
  final void Function(bool on) onChanged;
}

/// A slider from [min] to [max], showing [value] and calling [onChanged].
final class Slider extends Control {
  const Slider(super.label, this.value, this.onChanged, {this.min = 0, this.max = 1});
  final double Function() value;
  final void Function(double value) onChanged;
  final double min, max;
}

/// Seconds as the log shows them.
String secs(double s) => '${s.toStringAsFixed(2)} s';
