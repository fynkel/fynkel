// What every scene of the lab has: a stage (camera, sun, floor), the
// controls the page shows for it, a readout and a log of what happened.

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show FynkelView, FynkelViewStats;
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

  /// What the page puts over the game, or null: Flutter widgets of the
  /// scene's own. Built with the page, when the scene is in its world
  /// already, so it can use [world].
  Widget? overlay(BuildContext context) => null;

  /// The game's view: one [FynkelView] of the scene, unless the scene shows
  /// it some other way (a split screen).
  Widget view(BuildContext context) => FynkelView(root: this);

  /// What happened, newest last: the scene's own events, and what goes to
  /// [World.errors].
  final log = Log();

  /// A few lines the scene keeps up to date, such as the clock's values.
  final readout = ValueNotifier('');

  /// The frame's parts and systems ([World.profile]), each the median of
  /// the last second's frames, for the page's profiler.
  final profile = ValueNotifier('');

  /// Where the camera stands and looks.
  Vec3 get eye => Vec3(0, 4, 9);
  Vec3 get target => Vec3(0, 0.5, 0);

  /// Adds the scene's own objects.
  void build();

  @override
  void onLoad() {
    super.onLoad();
    // Heard a little later: by then the object may be destroyed, and its
    // name gone with it.
    world.errors.listen((e) {
      final object = e.gameObject;
      final who = object == null ? 'the world' : (object.isAlive ? object.name : '$object');
      log.add('error in $who: ${e.error}', error: true);
    });
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
        components: [Light.sun(direction: Vec3(-0.4, -1, -0.5), intensity: 2.5)],
      ),
    );
    add(GameObject(name: 'ambient', components: [Light.ambient(intensity: 0.5)]));
    add(_Profiler(profile));
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

/// Reads [World.profile] each frame and, once a second, puts the medians
/// of each part and system into [out]: one frame's are too uneven to read.
class _Profiler extends GameObject {
  _Profiler(this.out) : super(name: 'profiler');

  final ValueNotifier<String> out;
  final _times = <String, List<int>>{};
  var _frames = 0;

  @override
  void onLateUpdate() {
    super.onLateUpdate();
    final p = world.profile;
    void add(String name, Duration time) => (_times[name] ??= []).add(time.inMicroseconds);
    add('the whole update', p.total);
    add('updates', p.updates);
    add('late updates', p.lateUpdates);
    add('fixed updates', p.fixedUpdates);
    add('starts', p.starts);
    add('input', p.input);
    add('motions', p.motions);
    add('meshes', p.meshes);
    add('debug', p.debug);
    add('the world\'s own', p.other);
    for (final s in p.systems) {
      add(s.name, s.time);
    }
    if (++_frames < 60) return;
    String ms(int micros) => (micros / 1000).toStringAsFixed(2).padLeft(6);
    out.value = [
      for (final MapEntry(:key, :value) in _times.entries)
        if (value.isNotEmpty) '${ms((value..sort())[value.length ~/ 2])} ms  $key',
    ].join('\n');
    _times.clear();
    _frames = 0;
  }
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

/// `--dart-define=LAB_RATES=true` prints each view's rates as [fps] works
/// them out, to read from `flutter run`.
const printRates = bool.fromEnvironment('LAB_RATES');

/// Frames shown per second in [stats]' window (drawn, for a view with
/// nowhere to show them), as a whole number; printed with [label] when
/// [printRates].
String fps(String label, FynkelViewStats stats) {
  final seconds = stats.window.inMicroseconds / 1e6;
  final shown = stats.view.presented > 0 ? stats.view.presented : stats.view.rendered;
  if (printRates) {
    // ignore: avoid_print
    print(
      '[rate] $label shown ${(shown / seconds).toStringAsFixed(1)} drawn '
      '${(stats.view.rendered / seconds).toStringAsFixed(1)} ticks ${(stats.ticks / seconds).toStringAsFixed(1)} '
      'size ${stats.size.width.round()}x${stats.size.height.round()}',
    );
  }
  return (shown / seconds).round().toString();
}
