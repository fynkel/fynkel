// Hands a Rust library to the Dart and Flutter tools as a code asset, for a
// build hook's target. package:fynkel's hook hands over the engine with it,
// and package:fynkel_physics's the physics.
//
// A package in the Fynkel repository (packages/<name>, the Cargo workspace
// at its root) builds the library, always in release: a debug build is many
// times slower. Cargo is incremental, so an unchanged crate costs a second
// or two. A released package, from pub.dev, downloads the library already
// built (lib/src/prebuilt.dart), and needs no Rust.

import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:hooks/hooks.dart';

import 'src/build_plan.dart';
import 'src/prebuilt.dart';

export 'src/build_plan.dart' show HookException;

/// Builds [crate] (a cdylib in the workspace's crates/) for the target in
/// [input], or downloads it built, and adds it to [output] as the code asset
/// named [asset] of the package whose hook this is.
///
/// A game behind a wall can take the downloads from a mirror of its own, in
/// its pubspec.yaml (each is still checked against the package's hash):
///
/// ```yaml
/// hooks:
///   user_defines:
///     fynkel:
///       mirror: https://example.com/fynkel/
/// ```
Future<void> buildRustCrate(
  BuildInput input,
  BuildOutputBuilder output, {
  required String crate,
  required String asset,
}) async {
  if (!input.config.buildCodeAssets) return;
  final code = input.config.code;
  final os = code.targetOS;
  final arch = code.targetArchitecture;
  final triple = _rustTriple(os, arch, iosSdk: os == OS.iOS ? code.iOS.targetSdk : null);
  if (triple == null) {
    // `flutter build apk` builds 32-bit Android too (armeabi-v7a, x86):
    // leave the library out there, so the build goes on. Fynkel runs on
    // 64-bit devices only, and on a 32-bit one the game stops at once
    // with an error that says so (package:fynkel's lib/src/native.dart).
    if (os == OS.android && (arch == Architecture.arm || arch == Architecture.ia32)) return;
    throw HookException('Fynkel does not build for ${os.name}/${arch.name}.');
  }

  final workspace = input.packageRoot.resolve('../../');
  if (!File.fromUri(workspace.resolve('crates/$crate/Cargo.toml')).existsSync()) {
    return _usePrebuilt(input, output, crate: crate, asset: asset, triple: triple);
  }
  final cargo = _findCargo();
  if (cargo == null) {
    throw HookException('`cargo` was not found on PATH or in ~/.cargo/bin. Install Rust: https://rustup.rs');
  }

  final args = cargoArgs(crate, triple);
  stderr.writeln('fynkel: cargo ${args.join(' ')} (the first build takes a few minutes)');
  final result = await runStreaming(
    cargo,
    args,
    workingDirectory: workspace.toFilePath(),
    environment: {
      'PATH': '${Platform.environment['PATH'] ?? ''}:${File(cargo).parent.path}',
      // Where the library is copied from, below; a CARGO_TARGET_DIR of the
      // user's (which the hook runner doesn't pass on now) would move it.
      'CARGO_TARGET_DIR': workspace.resolve('target').toFilePath(),
      ...deploymentTarget(os, _appleVersion(code)),
      if (os == OS.android)
        'CARGO_TARGET_${triple.toUpperCase().replaceAll('-', '_')}_LINKER': androidLinker(
          triple: triple,
          api: code.android.targetNdkApi,
          compiler: code.cCompiler?.compiler,
          env: Platform.environment,
        ),
    },
    // Flutter shows a hook's output with -v.
    onOutput: stderr.write,
  );
  if (result.exitCode != 0) {
    // A missing standard library for the target is the usual first-time
    // failure (Flutter builds every architecture it ships).
    final hint = result.stderr.contains('E0463')
        ? '\nInstall the target: rustup target add $triple'
        : result.stderr.contains('--locked')
        ? '\nCargo.lock is out of date with a Cargo.toml: run `cargo update --workspace` in the repository.'
        : '';
    throw HookException('cargo ${args.join(' ')} failed (exit ${result.exitCode}):\n${result.stderr}$hint');
  }

  final fileName = os.dylibFileName(crate);
  final built = workspace.resolve('target/$triple/release/$fileName');
  final dest = input.outputDirectory.resolve(fileName);
  await copyBuilt(File.fromUri(built), dest.toFilePath());

  output.assets.code.add(
    CodeAsset(package: input.packageName, name: asset, linkMode: DynamicLoadingBundled(), file: dest),
  );

  // Run again when the workspace's source changes; cargo decides what to
  // rebuild.
  output.dependencies.add(workspace.resolve('Cargo.toml'));
  output.dependencies.add(workspace.resolve('Cargo.lock'));
  // third_party/ too: the patched Rapier the physics is built with.
  for (final folder in ['crates/', 'third_party/']) {
    for (final entity in Directory.fromUri(workspace.resolve(folder)).listSync(recursive: true)) {
      final path = entity.path;
      if (entity is File && (path.endsWith('.rs') || path.endsWith('.wgsl') || path.endsWith('Cargo.toml'))) {
        output.dependencies.add(entity.uri);
      }
    }
  }
}

/// Adds the library the package was released with.
Future<void> _usePrebuilt(
  BuildInput input,
  BuildOutputBuilder output, {
  required String crate,
  required String asset,
  required String triple,
}) async {
  final manifest = File.fromUri(input.packageRoot.resolve('prebuilt.json'));
  if (!manifest.existsSync()) {
    throw HookException(
      '${input.packageRoot.toFilePath()} has neither the Fynkel repository around it, to build $crate from, nor a '
      'prebuilt.json, which a release on pub.dev has. Depend on the pub.dev release, or on a whole clone of the '
      'repository.',
    );
  }
  final mirror = input.userDefines['mirror'];
  if (mirror is! String?) throw HookException("the pubspec's hooks: user_defines: mirror must be a URL.");
  final file = await Prebuilt.parse(
    manifest.readAsStringSync(),
    manifest.path,
  ).fetch(triple, cache: Directory.fromUri(input.outputDirectoryShared), mirror: mirror, onOutput: stderr.write);
  final dest = input.outputDirectory.resolve(input.config.code.targetOS.dylibFileName(crate));
  await file.copy(dest.toFilePath());
  output.assets.code.add(
    CodeAsset(package: input.packageName, name: asset, linkMode: DynamicLoadingBundled(), file: dest),
  );
  output.dependencies.add(manifest.uri);
}

/// The Rust target triple, or null for a target Fynkel doesn't build.
String? _rustTriple(OS os, Architecture arch, {IOSSdk? iosSdk}) {
  final a = switch (arch) {
    Architecture.arm64 => 'aarch64',
    Architecture.x64 => 'x86_64',
    _ => null,
  };
  if (a == null) return null;
  return switch (os) {
    OS.macOS => '$a-apple-darwin',
    OS.iOS when iosSdk == IOSSdk.iPhoneSimulator => a == 'aarch64' ? 'aarch64-apple-ios-sim' : 'x86_64-apple-ios',
    OS.iOS => a == 'aarch64' ? 'aarch64-apple-ios' : null,
    OS.android => '$a-linux-android',
    _ => null,
  };
}

/// The macOS or iOS deployment target Flutter gives the hook, if any.
int? _appleVersion(CodeConfig code) {
  try {
    return switch (code.targetOS) {
      OS.macOS => code.macOS.deploymentVersion.major,
      OS.iOS => code.iOS.deploymentVersion.major,
      _ => null,
    };
  } on StateError {
    return null; // A tool that doesn't say.
  }
}

String? _findCargo() {
  final home = Platform.environment['HOME'] ?? '';
  for (final dir in [...?Platform.environment['PATH']?.split(':'), '$home/.cargo/bin']) {
    if (dir.isNotEmpty && File('$dir/cargo').existsSync()) return '$dir/cargo';
  }
  return null;
}
