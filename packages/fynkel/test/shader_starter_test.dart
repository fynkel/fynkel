import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

import '../bin/shader.dart' as starter;

void main() {
  test('every starter `dart run fynkel:shader` writes compiles, as the kind it says', () {
    final dir = Directory.systemTemp.createTempSync('fynkel_starter');
    addTearDown(() => dir.deleteSync(recursive: true));
    final kinds = {
      'plain': <String>[],
      'flat': ['--unlit'],
      'sway': ['--moves'],
      'bands': ['--light'],
      'both': ['--moves', '--light'],
      'calm': ['--unlit', '--moves'],
      'vignette': ['--effect'],
    };
    for (final MapEntry(key: name, value: flags) in kinds.entries) {
      starter.main([name, ...flags, '--dir', dir.path]);
      final shader = Shader(File('${dir.path}/$name.wgsl').readAsStringSync(), name: '$name.wgsl');
      addTearDown(shader.dispose);
      expect(shader.isEffect, flags.contains('--effect'), reason: name);
      expect(shader.isLit, !flags.contains('--unlit') && !flags.contains('--effect'), reason: name);
      expect(shader.movesVertices, flags.contains('--moves'), reason: name);
      expect(shader.hasOwnLighting, flags.contains('--light'), reason: name);
      expect(shader.values, contains(flags.contains('--effect') ? 'strength' : 'tint'), reason: name);
    }
    expect(exitCode, 0);

    // It never writes over a file, and says why it refuses.
    starter.main(['plain', '--dir', dir.path]);
    expect(exitCode, 64);
    exitCode = 0;
    starter.main(['fx', '--effect', '--light', '--dir', dir.path]);
    expect(exitCode, 64);
    expect(File('${dir.path}/fx.wgsl').existsSync(), isFalse);
    exitCode = 0;
  });
}
