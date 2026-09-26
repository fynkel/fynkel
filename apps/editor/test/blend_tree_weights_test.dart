// A blend tree's weights in the editor (ADR-0033 § 1) against the same
// hand-worked values the runtime's tests hold
// (`crates/fynkel_animation/src/blend_tree.rs`): the map shows what the
// game plays.
import 'dart:math' as math;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/animator/blend_tree_inspector.dart';
import 'package:fynkel_editor/src/animator/blend_tree_weights.dart';

void main() {
  void close(List<double> a, List<double> b) {
    expect(a.length, b.length, reason: '$a vs $b');
    for (var i = 0; i < a.length; i++) {
      expect((a[i] - b[i]).abs() < 1e-4, isTrue, reason: '$a vs $b');
    }
  }

  // Idle at the centre, a walk at 2 m/s in each of the four directions.
  const four = [(0.0, 0.0), (0.0, 2.0), (0.0, -2.0), (-2.0, 0.0), (2.0, 0.0)];
  const twoD = ['simpleDirectional', 'freeformDirectional', 'freeformCartesian'];

  test('1D is linear between neighbours and clamped, in any order', () {
    const t = [(4.0, 0.0), (0.0, 0.0), (7.0, 0.0)];
    close(treeWeights('1d', t, (2, 0)), [0.5, 0.5, 0]);
    close(treeWeights('1d', t, (5.5, 0)), [0.5, 0, 0.5]);
    close(treeWeights('1d', t, (4, 0)), [1, 0, 0]);
    close(treeWeights('1d', t, (-3, 0)), [0, 1, 0]);
    close(treeWeights('1d', t, (9, 0)), [0, 0, 1]);
    close(treeWeights('1d', t, (4.0015, 0)), [1, 0, 0]);
  });

  test('every 2D type gives a child all the weight at its place, and sums to 1 everywhere', () {
    for (final type in twoD) {
      for (var i = 0; i < four.length; i++) {
        close(treeWeights(type, four, four[i]), [for (var j = 0; j < 5; j++) j == i ? 1 : 0]);
      }
      for (var x = -6; x <= 6; x++) {
        for (var y = -6; y <= 6; y++) {
          final ws = treeWeights(type, four, (x * 0.5, y * 0.5));
          expect((ws.fold(0.0, (a, b) => a + b) - 1).abs() < 1e-5, isTrue, reason: '$type $ws');
          expect(ws.every((w) => w >= 0), isTrue);
        }
      }
    }
  });

  test('simple directional is barycentric in the wedge', () {
    close(treeWeights('simpleDirectional', four, (0, 1)), [0.5, 0.5, 0, 0, 0]);
    close(treeWeights('simpleDirectional', four, (0.5, 0.5)), [0.5, 0.25, 0, 0, 0.25]);
    close(treeWeights('simpleDirectional', four, (1, 1)), [0, 0.5, 0, 0, 0.5]);
    close(treeWeights('simpleDirectional', four, (2, 2)), [0, 0.5, 0, 0, 0.5]);
    close(treeWeights('simpleDirectional', four, (0, 9)), [0, 1, 0, 0, 0]);
  });

  test('the cartesian gradient band', () {
    const two = [(0.0, 0.0), (2.0, 0.0)];
    close(treeWeights('freeformCartesian', two, (0.5, 0)), [0.75, 0.25]);
    close(treeWeights('freeformCartesian', two, (0.5, 3)), [0.75, 0.25]);
    close(treeWeights('freeformCartesian', four, (1, 0)), [0.5, 0, 0, 0, 0.5]);
  });

  test('the polar gradient band blends speed along a direction and angle between', () {
    const t = [(0.0, 0.0), (0.0, 2.0), (0.0, 4.0), (2.0, 0.0), (4.0, 0.0)];
    close(treeWeights('freeformDirectional', t, (0, 3)), [0, 0.5, 0.5, 0, 0]);
    final d = 2 / math.sqrt2;
    final sum = 1 + (1 - 4 / (4 + math.pi * math.pi / 4));
    close(treeWeights('freeformDirectional', t, (d, d)), [(sum - 1) / sum, 0.5 / sum, 0, 0.5 / sum, 0]);
    close(treeWeights('freeformDirectional', t, (0, 1)), [0.5, 0.5, 0, 0, 0]);
  });

  test('direct weights are the parameters normalized', () {
    close(directWeights([1, 3, -2]), [0.25, 0.75, 0]);
    close(directWeights([0, 0]), [1, 0]);
  });

  test('Compute Positions reads the travel as the map does: forward is up', () {
    // A clip walking forward (-Z) at 2 m/s turning 90°/s.
    const v = [0.0, 0.0, -2.0];
    expect(TravelMeasure.velocityXZ.place(v, 0), (0.0, 2.0));
    expect(TravelMeasure.speed.place(v, 0), (2.0, 0.0));
    expect(TravelMeasure.velocityZ.place(v, 0), (-2.0, 0.0));
    final (s, deg) = TravelMeasure.speedAndAngular.place(v, math.pi / 2);
    expect((s, deg.roundToDouble()), (2.0, 90.0));
  });

  test('the first clip a motion plays, depth first', () {
    const clip = {'asset': 'm', 'sub': 'anim/Walk'};
    expect(firstClip(clip), clip);
    expect(firstClip({'asset': 'space'}), isNull, reason: 'a blend space is not a clip');
    expect(
      firstClip({
        'blendTree': {
          'type': '1d',
          'children': [
            {'threshold': 0},
            {
              'motion': {
                'blendTree': {
                  'type': 'direct',
                  'children': [
                    {'motion': clip},
                  ],
                },
              },
            },
          ],
        },
      }),
      clip,
    );
  });
}
