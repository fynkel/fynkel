// Step 5: input on objects. Unity's pointer handlers: hover, press,
// click (and double click), long press, bubbling up to the parent, and
// drags that keep their object wherever the finger goes.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

class PointersScene extends LabScene {
  @override
  String get title => 'Input on objects';
  @override
  String get step => 'step 5';
  @override
  String get about =>
      'The three pads on the table light up under the mouse (onPointerEnter/Exit), sink while pressed '
      '(onPointerDown/Up) and count clicks; a double click spins one, a long press turns it red. The grey pad '
      'takes nothing, so its clicks bubble up to the table. Drag a crate (a Draggable behaviour): it follows the '
      'finger over the floor, even off the crate, until you let go.';

  @override
  Vec3 get eye => Vec3(0, 6, 8);

  late final GameObject camera = find('camera')!;

  @override
  void build() {
    final table = add(Table());
    for (final (i, color) in [const Color(0xFFE0613A), const Color(0xFF7BC86C), const Color(0xFF4CA6E0)].indexed) {
      table.add(Pad('pad ${i + 1}', color, at: Vec3(i * 1.6 - 2.4, 0.35, 0)));
    }
    table.add(Pad('grey pad', const Color(0xFF8A8F99), at: Vec3(2.4, 0.35, 0), takes: false));
    for (final (i, x) in [-3.0, 0.0, 3.0].indexed) {
      add(
        shape(
          'crate ${i + 1}',
          Mesh.cube(),
          const Color(0xFFC08A50),
          at: Vec3(x, 0.4, 2.5),
          size: Vec3(0.8, 0.8, 0.8),
          components: [Draggable()],
        ),
      );
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = [
      if (input.fingers.isEmpty) 'no fingers',
      for (final f in input.fingers)
        '${f.kind.name} ${f.id} over ${f.over?.object.name ?? 'nothing'}'
            '${f.isDown ? ', started over ${f.startedOver?.object.name ?? 'nothing'}' : ''}',
      'clicks: ${world.all<Pad>().map((p) => p.clicks).join(' · ')}   table: ${world.only<Table>().clicks}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [Button('clear log', log.clear)];
}

/// Takes the clicks its pads pass on.
class Table extends GameObject {
  Table() : super(name: 'table');

  var clicks = 0;

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        name: 'top',
        transform: Transform(position: Vec3(0, 0.1, 0), scale: Vec3(8, 0.2, 2)),
        components: [MeshRenderer(Mesh.cube(), own(Material(color: const Color(0xFF5A4A3A))))],
      ),
    );
  }

  @override
  bool onClick(Finger finger) {
    clicks++;
    ancestor<LabScene>()?.log.add('table: a click on ${finger.over?.object.name} bubbled up to me');
    return true;
  }
}

/// A pad on the table: lights under the pointer, sinks while pressed,
/// counts clicks, spins on a double click and goes red on a long press.
class Pad extends GameObject {
  Pad(String name, this.color, {required this.at, this.takes = true}) : super(name: name);

  final Color color;
  final Vec3 at;

  /// Whether it takes its clicks, or lets them bubble to the table.
  final bool takes;

  var clicks = 0;
  late final Material _paint;
  var _over = 0;
  var _pressed = false;
  double _spin = 0;

  LabScene get _scene => ancestor<LabScene>()!;

  @override
  void onLoad() {
    super.onLoad();
    _paint = own(Material(color: color));
    addComponent(MeshRenderer(Mesh.cube(), _paint));
    _place();
  }

  void _place() => transform = Transform(
    position: at - Vec3(0, _pressed ? 0.1 : 0, 0),
    rotation: Quat.axisAngle(Vec3.up, _spin),
    scale: Vec3(1.2, _pressed ? 0.3 : 0.5, 1.2),
  );

  @override
  void onPointerEnter(Finger finger) {
    if (_over++ == 0) _paint.emissive = Color.rgb(color.r * 0.5, color.g * 0.5, color.b * 0.5);
  }

  @override
  void onPointerExit(Finger finger) {
    if (--_over == 0) _paint.emissive = Color.black;
  }

  @override
  bool onPointerDown(Finger finger) {
    if (!takes) return false;
    _pressed = true;
    _place();
    return true;
  }

  @override
  void onPointerUp(Finger finger) {
    _pressed = false;
    _place();
  }

  @override
  bool onClick(Finger finger) {
    if (!takes) {
      _scene.log.add('$name: not mine, passed on');
      return false;
    }
    clicks++;
    _scene.log.add('$name: click ×${finger.tapCount}');
    if (finger.tapCount == 2) _spinOnce();
    return true;
  }

  @override
  bool onLongPress(Finger finger) {
    if (!takes) return false;
    _scene.log.add('$name: long press');
    _paint.color = const Color(0xFFD04848);
    start(() async {
      await wait(1);
      _paint.color = color;
    });
    return true;
  }

  void _spinOnce() {
    start(() async {
      for (var t = 0.0; t < 0.5; t += time.delta) {
        _spin = t / 0.5 * 2 * math.pi;
        _place();
        await nextFrame;
      }
      _spin = 0;
      _place();
    });
  }
}

/// Drags its object over the floor: it takes the drag's start, so the
/// rest of the drag is its own, wherever the finger goes.
class Draggable extends Behaviour {
  double _height = 0;

  @override
  bool onDragStart(Finger finger) {
    _height = gameObject.transform.position.y;
    ancestor()?.log.add('${gameObject.name}: drag taken');
    return true;
  }

  @override
  void onDrag(Finger finger) {
    final scene = ancestor()!;
    final ray = scene.camera.rayThrough(finger.position);
    // Where the ray meets the plane at the crate's height.
    if (ray.direction.y.abs() < 1e-6) return;
    final t = (_height - ray.origin.y) / ray.direction.y;
    if (t <= 0) return;
    final at = ray.origin + ray.direction * t;
    gameObject.transform = gameObject.transform.copyWith(
      position: Vec3(at.x.clamp(-6.0, 6.0), _height, at.z.clamp(-4.0, 5.0)),
    );
  }

  @override
  void onDragEnd(Finger finger) => ancestor()?.log.add(
    '${gameObject.name}: dropped, ${finger.over?.object == gameObject ? 'under' : 'not under'} the finger',
  );

  PointersScene? ancestor() => gameObject.ancestor<PointersScene>();
}
