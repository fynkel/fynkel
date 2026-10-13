// FrameList (extend.dart): a list emptied every frame that keeps its
// storage, so filling it again allocates nothing.

import 'package:fynkel/extend.dart';
import 'package:test/test.dart';

void main() {
  test('adds, reads, takes away and clears, in order', () {
    final list = FrameList<String>();
    expect((list.length, list.isEmpty, list.isNotEmpty), (0, true, false));
    for (var i = 0; i < 20; i++) {
      list.add('$i');
    }
    expect((list.length, list[0], list[19]), (20, '0', '19'));
    expect(() => list[20], throwsRangeError);
    expect(() => list[-1], throwsRangeError);
    list.removeWhere((s) => int.parse(s).isOdd);
    expect([for (var i = 0; i < list.length; i++) list[i]], ['0', '2', '4', '6', '8', '10', '12', '14', '16', '18']);
    expect(() => list[10], throwsRangeError);
    list.clear();
    expect((list.length, list.isEmpty), (0, true));
    expect(() => list[0], throwsRangeError);
    list.add('again');
    expect((list.length, list[0]), (1, 'again'));
  });
}
