// What the game shows over the view, in plain Flutter: the score sheet, a
// line under it, the finger's trail while it drags, and, until the first
// ball, how to bowl. None of it takes touches: they go to the game.

import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show Vec2;

import 'bowling.dart';

class Hud extends StatelessWidget {
  const Hud(this.game, {super.key});

  final BowlingGame game;

  static const _ink = Color(0xFFFFFFFF), _box = Color(0xCC1A1C24);

  @override
  Widget build(BuildContext context) => IgnorePointer(
    child: Stack(
      fit: StackFit.expand,
      children: [
        ValueListenableBuilder(
          valueListenable: game.trail,
          builder: (context, trail, _) => CustomPaint(painter: _Trail(trail)),
        ),
        SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Column(
              children: [
                ListenableBuilder(listenable: game.score, builder: (context, _) => _sheet()),
                const SizedBox(height: 8),
                ValueListenableBuilder(
                  valueListenable: game.message,
                  builder: (context, message, _) => Text(
                    message,
                    style: const TextStyle(fontSize: 22, fontWeight: FontWeight.bold, color: _ink),
                  ),
                ),
                // Up the lane, clear of the ball at the bottom.
                const Spacer(flex: 3),
                ValueListenableBuilder(
                  valueListenable: game.hint,
                  builder: (context, show, _) => show ? const _Hint() : const SizedBox.shrink(),
                ),
                const Spacer(flex: 2),
              ],
            ),
          ),
        ),
      ],
    ),
  );

  /// Ten boxes, each with its rolls' marks above and its total below.
  Widget _sheet() {
    final score = game.score;
    final totals = score.totals;
    // Smaller on a narrow screen, not cut off.
    return FittedBox(
      fit: BoxFit.scaleDown,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (var f = 0; f < 10; f++)
            Container(
              width: f == 9 ? 58 : 36,
              margin: const EdgeInsets.all(1),
              padding: const EdgeInsets.symmetric(vertical: 3),
              decoration: BoxDecoration(
                color: _box,
                border: Border.all(color: f == score.frame && !score.isOver ? const Color(0xFFFFC840) : _box),
              ),
              child: Column(
                children: [
                  Text(score.marks(f).join(' '), style: const TextStyle(fontSize: 12, color: _ink)),
                  Text(
                    f < totals.length && totals[f] != null ? '${totals[f]}' : '',
                    style: const TextStyle(fontSize: 15, fontWeight: FontWeight.bold, color: _ink),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

/// The finger's way, thin and faint at its tail, thick and bright at the
/// finger.
class _Trail extends CustomPainter {
  _Trail(this.points);

  final List<Vec2> points;

  @override
  void paint(Canvas canvas, Size size) {
    for (var i = 1; i < points.length; i++) {
      final t = i / (points.length - 1);
      final paint = Paint()
        ..color = Color.fromRGBO(255, 255, 255, 0.15 + 0.6 * t)
        ..strokeWidth = 3 + 9 * t
        ..strokeCap = StrokeCap.round;
      canvas.drawLine(Offset(points[i - 1].x, points[i - 1].y), Offset(points[i].x, points[i].y), paint);
    }
    if (points.isNotEmpty) {
      canvas.drawCircle(Offset(points.last.x, points.last.y), 14, Paint()..color = const Color(0x55FFFFFF));
    }
  }

  @override
  bool shouldRepaint(_Trail old) => old.points != points;
}

/// How to bowl: a finger that slides sideways, then flicks up, over and
/// over, and the words.
class _Hint extends StatefulWidget {
  const _Hint();

  @override
  State<_Hint> createState() => _HintState();
}

class _HintState extends State<_Hint> with SingleTickerProviderStateMixin {
  late final _clock = AnimationController(vsync: this, duration: const Duration(milliseconds: 2400))..repeat();

  @override
  void dispose() {
    _clock.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    children: [
      SizedBox(
        width: 160,
        height: 150,
        child: AnimatedBuilder(
          animation: _clock,
          builder: (context, _) => CustomPaint(painter: _Finger(_clock.value)),
        ),
      ),
      const Text(
        'Drag sideways to place the ball, flick up to bowl',
        textAlign: TextAlign.center,
        style: TextStyle(fontSize: 17, color: Hud._ink, shadows: [Shadow(blurRadius: 4)]),
      ),
      const SizedBox(height: 24),
    ],
  );
}

/// The hint's finger at time [t] of its loop, 0 to 1: it lands, slides
/// sideways and back (placing), then flicks up and fades (bowling).
class _Finger extends CustomPainter {
  _Finger(this.t);

  final double t;

  @override
  void paint(Canvas canvas, Size size) {
    final bottom = size.height - 18, middle = size.width / 2;
    late Offset at;
    var fade = 1.0;
    if (t < 0.45) {
      // Placing: right and back to the middle.
      final s = Curves.easeInOut.transform(t / 0.45);
      at = Offset(middle + 40 * (s < 0.5 ? s * 2 : 2 - s * 2), bottom);
    } else if (t < 0.55) {
      at = Offset(middle, bottom);
    } else {
      // Bowling: a quick flick up, fading out.
      final s = Curves.easeIn.transform(((t - 0.55) / 0.3).clamp(0, 1));
      at = Offset(middle, bottom - s * (size.height - 36));
      fade = 1 - ((t - 0.55) / 0.45);
      if (s > 0) {
        canvas.drawLine(
          Offset(middle, bottom),
          at,
          Paint()
            ..color = Color.fromRGBO(255, 255, 255, 0.35 * fade)
            ..strokeWidth = 8
            ..strokeCap = StrokeCap.round,
        );
      }
    }
    canvas.drawCircle(at, 16, Paint()..color = Color.fromRGBO(255, 255, 255, 0.85 * fade));
    canvas.drawCircle(
      at,
      16,
      Paint()
        ..color = Color.fromRGBO(0, 0, 0, 0.3 * fade)
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2,
    );
  }

  @override
  bool shouldRepaint(_Finger old) => old.t != t;
}
