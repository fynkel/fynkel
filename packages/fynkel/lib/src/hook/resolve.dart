// Pure helpers for hook/build.dart (ADR-0015): target naming, the runtime
// lock file, and the source-workspace mapping. Kept free of hook I/O so they
// are unit-testable.

import 'dart:convert';

import 'package:code_assets/code_assets.dart';

/// The code asset every `@Native` binding in this package resolves through.
const assetName = 'fynkel_native';

/// Which native crate serves a target. Apple targets ship the platform dylib
/// (it exports the `fynkel_*` ABI plus the IOSurface viewport); everything
/// else gets the bare ABI crate.
String crateFor(OS os) => os == OS.macOS || os == OS.iOS ? 'fynkel_platform_apple' : 'fynkel_native';

/// `<os>[-simulator]-<arch>` key used by `fynkel.lock.json` and the release
/// store file names.
String targetKey(OS os, Architecture arch, {IOSSdk? iosSdk}) {
  final sim = os == OS.iOS && iosSdk == IOSSdk.iPhoneSimulator ? '-simulator' : '';
  return '${os.name}$sim-${arch.name}';
}

/// Rust target triple, or null when the target is not one we build.
String? rustTriple(OS os, Architecture arch, {IOSSdk? iosSdk}) {
  final a = switch (arch) {
    Architecture.arm64 => 'aarch64',
    Architecture.x64 => 'x86_64',
    _ => null,
  };
  if (a == null) return null;
  return switch (os) {
    OS.macOS => '$a-apple-darwin',
    OS.iOS when iosSdk == IOSSdk.iPhoneSimulator => arch == Architecture.arm64 ? 'aarch64-apple-ios-sim' : '$a-apple-ios',
    OS.iOS => arch == Architecture.arm64 ? 'aarch64-apple-ios' : null,
    OS.linux => '$a-unknown-linux-gnu',
    OS.windows => '$a-pc-windows-msvc',
    _ => null,
  };
}

/// Cargo features of the native crates an app may opt into through
/// `hooks.user_defines.fynkel.features`. Anything else is refused by the
/// hook. `authoring`: the editor's authoring ABI (`fynkel_authoring_*`,
/// `include/fynkel_authoring.h`); a game never lists it, so its runtime
/// carries no editor code (blueprint §19.2e item 1).
const knownFeatures = {'authoring'};

/// The optional engine modules (ADR-0029), each a cargo feature of the
/// runtime crates, on in every build unless the project strips it.
const moduleFeatures = ['physics', 'audio', 'animation'];

/// The module features to build for a project file's JSON ([project]
/// null: no project named, every module). Only `modules.strip: true`
/// leaves anything out — the modules the project turns off; without it
/// an off module is off in behaviour only (the runtime keeps its code).
List<String> moduleFeaturesFor(Map<String, Object?>? project) {
  final modules = project?['modules'];
  if (modules is! Map || modules['strip'] != true) return moduleFeatures;
  return [
    for (final m in moduleFeatures)
      if (modules[m] != false) m,
  ];
}

/// The `cargo build` arguments for one target: always `--no-default-features`
/// so the built runtime carries exactly the features the app asked for — the
/// crates' defaults are for the workspace, not for a game.
List<String> cargoBuildArgs(String crate, {String? targetTriple, Iterable<String> features = const []}) => [
      'build',
      '--release',
      '-p',
      crate,
      '--no-default-features',
      if (features.isNotEmpty) ...['--features', (features.toList()..sort()).join(',')],
      if (targetTriple != null) ...['--target', targetTriple],
    ];

/// Dynamic library file name cargo produces for [crate] on [os].
String cargoLibraryFileName(String crate, OS os) => switch (os) {
      OS.windows => '$crate.dll',
      OS.macOS || OS.iOS => 'lib$crate.dylib',
      _ => 'lib$crate.so',
    };

/// One prebuilt artifact entry from the lock file.
class LockArtifact {
  const LockArtifact({required this.file, required this.sha256});
  final String file;

  /// Lower-case hex SHA-256, or null when the artifact is not published.
  final String? sha256;
  bool get isPublished => sha256 != null && sha256!.isNotEmpty;
}

/// `fynkel.lock.json` (blueprint §13.5), the parts the build hook needs.
class RuntimeLock {
  const RuntimeLock({
    required this.runtimeVersion,
    required this.abiVersion,
    required this.releaseBaseUrl,
    required this.artifacts,
  });

  final String runtimeVersion;
  final int abiVersion;

  /// May contain `{version}`, substituted with [runtimeVersion].
  final String releaseBaseUrl;
  final Map<String, LockArtifact> artifacts;

  static RuntimeLock parse(String json) {
    final root = jsonDecode(json);
    if (root is! Map<String, dynamic>) throw const FormatException('fynkel.lock.json: expected an object');
    final version = root['runtime_version'];
    if (version is! String || version.isEmpty) {
      throw const FormatException('fynkel.lock.json: "runtime_version" must be a non-empty string');
    }
    final abi = root['abi_version'];
    if (abi is! int) throw const FormatException('fynkel.lock.json: "abi_version" must be an integer');
    final base = root['release_base_url'];
    if (base is! String) throw const FormatException('fynkel.lock.json: "release_base_url" must be a string');
    final rawArtifacts = root['artifacts'];
    if (rawArtifacts is! Map<String, dynamic>) {
      throw const FormatException('fynkel.lock.json: "artifacts" must be an object keyed by target');
    }
    final artifacts = <String, LockArtifact>{};
    for (final entry in rawArtifacts.entries) {
      final v = entry.value;
      if (v is! Map<String, dynamic> || v['file'] is! String) {
        throw FormatException('fynkel.lock.json: artifacts.${entry.key} needs a "file" string');
      }
      final sha = v['sha256'];
      if (sha != null && (sha is! String || !_isSha256(sha))) {
        throw FormatException('fynkel.lock.json: artifacts.${entry.key}.sha256 must be 64 hex chars or null');
      }
      artifacts[entry.key] = LockArtifact(file: v['file'] as String, sha256: (sha as String?)?.toLowerCase());
    }
    return RuntimeLock(runtimeVersion: version, abiVersion: abi, releaseBaseUrl: base, artifacts: artifacts);
  }

  /// Download URL for [key], or null when the lock has no published artifact.
  Uri? artifactUrl(String key) {
    final a = artifacts[key];
    if (a == null || !a.isPublished) return null;
    final base = releaseBaseUrl.replaceAll('{version}', runtimeVersion);
    return Uri.parse(base.endsWith('/') ? base : '$base/').resolve(a.file);
  }

  static bool _isSha256(String s) => s.length == 64 && RegExp(r'^[0-9a-fA-F]+$').hasMatch(s);
}

/// Describes how the hook obtained the runtime; goes into the diagnostics.
enum RuntimeSource { userDefine, workspace, prebuilt, cargoFallback }
