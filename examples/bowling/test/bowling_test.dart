// The score, as bowling counts it, and the game: pins that stand, a swipe
// that rolls, a pocket hit that strikes, a gutter ball, and a frame.

import 'dart:math' as math;

import 'package:bowling/bowling.dart';
import 'package:bowling/score.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';

void main() {
  group('the score', () {
    Score bowled(List<int> rolls) {
      final score = Score();
      rolls.forEach(score.add);
      return score;
    }

    test('a perfect game is 300, all nines 90, all gutters 0', () {
      expect(bowled(List.filled(12, 10)).total, 300);
      expect(bowled(List.filled(12, 10)).isOver, isTrue);
      expect(
        bowled([
          for (var i = 0; i < 10; i++) ...[9, 0],
        ]).total,
        90,
      );
      expect(bowled(List.filled(20, 0)).total, 0);
      expect(bowled(List.filled(20, 0)).isOver, isTrue);
    });

    test('a spare counts the next roll, a strike the next two', () {
      expect(bowled(List.filled(21, 5)).total, 150);
      expect(bowled([10, 3, 4]).totals, [17, 24]);
      expect(bowled([7, 3, 4, 2]).totals, [14, 20]);
      expect(bowled([10, 10]).totals, [null, null], reason: 'waiting for a roll');
      expect(bowled([10, 10, 10]).totals, [30, null, null]);
      expect(bowled([10, 10, 10]).total, 30);
    });

    test('the tenth frame gives the rolls a strike or spare needs', () {
      final open = bowled([...List.filled(18, 0), 3, 4]);
      expect(open.isOver, isTrue);
      final spare = bowled([...List.filled(18, 0), 7, 3]);
      expect(spare.isOver, isFalse);
      spare.add(10);
      expect((spare.isOver, spare.total), (true, 20));
      expect(() => spare.add(1), throwsStateError);
    });

    test('marks: X, /, - and the numbers', () {
      final score = bowled([10, 7, 3, 0, 9, ...List.filled(12, 0), 10, 7, 3]);
      expect(score.marks(0), ['X']);
      expect(score.marks(1), ['7', '/']);
      expect(score.marks(2), ['-', '9']);
      expect(score.marks(9), ['X', '7', '/']);
      expect(score.lastMark, '/');
      expect(bowled([...List.filled(18, 0), 10, 10, 10]).marks(9), ['X', 'X', 'X']);
      expect(bowled([...List.filled(18, 0), 3, 7, 10]).marks(9), ['3', '/', 'X']);
    });
  });

  group('the game', () {
    late World world;
    late BowlingGame game;
    void seconds(double s) {
      for (var i = 0; i < s * 60; i++) {
        world.update(1 / 60);
      }
    }

    setUp(() {
      world = World();
      addTearDown(world.dispose);
      game = world.add(BowlingGame());
      world.input.resize(400, 800);
      seconds(1);
    });

    /// Rolls from [x] to [aim] on the head pin's row, and waits until the
    /// pins are counted.
    void roll(double x, double aim, {double speed = 8}) {
      game.roll(x: x, speed: speed, angle: math.atan2(aim - x, headPin));
      expect(game.rolling, isTrue);
      var waited = 0.0;
      while (game.rolling) {
        seconds(0.5);
        waited += 0.5;
        expect(waited, lessThan(12), reason: 'the roll ends');
      }
    }

    test('the pins stand still on their own', () {
      final where = [for (final p in game.standing) p.position];
      seconds(5);
      expect(game.standing, hasLength(10));
      for (final (i, p) in game.standing.indexed) {
        expect((p.position - where[i]).length, lessThan(0.001));
      }
    });

    test('a swipe up the view rolls the ball from where it started, as fast as it went', () {
      world.input.fingerDown(1, 200, 700);
      world.update(1 / 60);
      for (var i = 1; i <= 6; i++) {
        world.input.fingerMove(1, 200, 700 - i * 40);
        world.update(1 / 60);
      }
      world.input.fingerUp(1, 200, 460);
      world.update(1 / 60);
      expect(game.rolling, isTrue);
      seconds(0.5);
      final ball = game.ball!;
      expect(ball.position.x.abs(), lessThan(0.05), reason: 'started in the middle, rolled straight');
      expect(ball.position.z, lessThan(-2), reason: 'down the lane');
    });

    test('a pocket hit strikes, and the pins are set up again', () {
      roll(0, 0.08);
      expect(game.score.frames.first, [10]);
      expect(game.message.value, 'Strike!');
      expect(game.standing, hasLength(10));
      expect(game.ball, isNull);
    });

    test('a gutter ball rolls past the pins to the pit, and knocks none down', () {
      var waited = 0.0;
      game.roll(x: 0.4, speed: 6, angle: math.atan2(0.4, headPin));
      while (game.rolling) {
        seconds(0.5);
        waited += 0.5;
      }
      expect(game.message.value, 'Gutter');
      expect(game.score.frames.first, [0]);
      expect(waited, lessThan(8), reason: 'the pit ended the roll, not the clock');
    });

    test('a frame is two rolls: what stands after the first stays for the second', () {
      roll(-0.4, -0.25);
      final first = game.score.frames.first.single;
      expect(first, inInclusiveRange(1, 9));
      expect(game.standing, hasLength(10 - first));
      expect(game.score.frame, 0);
      roll(0, 0);
      expect(game.score.frame, 1);
      expect(game.standing, hasLength(10));
    });
  });
}
