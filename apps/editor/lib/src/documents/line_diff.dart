// A line diff for the conflict dialog: what changed on disk since the tab
// last agreed with the file. Common prefix and suffix are stripped, the
// middle is an LCS table (capped — a document is a few thousand lines at
// most; beyond the cap the middle is shown as one replacement).
import 'dart:typed_data';

enum DiffKind { same, add, remove }

final class DiffLine {
  const DiffLine(this.kind, this.text);

  final DiffKind kind;
  final String text;

  String get prefix => switch (kind) {
    DiffKind.same => ' ',
    DiffKind.add => '+',
    DiffKind.remove => '-',
  };

  @override
  String toString() => '$prefix$text';
}

/// The line-by-line diff from [a] to [b].
List<DiffLine> lineDiff(String a, String b, {int maxTable = 4000}) {
  final la = a.split('\n');
  final lb = b.split('\n');
  var start = 0;
  while (start < la.length && start < lb.length && la[start] == lb[start]) {
    start++;
  }
  var endA = la.length, endB = lb.length;
  while (endA > start && endB > start && la[endA - 1] == lb[endB - 1]) {
    endA--;
    endB--;
  }
  final out = <DiffLine>[for (var i = 0; i < start; i++) DiffLine(DiffKind.same, la[i])];
  final midA = la.sublist(start, endA), midB = lb.sublist(start, endB);
  if (midA.length > maxTable || midB.length > maxTable) {
    out.addAll([for (final l in midA) DiffLine(DiffKind.remove, l), for (final l in midB) DiffLine(DiffKind.add, l)]);
  } else {
    out.addAll(_lcsDiff(midA, midB));
  }
  out.addAll([for (var i = endA; i < la.length; i++) DiffLine(DiffKind.same, la[i])]);
  return out;
}

List<DiffLine> _lcsDiff(List<String> a, List<String> b) {
  final n = a.length, m = b.length;
  if (n == 0) return [for (final l in b) DiffLine(DiffKind.add, l)];
  if (m == 0) return [for (final l in a) DiffLine(DiffKind.remove, l)];
  // table[i][j] = LCS length of a[i..] and b[j..].
  final w = m + 1;
  final table = Uint32List((n + 1) * w);
  for (var i = n - 1; i >= 0; i--) {
    for (var j = m - 1; j >= 0; j--) {
      table[i * w + j] = a[i] == b[j]
          ? table[(i + 1) * w + j + 1] + 1
          : (table[(i + 1) * w + j] > table[i * w + j + 1] ? table[(i + 1) * w + j] : table[i * w + j + 1]);
    }
  }
  final out = <DiffLine>[];
  var i = 0, j = 0;
  while (i < n && j < m) {
    if (a[i] == b[j]) {
      out.add(DiffLine(DiffKind.same, a[i]));
      i++;
      j++;
    } else if (table[(i + 1) * w + j] >= table[i * w + j + 1]) {
      out.add(DiffLine(DiffKind.remove, a[i]));
      i++;
    } else {
      out.add(DiffLine(DiffKind.add, b[j]));
      j++;
    }
  }
  while (i < n) {
    out.add(DiffLine(DiffKind.remove, a[i++]));
  }
  while (j < m) {
    out.add(DiffLine(DiffKind.add, b[j++]));
  }
  return out;
}

/// A unified-style text: changed lines with [context] unchanged lines around
/// them, hunks separated by `@@` lines with the line numbers.
String unifiedDiff(List<DiffLine> diff, {int context = 3}) {
  final keep = List<bool>.filled(diff.length, false);
  for (var i = 0; i < diff.length; i++) {
    if (diff[i].kind == DiffKind.same) continue;
    for (var k = i - context; k <= i + context; k++) {
      if (k >= 0 && k < diff.length) keep[k] = true;
    }
  }
  final out = StringBuffer();
  var lineA = 1, lineB = 1;
  var inHunk = false;
  for (var i = 0; i < diff.length; i++) {
    final d = diff[i];
    if (keep[i]) {
      if (!inHunk) {
        out.writeln('@@ -$lineA +$lineB @@');
        inHunk = true;
      }
      out.writeln('${d.prefix}${d.text}');
    } else {
      inHunk = false;
    }
    if (d.kind != DiffKind.add) lineA++;
    if (d.kind != DiffKind.remove) lineB++;
  }
  return out.toString();
}

/// `(+added, −removed)` line counts.
(int, int) diffCounts(List<DiffLine> diff) {
  var add = 0, remove = 0;
  for (final d in diff) {
    if (d.kind == DiffKind.add) add++;
    if (d.kind == DiffKind.remove) remove++;
  }
  return (add, remove);
}
