// The editor finds the newest `fynkel` CLI: one installed on PATH that is
// older than the checkout's build lacks what this editor asks of it (found
// by the user: New Behaviour failed with `unknown argument --project-only`).
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/env/tools.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';

void main() {
  test('the newest of PATH and the checkout build wins; an explicit choice always does', () async {
    final dir = Directory.systemTemp.createTempSync('fynkel-toolchain');
    addTearDown(() => dir.deleteSync(recursive: true));
    final root = '${dir.path}/checkout';
    File('$root/Cargo.toml')
      ..createSync(recursive: true)
      ..writeAsStringSync('[workspace]\nmembers = ["crates/fynkel_native"]\n');
    File exe(String path, DateTime at) => File(path)
      ..createSync(recursive: true)
      ..writeAsStringSync('#!/bin/sh\n')
      ..setLastModifiedSync(at);
    final installed = exe('${dir.path}/bin/fynkel', DateTime(2026, 9, 23));
    final debug = exe('$root/target/debug/fynkel', DateTime(2026, 9, 24));

    Future<String?> cli({Map<String, String> env = const {}}) async => (await Toolchain.detect(
      prefs: Preferences.inMemory(),
      environment: {'PATH': '${dir.path}/bin', ...env},
      run: (exe, args, {workingDirectory}) async => ProcessResult(0, 1, '', ''),
      workingDirectory: root,
      probeAbi: false,
    )).fynkelCli;

    expect(await cli(), debug.path);
    installed.setLastModifiedSync(DateTime(2026, 9, 25));
    expect(await cli(), installed.path);
    expect(await cli(env: {'FYNKEL_CLI': debug.path}), debug.path);
  });
}
