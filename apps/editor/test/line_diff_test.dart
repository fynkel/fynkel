import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/documents/line_diff.dart';

void main() {
  test('a line diff keeps the common prefix and suffix and marks the middle', () {
    final d = lineDiff('a\nb\nc\nd\ne', 'a\nB\nc\nd2\ne');
    expect(d.map((l) => l.toString()).toList(), [' a', '-b', '+B', ' c', '-d', '+d2', ' e']);
    expect(diffCounts(d), (2, 2));
  });

  test('identical texts diff to nothing; a pure insertion is all adds', () {
    expect(diffCounts(lineDiff('x\ny', 'x\ny')), (0, 0));
    final ins = lineDiff('x\ny', 'x\nnew\ny');
    expect(ins.where((l) => l.kind == DiffKind.add).map((l) => l.text), ['new']);
    expect(diffCounts(ins), (1, 0));
  });

  test('a unified view shows hunks with context and line numbers', () {
    final a = List.generate(30, (i) => 'line $i').join('\n');
    final b = a.replaceFirst('line 10', 'line ten').replaceFirst('line 25', 'line twenty-five');
    final u = unifiedDiff(lineDiff(a, b), context: 1);
    expect(u, contains('@@ -10 +10 @@'));
    expect(u, contains('-line 10\n+line ten\n'));
    expect(u, contains('@@ -25 +25 @@'));
    expect(u, isNot(contains('line 17'))); // outside every hunk
  });

  test('a huge middle falls back to one replacement instead of the table', () {
    final a = List.generate(5000, (i) => 'a$i').join('\n');
    final b = List.generate(5000, (i) => 'b$i').join('\n');
    final d = lineDiff(a, b, maxTable: 100);
    expect(diffCounts(d), (5000, 5000));
  });
}
