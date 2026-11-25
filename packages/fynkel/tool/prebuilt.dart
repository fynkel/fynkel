// Builds the libraries a release ships, so games need no Rust, and writes
// each package's prebuilt.json. Run on a Mac, from a clean checkout of what
// is to be released:
//
//   cd packages/fynkel && dart run tool/prebuilt.dart
//
// The libraries, with their SHA256SUMS, are left in target/prebuilt/v<version>/
// for the GitHub release v<version>; this uploads nothing. `--allow-dirty`
// takes uncommitted changes, to try it out.

import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:crypto/crypto.dart';
import 'package:fynkel/src/build_plan.dart';
import 'package:fynkel/src/prebuilt.dart';

/// Each package that ships a library, and its crate.
const _crates = {'fynkel': 'fynkel_ffi', 'fynkel_physics': 'fynkel_physics_ffi', 'fynkel_audio': 'fynkel_audio_ffi'};

// The oldest systems package:fynkel_flutter runs on (its Package.swift and
// build.gradle.kts); a library built for them runs on newer ones.
const _macOS = 12;
const _iOS = 15;
const _androidApi = 30;

void main(List<String> args) async {
  final root = Directory.fromUri(Platform.script.resolve('../../../')).path.replaceFirst(RegExp(r'/$'), '');
  final home = Platform.environment['HOME']!;

  final version = _version(root);
  if (!args.contains('--allow-dirty')) {
    final status = Process.runSync('git', ['status', '--porcelain'], workingDirectory: root).stdout as String;
    if (status.trim().isNotEmpty) _fail('uncommitted changes:\n$status');
  }

  final target = '$root/target/prebuilt-build';
  final out = Directory('$root/target/prebuilt/v$version');
  if (out.existsSync()) out.deleteSync(recursive: true);
  out.createSync(recursive: true);

  // Paths on this machine, which rustc writes into a library for its panic
  // messages, say whose it is: they are left out. The last that matches wins.
  final flags = [
    '--remap-path-prefix=$home=/home',
    '--remap-path-prefix=$home/.cargo/registry/src=/cargo',
    '--remap-path-prefix=$root=/fynkel',
  ];

  final sums = StringBuffer();
  for (final MapEntry(key: package, value: crate) in _crates.entries) {
    final files = <String, ({String sha256, int bytes})>{};
    for (final triple in prebuiltTriples) {
      final os = triple.endsWith('-darwin')
          ? OS.macOS
          : triple.endsWith('-android')
          ? OS.android
          : OS.iOS;
      stdout.writeln('$crate for $triple');
      final result = await runStreaming(
        'cargo',
        cargoArgs(crate, triple),
        workingDirectory: root,
        environment: {
          'CARGO_TARGET_DIR': target,
          'CARGO_ENCODED_RUSTFLAGS': [
            ...flags,
            // A library for Apple names itself, by where it was built unless
            // told otherwise. Flutter gives it a longer name in an app: room
            // is left for it.
            if (os != OS.android) ...[
              '-Clink-arg=-Wl,-install_name,@rpath/${os.dylibFileName(crate)}',
              '-Clink-arg=-Wl,-headerpad_max_install_names',
            ],
          ].join('\x1f'),
          ...deploymentTarget(os, os == OS.macOS ? _macOS : _iOS),
          if (os == OS.android)
            'CARGO_TARGET_${triple.toUpperCase().replaceAll('-', '_')}_LINKER': androidLinker(
              triple: triple,
              api: _androidApi,
              compiler: null,
              env: Platform.environment,
            ),
        },
        onOutput: (_) {},
      );
      if (result.exitCode != 0) _fail(result.stderr);
      final name = prebuiltName(crate, triple);
      final file = File('${out.path}/$name');
      await copyBuilt(File('$target/$triple/release/${os.dylibFileName(crate)}'), file.path);
      _check(file, os, triple, home);
      final bytes = file.readAsBytesSync();
      final hash = '${sha256.convert(bytes)}';
      files[triple] = (sha256: hash, bytes: bytes.length);
      sums.writeln('$hash  $name');
    }
    File('$root/packages/$package/prebuilt.json').writeAsStringSync(
      Prebuilt(
        crate: crate,
        url: 'https://github.com/fynkel/fynkel/releases/download/v$version',
        files: files,
      ).toJson(),
    );
  }
  File('${out.path}/SHA256SUMS').writeAsStringSync('$sums');
  stdout.writeln('${out.path}: ${_crates.length * prebuiltTriples.length} libraries');
}

/// The version all the packages must share.
String _version(String root) {
  final versions = {
    for (final package in [..._crates.keys, 'fynkel_flutter'])
      package: RegExp(
        r'^version: (\S+)$',
        multiLine: true,
      ).firstMatch(File('$root/packages/$package/pubspec.yaml').readAsStringSync())?.group(1),
  };
  if (versions.values.toSet().length != 1 || versions['fynkel'] == null) _fail('the versions differ: $versions');
  return versions['fynkel']!;
}

/// That [file] holds nothing of this machine's, and runs where it should.
void _check(File file, OS os, String triple, String home) {
  // As Latin-1, every byte is a character: the search is of the bytes.
  if (String.fromCharCodes(file.readAsBytesSync()).contains(home)) _fail('${file.path} holds the path $home.');
  if (os == OS.android) {
    final readelf = androidLinker(
      triple: triple,
      api: _androidApi,
      compiler: null,
      env: Platform.environment,
    ).replaceFirst(RegExp(r'[^/]+$'), 'llvm-readelf');
    final headers = Process.runSync(readelf, ['-lW', file.path]).stdout as String;
    final loads = headers.split('\n').where((line) => line.trim().startsWith('LOAD')).toList();
    // Google Play asks for 16 KB pages.
    if (loads.isEmpty || !loads.every((line) => line.trim().endsWith('0x4000'))) {
      _fail('${file.path} is not laid out for 16 KB pages:\n$headers');
    }
  } else {
    final info = Process.runSync('vtool', ['-show-build', file.path]).stdout as String;
    final want = os == OS.macOS ? _macOS : _iOS;
    if (!RegExp('minos $want\\.0\\b').hasMatch(info)) _fail('${file.path} is not built for ${os.name} $want:\n$info');
  }
}

Never _fail(String why) {
  stderr.writeln('prebuilt: $why');
  exit(1);
}
