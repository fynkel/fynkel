// Stage 15 phase 2: flat text in the scene. Signs drawn as TextMeshPro
// draws them, from the font's atlas of distances: an EXIT sign with an
// outline on a wall, START painted on the ground, a shop sign lit by the
// sun, names standing over walkers. Hitting the dummy pops numbers over
// everything; "200 numbers" changes 200 every frame, for the profiler.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level; with `LAB_RATES`, its load starts on and its update times
/// print.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// A number that pops up from a hit, rises and fades.
class PopNumber extends GameObject {
  PopNumber(String text, Font font, Vec3 at, {bool critical = false})
    : _color = critical ? const Color(0xFFFF5040) : const Color(0xFFFFE070),
      super(
        name: 'number',
        transform: Transform(position: at),
      ) {
    addComponent(
      TextRenderer(
        text,
        font: font,
        size: critical ? 0.6 : 0.4,
        color: _color,
        outline: 0.35,
        shadowColor: const Color.rgb(0, 0, 0, 0.6),
        shadowOffset: Vec2(0.06, -0.08),
        facing: Facing.camera,
        onTop: true,
      ),
    );
  }

  final Color _color;
  static const life = 1.0;
  double age = 0;
  late final Vec3 _drift = Vec3(_random.nextDouble() - 0.5, 1.6, 0);
  static final _random = math.Random(7);

  @override
  void onUpdate() {
    super.onUpdate();
    age += time.delta;
    if (age >= life) {
      destroy();
      return;
    }
    position = position + _drift * time.delta;
    // Holds, then fades over the last half.
    final fade = (2 - 2 * age / life).clamp(0.0, 1.0);
    get<TextRenderer>()!.color = _color.withAlpha(fade);
  }
}

/// What the numbers pop from: click or tap it.
class Dummy extends GameObject {
  Dummy(this.hit)
    : super(
        name: 'dummy',
        transform: Transform(position: Vec3(2.5, 1, 0.5)),
      );

  final void Function() hit;

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(Mesh.capsule(), Material(color: const Color(0xFFC08850))));
  }

  @override
  bool onPointerDown(Finger finger) {
    hit();
    return true;
  }
}

/// A cube going round, its name standing over it, facing the camera.
class NamedWalker extends GameObject {
  NamedWalker(String name, Font font, this._angle, this.speed)
    : super(
        name: name,
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF4CA6E0)))],
      ) {
    add(
      GameObject(
        name: 'name',
        transform: Transform(position: Vec3(0, 0.75, 0)),
        components: [
          TextRenderer(name, font: font, size: 0.3, facing: Facing.upright, shadowColor: const Color.rgb(0, 0, 0, 0.5)),
        ],
      ),
    );
  }

  double _angle;
  final double speed;

  @override
  void onUpdate() {
    super.onUpdate();
    _angle += speed * time.delta;
    position = Vec3(-2.5 + 1.5 * math.sin(_angle), 0.5, 1 + 1.5 * math.cos(_angle));
  }
}

class FlatTextScene extends LabScene {
  @override
  String get title => 'Text in the world';
  @override
  String get step => 'stage 15';
  @override
  String get about =>
      'Flat text (TextRenderer), crisp close up and still read far off: EXIT with an outline on the wall, START '
      'on the ground (Facing.up), a shop sign lit by the sun (lit), names standing over the walkers '
      '(Facing.upright, a shadow). Click the dummy, or let it be hit by itself: numbers pop over everything '
      '(onTop). "200 numbers" changes 200 every frame: watch the profiler.';

  @override
  Vec3 get eye => Vec3(0, 3, 9);
  @override
  Vec3 get target => Vec3(0, 1, 0);

  late final Font font;
  final grid = <TextRenderer>[];
  late final GameObject numbers;
  var hitting = true;
  var _sinceHit = 0.0;
  var _frame = 0;
  // The font is in and the texts made.
  var _ready = false;
  final _random = math.Random(3);

  // The grid's numbers, made once: a game's formatting is its own garbage.
  static final _values = [for (var i = 0; i < 997; i++) i.isEven ? '${i * 31 % 1000}' : '+${i * 7}'];

  @override
  void build() {
    add(shape('wall', Mesh.cube(), const Color(0xFF6A6F78), at: Vec3(0, 1.5, -2), size: Vec3(5, 3, 0.2)));
    add(shape('post', Mesh.cylinder(), const Color(0xFF806040), at: Vec3(-4, 1, -0.5), size: Vec3(0.12, 2, 0.12)));
    add(dummy = Dummy(hit));
  }

  late final Dummy dummy;

  @override
  Future<void> onLoad() async {
    super.onLoad();
    font = await world.assets.font('fonts/Roboto-Regular.ttf');
    add(
      GameObject(
        name: 'exit sign',
        transform: Transform(position: Vec3(0, 2.2, -1.88)),
        components: [
          TextRenderer(
            'EXIT',
            font: font,
            size: 1,
            color: const Color(0xFFE02020),
            outline: 0.5,
            outlineColor: const Color(0xFFFFE040),
          ),
        ],
      ),
    );
    add(
      GameObject(
        name: 'small print',
        transform: Transform(position: Vec3(0, 1.1, -1.88)),
        components: [
          TextRenderer(
            'Keep this door clear at all times.\nNo smoking past this point.',
            font: font,
            size: 0.16,
            color: const Color(0xFFE8E8E8),
            lineSpacing: 1.2,
          ),
        ],
      ),
    );
    add(
      GameObject(
        name: 'ground',
        transform: Transform(position: Vec3(0.5, 0.01, 2.5)),
        components: [
          TextRenderer(
            'START',
            font: font,
            size: 0.9,
            color: const Color.rgb(1, 1, 1, 0.8),
            softness: 0.15,
            facing: Facing.up,
          ),
        ],
      ),
    );
    add(
      GameObject(
        name: 'shop sign',
        transform: Transform(position: Vec3(-4, 2.2, -0.4), rotation: Quat.axisAngle(Vec3.up, 0.5)),
        components: [
          TextRenderer('SHOP', font: font, size: 0.45, color: const Color(0xFFF0D8A0), outline: 0.3, lit: true),
        ],
      ),
    );
    for (var i = 0; i < 3; i++) {
      add(NamedWalker(['Ada', 'Bo', 'Cy'][i], font, i * 2 * math.pi / 3, 0.6));
    }
    numbers = add(GameObject(name: '200 numbers'))..active = false;
    for (var i = 0; i < 200; i++) {
      final text = TextRenderer('0', font: font, size: 0.17, facing: Facing.camera, color: const Color(0xFF90F0A0));
      grid.add(text);
      numbers.add(
        GameObject(
          transform: Transform(position: Vec3((i % 20 - 9.5) * 0.5, 3.4 + (i ~/ 20) * 0.27, 0)),
          components: [text],
        ),
      );
    }
    if (_startQuality.isNotEmpty) world.quality = Quality.levels.firstWhere((q) => q.name == _startQuality);
    if (printRates) numbers.active = true;
    _ready = true;
  }

  /// A hit on the dummy: a number pops from it.
  void hit() {
    if (!_ready) return;
    final critical = _random.nextDouble() < 0.15;
    final damage = critical ? 80 + _random.nextInt(60) : 5 + _random.nextInt(40);
    add(PopNumber(critical ? '$damage!' : '-$damage', font, dummy.position + Vec3(0, 1.2, 0), critical: critical));
    log.add('hit for $damage${critical ? ', critical' : ''}');
  }

  final _times = UpdateTimes('200 numbers');

  @override
  void onUpdate() {
    super.onUpdate();
    if (!_ready) return;
    _times.add(world);
    if (hitting) {
      _sinceHit += time.delta;
      if (_sinceHit > 0.35) {
        _sinceHit = 0;
        hit();
      }
    }
    if (numbers.active) {
      _frame++;
      for (var i = 0; i < grid.length; i++) {
        grid[i].text = _values[(_frame * 13 + i) % _values.length];
      }
    }
    final popping = children.whereType<PopNumber>().length;
    readout.value = '$popping numbers popping${numbers.active ? ', 200 changed every frame' : ''}';
  }

  @override
  List<Control> get controls => [
    Toggle('hit by itself', () => hitting, (on) => hitting = on),
    Toggle('200 numbers', () => numbers.active, (on) => numbers.active = on),
  ];
}
