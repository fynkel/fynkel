// The eases (docs/stage-4.md §3.7, step 6): each one's ends, a few
// hand-worked values, and its shape.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

const eases = <String, EaseFunction>{
  'linear': Ease.linear,
  'inSine': Ease.inSine,
  'outSine': Ease.outSine,
  'inOutSine': Ease.inOutSine,
  'inQuad': Ease.inQuad,
  'outQuad': Ease.outQuad,
  'inOutQuad': Ease.inOutQuad,
  'inCubic': Ease.inCubic,
  'outCubic': Ease.outCubic,
  'inOutCubic': Ease.inOutCubic,
  'inQuart': Ease.inQuart,
  'outQuart': Ease.outQuart,
  'inOutQuart': Ease.inOutQuart,
  'inQuint': Ease.inQuint,
  'outQuint': Ease.outQuint,
  'inOutQuint': Ease.inOutQuint,
  'inExpo': Ease.inExpo,
  'outExpo': Ease.outExpo,
  'inOutExpo': Ease.inOutExpo,
  'inCirc': Ease.inCirc,
  'outCirc': Ease.outCirc,
  'inOutCirc': Ease.inOutCirc,
  'inBack': Ease.inBack,
  'outBack': Ease.outBack,
  'inOutBack': Ease.inOutBack,
  'inElastic': Ease.inElastic,
  'outElastic': Ease.outElastic,
  'inOutElastic': Ease.inOutElastic,
  'inBounce': Ease.inBounce,
  'outBounce': Ease.outBounce,
  'inOutBounce': Ease.inOutBounce,
};

Iterable<double> get inside => [for (var i = 1; i < 100; i++) i / 100];

void main() {
  test('every ease starts at 0 and ends at 1', () {
    for (final MapEntry(key: name, value: ease) in eases.entries) {
      expect(ease(0), closeTo(0, 1e-3), reason: name);
      expect(ease(1), closeTo(1, 1e-12), reason: name);
    }
  });

  test('hand-worked values', () {
    expect(Ease.linear(0.3), 0.3);
    expect(Ease.inQuad(0.5), 0.25);
    expect(Ease.outQuad(0.5), 0.75);
    expect(Ease.inOutQuad(0.25), 0.125);
    expect(Ease.inOutQuad(0.75), 0.875);
    expect(Ease.inCubic(0.5), 0.125);
    expect(Ease.outCubic(0.5), 0.875);
    expect(Ease.inQuart(0.5), 0.0625);
    expect(Ease.inQuint(0.5), 0.03125);
    expect(Ease.inSine(0.5), closeTo(1 - math.cos(math.pi / 4), 1e-12));
    expect(Ease.outSine(0.5), closeTo(math.sin(math.pi / 4), 1e-12));
    expect(Ease.inExpo(0.5), closeTo(1 / 32, 1e-12));
    expect(Ease.inCirc(0.6), closeTo(0.2, 1e-12));
    expect(Ease.outBounce(1 / 2.75), closeTo(1, 1e-12), reason: 'the first bounce lands');
  });

  test("an in ease starts slow, an out one ends slow, and an in-out one is both, mirrored", () {
    for (final kind in ['Sine', 'Quad', 'Cubic', 'Quart', 'Quint', 'Expo', 'Circ']) {
      for (final t in inside) {
        expect(eases['in$kind']!(t), lessThan(t), reason: 'in$kind($t)');
        expect(eases['out$kind']!(t), greaterThan(t), reason: 'out$kind($t)');
      }
    }
    for (final MapEntry(key: name, value: ease) in eases.entries.where((e) => e.key.startsWith('inOut'))) {
      expect(ease(0.5), closeTo(0.5, 1e-12), reason: name);
      for (final t in inside) {
        expect(ease(1 - t), closeTo(1 - ease(t), 1e-9), reason: '$name($t)');
      }
    }
  });

  test('each in ease is its out ease backwards', () {
    for (final kind in ['Sine', 'Quad', 'Cubic', 'Quart', 'Quint', 'Expo', 'Circ', 'Back', 'Elastic', 'Bounce']) {
      for (final t in inside) {
        expect(eases['in$kind']!(t), closeTo(1 - eases['out$kind']!(1 - t), 1e-9), reason: '$kind($t)');
      }
    }
  });

  test('back pulls back first and overshoots after; elastic springs past; bounce never leaves 0–1', () {
    expect(inside.map(Ease.inBack).reduce(math.min), lessThan(-0.09));
    expect(inside.map(Ease.outBack).reduce(math.max), greaterThan(1.09));
    expect(inside.map(Ease.outElastic).reduce(math.max), greaterThan(1.3));
    expect(inside.map(Ease.inElastic).reduce(math.min), lessThan(-0.3));
    for (final ease in [Ease.inBounce, Ease.outBounce, Ease.inOutBounce]) {
      for (final t in inside) {
        expect(ease(t), inInclusiveRange(0, 1));
      }
    }
  });

  test('the steady eases only ever go forwards', () {
    for (final MapEntry(key: name, value: ease) in eases.entries) {
      if (name.contains('Back') || name.contains('Elastic') || name.contains('Bounce')) continue;
      var last = ease(0);
      for (final t in [...inside, 1.0]) {
        expect(ease(t), greaterThanOrEqualTo(last), reason: '$name($t)');
        last = ease(t);
      }
    }
  });
}
