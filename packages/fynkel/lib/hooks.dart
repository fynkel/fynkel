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
  final triple = rustTriple(os, arch, iosSdk: os == OS.iOS ? code.iOS.targetSdk : null);
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

  final args = ['build', '--release', '--package', crate, '--target', triple];
  final result = await Process.run(
    cargo,
    args,
    workingDirectory: workspace.toFilePath(),
    environment: {
      'PATH': '${Platform.environment['PATH'] ?? ''}:${File(cargo).parent.path}',
      ...?_androidLinker(os, arch, triple),
    },
  );
  if (result.exitCode != 0) {
    // A missing standard library for the target is the usual first-time
    // failure (Flutter builds every architecture it ships).
    final hint = '${result.stderr}'.contains('E0463') ? '\nInstall the target: rustup target add $triple' : '';
    throw HookException('cargo ${args.join(' ')} failed (exit ${result.exitCode}):\n${result.stderr}$hint');
  }

  final fileName = os.dylibFileName(crate);
  final built = workspace.resolve('target/$triple/release/$fileName');
  if (!File.fromUri(built).existsSync()) throw HookException('cargo succeeded but ${built.toFilePath()} is missing.');
  final dest = input.outputDirectory.resolve(fileName);
  await File.fromUri(built).copy(dest.toFilePath());

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
String? rustTriple(OS os, Architecture arch, {IOSSdk? iosSdk}) {
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

/// Android needs the NDK's clang as the linker, for API 30 (Android 11),
/// package:fynkel_flutter's floor. The NDK is ANDROID_NDK_HOME, or the
/// newest one in the SDK (ANDROID_HOME, or Android Studio's default place).
Map<String, String>? _androidLinker(OS os, Architecture arch, String triple) {
  if (os != OS.android) return null;
  final env = Platform.environment;
  var ndk = env['ANDROID_NDK_HOME'];
  if (ndk == null) {
    final sdk = env['ANDROID_HOME'] ?? env['ANDROID_SDK_ROOT'] ?? '${env['HOME'] ?? ''}/Library/Android/sdk';
    final dir = Directory('$sdk/ndk');
    final versions = dir.existsSync() ? (dir.listSync().map((e) => e.path).toList()..sort()) : <String>[];
    if (versions.isEmpty) {
      throw HookException(
        'Android builds need the Android NDK, and there is none in $sdk/ndk. '
        "Install it with Android Studio's SDK Manager (SDK Tools → NDK), or set ANDROID_NDK_HOME.",
      );
    }
    ndk = versions.last;
  }
  // The NDK's prebuilt tools for macOS are universal binaries in
  // darwin-x86_64, Apple silicon included.
  final clang = '$ndk/toolchains/llvm/prebuilt/darwin-x86_64/bin/${triple}30-clang';
  if (!File(clang).existsSync()) throw HookException('the NDK at $ndk has no $clang.');
  return {'CARGO_TARGET_${triple.toUpperCase().replaceAll('-', '_')}_LINKER': clang};
}

String? _findCargo() {
  final home = Platform.environment['HOME'] ?? '';
  for (final dir in [...?Platform.environment['PATH']?.split(':'), '$home/.cargo/bin']) {
    if (dir.isNotEmpty && File('$dir/cargo').existsSync()) return '$dir/cargo';
  }
  return null;
}

/// What a build hook throws when it can't build.
class HookException implements Exception {
  HookException(this.message);
  final String message;
  @override
  String toString() => 'fynkel build hook: $message';
}
