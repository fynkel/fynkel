// Makes the packages as they go to pub.dev, in fynkel-release-packages/ in
// the system's temporary folder (in a folder git ignores, pub sees none of
// their files), and asks pub what it thinks of each. Publishes nothing. After
// tool/prebuilt.dart, from the same commit:
//
//   cd packages/fynkel && dart run tool/release.dart
//
// The packages here depend on each other by path, and say publish_to: none,
// so none goes out by mistake; a released one depends on the others' same
// version, and has its library's prebuilt.json and the licences of the code
// in it. `--allow-dirty` as in tool/prebuilt.dart. docs/releasing.md has the
// whole release.

import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:fynkel/src/prebuilt.dart';

import 'notices.dart';
import 'prebuilt.dart' show libraryCrates, packageVersion, releasedPackages;

/// What a package's user has no use for.
final _leftOut = RegExp(r'^(test|benchmark|tool|build)/|^dart_test\.yaml$|^example/(?!example\.md$)');

void main(List<String> args) {
  final root = Directory.fromUri(Platform.script.resolve('../../../')).path.replaceFirst(RegExp(r'/$'), '');
  final version = packageVersion(root);
  final parts = version.split('.');
  final next = [...parts.take(2), int.parse(parts[2]) + 1].join('.');
  final built = Directory('$root/target/prebuilt/v$version');
  String git(List<String> args) => Process.runSync('git', args, workingDirectory: root).stdout as String;
  if (!built.existsSync()) _fail('run tool/prebuilt.dart first.');
  if (!args.contains('--allow-dirty')) {
    if (git(['status', '--porcelain']).trim().isNotEmpty) _fail('uncommitted changes.');
    final commit = File('${built.path}/COMMIT').readAsStringSync().trim();
    final head = git(['rev-parse', 'HEAD']).trim();
    if (commit != head) _fail('the libraries are of $commit, and this is $head: run tool/prebuilt.dart again.');
  }

  final out = Directory('${Directory.systemTemp.path}/fynkel-release-packages');
  if (out.existsSync()) out.deleteSync(recursive: true);
  for (final package in releasedPackages) {
    final from = '$root/packages/$package';
    final to = '${out.path}/$package';
    final files = git(['ls-files', '--cached', '--others', '--exclude-standard', '--', 'packages/$package']);
    for (final line in files.trim().split('\n')) {
      final path = line.substring('packages/$package/'.length);
      if (_leftOut.hasMatch(path)) continue;
      final file = File('$to/$path')..parent.createSync(recursive: true);
      File('$from/$path').copySync(file.path);
    }
    for (final needed in ['LICENSE', 'README.md', 'CHANGELOG.md', 'example/example.md']) {
      if (!File('$to/$needed').existsSync()) _fail('$package has no $needed.');
    }
    if (!File('$to/CHANGELOG.md').readAsStringSync().contains('## $version\n')) {
      _fail("$package's CHANGELOG.md says nothing of $version.");
    }

    final crate = libraryCrates[package];
    final pubspec = File('$to/pubspec.yaml');
    var text = pubspec.readAsStringSync();
    text = text.replaceFirst('publish_to: none\n', '');
    // No other: the packages share parts that aren't public, and a library
    // must be of the Dart that calls it.
    text = text.replaceAllMapped(
      RegExp(r'^  (fynkel\w*):\n    path: \S+$', multiLine: true),
      (m) => '  ${m[1]}: ">=$version <$next"',
    );
    if (text.contains('path: ../')) _fail("$package's pubspec still has a path.");
    if (crate != null) {
      text = '$text\n# For the app\'s licence page.\nflutter:\n  licenses:\n    - THIRD_PARTY_LICENSES\n';
    }
    pubspec.writeAsStringSync(text);

    if (crate != null) {
      final json = File('${built.path}/$package.prebuilt.json').readAsStringSync();
      for (final MapEntry(key: triple, value: want) in Prebuilt.parse(json, package).files.entries) {
        final library = File('${built.path}/${prebuiltName(crate, triple)}');
        if ('${sha256.convert(library.readAsBytesSync())}' != want.sha256) _fail('${library.path} has changed.');
      }
      File('$to/prebuilt.json').writeAsStringSync(json);
      File('$to/THIRD_PARTY_LICENSES').writeAsStringSync('${notices(root: root, crate: crate, package: package)}\n');
    }
  }

  // pub resolves what it checks, and the others aren't on pub.dev yet.
  var failed = false;
  for (final package in releasedPackages) {
    final dir = '${out.path}/$package';
    final overrides = File('$dir/pubspec_overrides.yaml');
    if (File('$dir/pubspec.yaml').readAsStringSync().contains('\n  fynkel: ')) {
      overrides.writeAsStringSync('dependency_overrides:\n  fynkel:\n    path: ../fynkel\n');
    }
    final result = Process.runSync('flutter', ['pub', 'publish', '--dry-run'], workingDirectory: dir);
    if (overrides.existsSync()) overrides.deleteSync();
    final said = '${result.stdout}${result.stderr}';
    stdout.writeln(
      '--- $package (exit ${result.exitCode})\n${said.substring(said.indexOf('Total compressed').clamp(0, said.length))}',
    );
    failed |= result.exitCode != 0;
    final tool = Directory('$dir/.dart_tool');
    if (tool.existsSync()) tool.deleteSync(recursive: true);
  }
  stdout.writeln('${out.path}: ${releasedPackages.join(', ')}');
  if (failed) exitCode = 1;
}

Never _fail(String why) {
  stderr.writeln('release: $why');
  exit(1);
}
