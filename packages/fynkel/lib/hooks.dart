// Builds a crate of the Fynkel Cargo workspace for a build hook's target
// and hands it to the Dart and Flutter tools as a code asset. package:fynkel's
// hook builds the engine with it, and package:fynkel_physics's the physics.
//
// It builds only from the Cargo workspace the calling package sits in
// (packages/<name> → the repository root), always in release: a debug build
// is many times slower. Cargo is incremental, so an unchanged crate costs a
// second or two.

import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:hooks/hooks.dart';

import 'src/build_plan.dart';

export 'src/build_plan.dart' show HookException;

/// Builds [crate] (a cdylib in the workspace's crates/) for the target in
/// [input], and adds it to [output] as the code asset named [asset] of the
/// package whose hook this is.
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
    throw HookException('expected the Fynkel Cargo workspace, with crates/$crate, at ${workspace.toFilePath()}.');
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
  for (final entity in Directory.fromUri(workspace.resolve('crates/')).listSync(recursive: true)) {
    final path = entity.path;
    if (entity is File && (path.endsWith('.rs') || path.endsWith('.wgsl') || path.endsWith('Cargo.toml'))) {
      output.dependencies.add(entity.uri);
    }
  }
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
      OS.macOS => code.macOS.targetVersion,
      OS.iOS => code.iOS.targetVersion,
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
