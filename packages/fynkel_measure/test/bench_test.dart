import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_measure/fynkel_measure.dart';

void main() {
  // `measure bench` runs this on the phones; a scene built wrong (its rays
  // missing) throws, and the tool waits for a 'done' that never comes.
  test('the bench runs through, every line timed', () {
    final lines = bench();
    expect(lines.where((l) => l.startsWith('raycast')), hasLength(2));
  }, timeout: const Timeout(Duration(minutes: 3)));
}
