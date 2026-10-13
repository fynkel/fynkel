import 'dart:ui' as ui;

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' as fynkel;
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  test('a colour goes from Flutter to Fynkel and back as it was', () {
    const flutter = ui.Color(0x804CA6E0);
    expect(flutter.toFynkel(), const fynkel.Color(0x804CA6E0));
    expect(flutter.toFynkel().toFlutter(), flutter);
    expect(fynkel.Color.white.toFlutter(), const ui.Color(0xFFFFFFFF));
  });

  test('a Display P3 colour comes to Fynkel in sRGB', () {
    const p3 = ui.Color.from(alpha: 1, red: 0.8, green: 0.3, blue: 0.3, colorSpace: ui.ColorSpace.displayP3);
    final c = p3.toFynkel();
    expect(c.r, greaterThan(0.8), reason: 'the same red is redder in sRGB numbers');
    expect(c.g, lessThan(0.3));
    expect(
      const ui.Color.from(alpha: 1, red: 1, green: 0, blue: 0, colorSpace: ui.ColorSpace.displayP3).toFynkel().r,
      1,
      reason: 'what sRGB can\'t show is cut to it',
    );
  });
}
