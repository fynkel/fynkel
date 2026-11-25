// A game made with the real Flutter, on this clone: it analyzes, and its own
// test passes.
@Timeout(Duration(minutes: 10))
library;

import 'dart:io';

import 'package:fynkel_cli/fynkel_cli.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

class _In extends Tools {
  _In(this.here);
  @override
  final Directory here;
}

void main() {
  test('fynkel create makes a game that analyzes and passes its test', () async {
    final dir = Directory(Directory.systemTemp.createTempSync('fynkel_real').resolveSymbolicLinksSync());
    addTearDown(() => dir.deleteSync(recursive: true));
    final out = StringBuffer();
    final args = ['create', 'my_game', '--physics', '--audio', '--path', p.join(Directory.current.path, '..', '..')];
    expect(
      await FynkelCli(tools: _In(dir), out: out, err: out).run(args),
      0,
      reason: '$out',
    );

    final game = Directory(p.join(dir.path, 'my_game'));
    for (final args in [
      ['analyze', '--fatal-infos'],
      ['test'],
    ]) {
      final result = Process.runSync('flutter', args, workingDirectory: game.path);
      expect(result.exitCode, 0, reason: 'flutter ${args.join(' ')}:\n${result.stdout}${result.stderr}');
    }
    out.clear();
    expect(
      await FynkelCli(tools: _In(game), out: out, err: out).run(['doctor']),
      0,
      reason: '$out',
    );
    expect(out.toString(), contains('[✓] Android: a FynkelActivity'));
  });
}
