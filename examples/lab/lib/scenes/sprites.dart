// Stage 15 phase 3: pictures in the scene. A speech bubble that's a
// 9-sliced sprite, sized to its words; rings pulsing on the ground; a
// poster drawn from a Flutter widget, drawn again on a press; three cards
// drawn by their order, not their distance; and "1,000 sprites" of one
// texture swirling, one draw, for the profiler.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/material.dart' as flutter;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level; with `LAB_RATES`, its load starts on and its update times
/// print.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// [size] × [size] pixels of [rgba] for each pixel's distance from the
/// middle, 0 there to 1 at the edge's middle.
Texture2D round(int size, List<int> Function(double distance) rgba) {
  final pixels = Uint8List(size * size * 4);
  for (var y = 0; y < size; y++) {
    for (var x = 0; x < size; x++) {
      final d = math.sqrt(math.pow(x + 0.5 - size / 2, 2) + math.pow(y + 0.5 - size / 2, 2)) / (size / 2);
      pixels.setAll((y * size + x) * 4, rgba(d));
    }
  }
  return Texture2D.rgba(size, size, pixels);
}

/// Draws the bubble's texture: a white rounded box with a dark border, 48
/// pixels, its corners 16 in, for 9-slice.
Future<Texture2D> bubbleTexture() => flutter.DecoratedBox(
  decoration: flutter.BoxDecoration(
    color: const flutter.Color(0xFFFFFFFF),
    border: flutter.Border.all(color: const flutter.Color(0xFF203040), width: 3),
    borderRadius: flutter.BorderRadius.circular(14),
  ),
).toTexture(size: const flutter.Size(48, 48));

/// A poster for the wall, as the game's UI would draw it.
flutter.Widget poster(int visits) => flutter.Container(
  decoration: flutter.BoxDecoration(
    gradient: const flutter.LinearGradient(
      colors: [flutter.Color(0xFF3050A0), flutter.Color(0xFF8040A0)],
      begin: flutter.Alignment.topLeft,
      end: flutter.Alignment.bottomRight,
    ),
    borderRadius: flutter.BorderRadius.circular(12),
  ),
  padding: const flutter.EdgeInsets.all(12),
  child: flutter.Column(
    mainAxisAlignment: flutter.MainAxisAlignment.center,
    children: [
      const flutter.Icon(flutter.Icons.local_cafe, color: flutter.Color(0xFFFFE070), size: 40),
      const flutter.Text(
        'CAFÉ FYNKEL',
        style: flutter.TextStyle(color: flutter.Color(0xFFFFFFFF), fontSize: 22, fontWeight: flutter.FontWeight.bold),
      ),
      flutter.Text(
        'drawn from a widget · visit $visits',
        style: const flutter.TextStyle(color: flutter.Color(0xFFE0E0FF), fontSize: 12),
      ),
    ],
  ),
);

class SpritesScene extends LabScene {
  @override
  String get title => 'Pictures in the world';
  @override
  String get step => 'stage 15';
  @override
  String get about =>
      'SpriteRenderer: the speech bubble is one 48-pixel texture 9-sliced (slice), its corners kept as it grows to '
      'fit its words (font.measure); rings pulse on the ground (Facing.up); the poster is a Flutter widget drawn '
      'once into a texture (widget.toTexture), drawn again on "redraw the poster". The three cards are drawn by '
      'their order, not by how far they are: "blue on top" changes it. "1,000 sprites" swirls a thousand of one '
      'texture, one draw.';

  @override
  Vec3 get eye => Vec3(0, 3, 9);
  @override
  Vec3 get target => Vec3(0, 1.2, 0);

  late final Font font;
  late final Texture2D bubble, ring, dot, card;
  late final SpriteRenderer bubbleSprite, posterSprite;
  late final TextRenderer words;
  final rings = <SpriteRenderer>[];
  final cards = <SpriteRenderer>[];
  final swarm = <GameObject>[];
  late final GameObject speaker, swarmRoot;
  var visits = 1;
  var blueOnTop = false;
  var _said = 0;
  var _sinceSaid = 0.0;
  var _time = 0.0;
  var _ready = false;

  static const sayings = ['Hi!', 'Nice day for a walk.', 'Have you tried the café?', 'Sprites, all of us.'];

  @override
  void build() {
    add(shape('wall', Mesh.cube(), const Color(0xFF6A6F78), at: Vec3(0, 1.5, -2), size: Vec3(6, 3, 0.2)));
    speaker = add(shape('speaker', Mesh.capsule(), const Color(0xFF4CA6E0), at: Vec3(-2.2, 1, 0.5)));
  }

  @override
  Future<void> onLoad() async {
    super.onLoad();
    font = await world.assets.font('fonts/Roboto-Regular.ttf');
    bubble = await bubbleTexture();
    ring = round(64, (d) => [255, 255, 255, d > 0.75 && d < 0.95 ? 255 : 0]);
    dot = round(16, (d) => [255, 255, 255, (255 * (1 - d).clamp(0.0, 1.0)).round()]);
    card = round(32, (d) => [255, 255, 255, 255]);

    // The bubble and its words, over the speaker's head, facing the camera.
    words = TextRenderer('', font: font, size: 0.22, color: const Color(0xFF203040), facing: Facing.camera);
    bubbleSprite = SpriteRenderer(
      bubble,
      slice: const Insets.all(16),
      pixelsPerMetre: 160,
      pivot: Vec2(0.5, 0),
      facing: Facing.camera,
    );
    speaker.add(
      GameObject(
        name: 'bubble',
        transform: Transform(position: Vec3(0, 1.3, 0)),
        components: [bubbleSprite],
        children: [
          // A little in front of the bubble, so it's drawn after it.
          GameObject(
            name: 'words',
            transform: Transform(position: Vec3(0, 0.22, 0.02)),
            components: [words],
          ),
        ],
      ),
    );
    say(sayings.first);

    // Rings on the ground.
    for (var i = 0; i < 5; i++) {
      final sprite = SpriteRenderer(ring, size: Vec2(1, 1), color: const Color(0xFFFFD040), facing: Facing.up);
      rings.add(sprite);
      add(
        GameObject(
          name: 'ring $i',
          transform: Transform(position: Vec3(-2 + i * 1.2, 0.02, 2.5)),
          components: [sprite],
        ),
      );
    }

    // The poster on the wall, from a widget.
    posterSprite = SpriteRenderer(await poster(visits).toTexture(size: const flutter.Size(240, 150), pixelRatio: 2));
    posterSprite.size = Vec2(2.4, 1.5);
    add(
      GameObject(
        name: 'poster',
        transform: Transform(position: Vec3(1.2, 1.7, -1.88)),
        components: [posterSprite],
      ),
    );

    // Three cards, the red one nearest, yet green over it and blue under
    // both: drawn by order, not distance.
    for (final (i, color) in [const Color(0xFFE04040), const Color(0xFF40B060), const Color(0xFF4060E0)].indexed) {
      final sprite = SpriteRenderer(card, size: Vec2(0.8, 1.1), color: color, order: i < 2 ? i : -1);
      cards.add(sprite);
      add(
        GameObject(
          name: 'card $i',
          transform: Transform(position: Vec3(2.6 + i * 0.3, 0.8 - i * 0.15, 1.2 - i * 0.3)),
          components: [sprite],
        ),
      );
    }

    // A thousand dots of one texture, off until asked for.
    swarmRoot = add(GameObject(name: '1,000 sprites'))..active = false;
    for (var i = 0; i < 1000; i++) {
      swarm.add(
        swarmRoot.add(
          GameObject(
            components: [
              SpriteRenderer(
                dot,
                size: Vec2(0.12, 0.12),
                color: Color.rgb(0.5 + 0.5 * math.sin(i * 0.1), 0.6, 0.5 + 0.5 * math.cos(i * 0.07)),
                facing: Facing.camera,
              ),
            ],
          ),
        ),
      );
    }
    if (_startQuality.isNotEmpty) world.quality = Quality.levels.firstWhere((q) => q.name == _startQuality);
    if (printRates) swarmRoot.active = true;
    _ready = true;
  }

  /// The speaker says [text]: the bubble grows to fit it.
  void say(String text) {
    words.text = text;
    final measured = font.measure(text, size: 0.22);
    bubbleSprite.size = Vec2(measured.x + 0.4, measured.y + 0.3);
    log.add('says "$text"');
  }

  /// Draws the poster again, a visit more.
  Future<void> redrawPoster() async {
    visits++;
    final old = posterSprite.texture;
    posterSprite.texture = await poster(visits).toTexture(size: const flutter.Size(240, 150), pixelRatio: 2);
    old.dispose();
  }

  void setBlueOnTop(bool on) {
    blueOnTop = on;
    cards[2].order = on ? 3 : -1;
  }

  final _times = UpdateTimes('1,000 sprites');

  @override
  void onUpdate() {
    super.onUpdate();
    if (!_ready) return;
    _times.add(world);
    _time += time.delta;
    _sinceSaid += time.delta;
    if (_sinceSaid > 2.5) {
      _sinceSaid = 0;
      _said = (_said + 1) % sayings.length;
      say(sayings[_said]);
    }
    for (var i = 0; i < rings.length; i++) {
      final pulse = 0.5 + 0.5 * math.sin(_time * 3 - i * 0.8);
      rings[i].color = Color.rgb(1, 0.82, 0.25, 0.3 + 0.7 * pulse);
    }
    if (swarmRoot.active) {
      for (var i = 0; i < swarm.length; i++) {
        final a = _time * 0.5 + i * 0.37;
        final r = 1 + (i % 50) * 0.06;
        swarm[i].position = Vec3(r * math.cos(a), 2.6 + 0.8 * math.sin(a * 1.3 + i), r * math.sin(a) * 0.5);
      }
    }
    readout.value = 'the speaker says "${words.text}"${swarmRoot.active ? ', 1,000 sprites swirling' : ''}';
  }

  @override
  List<Control> get controls => [
    Button('redraw the poster', redrawPoster),
    Toggle('blue on top', () => blueOnTop, setBlueOnTop),
    Toggle('1,000 sprites', () => swarmRoot.active, (on) => swarmRoot.active = on),
  ];
}
