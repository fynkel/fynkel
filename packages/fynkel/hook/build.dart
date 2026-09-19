// Native-assets build hook for package:fynkel (ADR-0015, blueprint §13.4).
//
// Provides the `package:fynkel/fynkel_native` code asset that the `@Native`
// bindings resolve through. Resolution order:
//
//   1. `hooks.user_defines.fynkel.native_lib` — an explicit library path.
//   2. Fynkel source checkout (this package lives inside the Cargo workspace)
//      — always `cargo build` so the runtime matches the working tree.
//   3. Prebuilt artifact from the release store named by fynkel.lock.json,
//      SHA-256 verified, cached in the shared output directory.
//   4. `cargo build` from the vendored source when a Rust toolchain exists and
//      the artifact is unpublished, or `build_from_source: true` is set.
//
// Anything else fails with a diagnostic that names the fix.
//
// User-defines (in the app's pubspec.yaml):
//
//   hooks:
//     user_defines:
//       fynkel:
//         lock_file: fynkel.lock.json   # project runtime pin (default: the package's)
//         build_from_source: true       # skip the download
//         native_lib: path/to/lib.dylib # use this library as-is
//
// Hooks run with a stripped environment (only PATH/HOME and a few tool
// variables survive), so `FYNKEL_BUILD_FROM_SOURCE` is a user-define here.

import 'dart:ffi' show Abi;
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:crypto/crypto.dart';
import 'package:fynkel/src/hook/resolve.dart';
import 'package:hooks/hooks.dart';

void main(List<String> args) async {
  await build(args, (input, output) async {
    if (!input.config.buildCodeAssets) return;
    final code = input.config.code;
    final os = code.targetOS;
    final arch = code.targetArchitecture;
    final iosSdk = os == OS.iOS ? code.iOS.targetSdk : null;
    final key = targetKey(os, arch, iosSdk: iosSdk);
    final crate = crateFor(os);
    final log = _Log(input.packageName);

    final lock = _loadLock(input, output);
    final workspace = _sourceWorkspace(input.packageRoot);
    final userLib = input.userDefines.path('native_lib');
    final forceSource = _flag(input, 'build_from_source');

    final Uri built;
    final RuntimeSource source;
    if (userLib != null) {
      if (!File.fromUri(userLib).existsSync()) {
        throw _fail('hooks.user_defines.fynkel.native_lib points at ${userLib.toFilePath()}, which does not exist.');
      }
      output.dependencies.add(userLib);
      built = userLib;
      source = RuntimeSource.userDefine;
    } else if (workspace != null) {
      built = await _cargoBuild(workspace, crate, os, arch, iosSdk, output, log);
      source = RuntimeSource.workspace;
    } else {
      final url = lock.artifactUrl(key);
      final cargo = _findCargo();
      if (url != null && !forceSource) {
        built = await _download(input, lock, key, url, log);
        source = RuntimeSource.prebuilt;
      } else if (cargo != null) {
        final vendored = _vendoredWorkspace(input.packageRoot);
        if (vendored == null) {
          throw _fail('No prebuilt Fynkel runtime for $key and no Rust source alongside the package. '
              'Set hooks.user_defines.fynkel.native_lib to a built library, or use a Fynkel source checkout.');
        }
        built = await _cargoBuild(vendored, crate, os, arch, iosSdk, output, log);
        source = RuntimeSource.cargoFallback;
      } else {
        final why = url == null
            ? 'fynkel.lock.json has no published artifact for target "$key" (runtime ${lock.runtimeVersion})'
            : 'build_from_source is set';
        throw _fail('$why and no Rust toolchain was found (looked for `cargo` on PATH and in ~/.cargo/bin).\n'
            'Fix one of:\n'
            '  - install Rust (https://rustup.rs) so the runtime can be built from source;\n'
            '  - point hooks.user_defines.fynkel.native_lib at a prebuilt library;\n'
            '  - pin a runtime version whose artifacts are published in fynkel.lock.json.');
      }
    }

    final fileName = cargoLibraryFileName(crate, os);
    final dest = input.outputDirectory.resolve(fileName);
    await Directory.fromUri(input.outputDirectory).create(recursive: true);
    await File.fromUri(built).copy(dest.toFilePath());
    log.info('runtime for $key from ${source.name}: ${dest.toFilePath()}');

    output.assets.code.add(CodeAsset(
      package: input.packageName,
      name: assetName,
      linkMode: DynamicLoadingBundled(),
      file: dest,
    ));
  });
}

// ---- lock file -------------------------------------------------------------

RuntimeLock _loadLock(BuildInput input, BuildOutputBuilder output) {
  final override = input.userDefines.path('lock_file');
  final uri = override ?? input.packageRoot.resolve('fynkel.lock.json');
  final file = File.fromUri(uri);
  if (!file.existsSync()) {
    throw _fail('${uri.toFilePath()} not found (hooks.user_defines.fynkel.lock_file${override == null ? ' is unset' : ''}).');
  }
  output.dependencies.add(uri);
  try {
    return RuntimeLock.parse(file.readAsStringSync());
  } on FormatException catch (e) {
    throw _fail('${uri.toFilePath()}: ${e.message}');
  }
}

bool _flag(BuildInput input, String key) {
  final v = input.userDefines[key];
  if (v is! bool?) throw _fail('hooks.user_defines.fynkel.$key must be a boolean');
  return v ?? false;
}

// ---- source builds ---------------------------------------------------------

/// The Fynkel Cargo workspace when this package is checked out inside it
/// (packages/fynkel → repo root).
Uri? _sourceWorkspace(Uri packageRoot) {
  final root = packageRoot.resolve('../../');
  return _isFynkelWorkspace(root) ? root : null;
}

/// Rust source vendored next to a published package (`<package>/native/`).
Uri? _vendoredWorkspace(Uri packageRoot) {
  final root = packageRoot.resolve('native/');
  return _isFynkelWorkspace(root) ? root : null;
}

bool _isFynkelWorkspace(Uri root) {
  final manifest = File.fromUri(root.resolve('Cargo.toml'));
  if (!manifest.existsSync()) return false;
  final text = manifest.readAsStringSync();
  return text.contains('[workspace]') && text.contains('fynkel_native');
}

String? _findCargo() {
  final home = Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'];
  final exe = Platform.isWindows ? 'cargo.exe' : 'cargo';
  final sep = Platform.isWindows ? ';' : ':';
  final dirs = [
    ...?Platform.environment['PATH']?.split(sep),
    if (home != null) '$home/.cargo/bin',
  ];
  for (final d in dirs) {
    if (d.isEmpty) continue;
    final candidate = '$d${Platform.pathSeparator}$exe';
    if (File(candidate).existsSync()) return candidate;
  }
  return null;
}

Future<Uri> _cargoBuild(
  Uri workspace,
  String crate,
  OS os,
  Architecture arch,
  IOSSdk? iosSdk,
  BuildOutputBuilder output,
  _Log log,
) async {
  final cargo = _findCargo();
  if (cargo == null) {
    throw _fail('Fynkel source checkout at ${workspace.toFilePath()} but no `cargo` on PATH or in ~/.cargo/bin. '
        'Install Rust (https://rustup.rs) or set hooks.user_defines.fynkel.native_lib.');
  }
  final triple = rustTriple(os, arch, iosSdk: iosSdk);
  if (triple == null) throw _fail('Fynkel does not build for ${os.name}/${arch.name}${iosSdk == null ? '' : ' ($iosSdk)'}.');
  final crossCompile = triple != _hostTriple();
  final args = ['build', '--release', '-p', crate, if (crossCompile) ...['--target', triple]];
  log.info('${workspace.toFilePath()}: cargo ${args.join(' ')}');
  final result = await Process.run(
    cargo,
    args,
    workingDirectory: workspace.toFilePath(),
    environment: {'PATH': '${Platform.environment['PATH'] ?? ''}:${File(cargo).parent.path}'},
  );
  if (result.exitCode != 0) {
    final stderr = '${result.stderr}';
    // Flutter builds universal macOS apps by default, so the hook runs once
    // per architecture; a missing std for the other one is the usual cause.
    final hint = stderr.contains('E0463') ? '\nInstall the target: `rustup target add $triple`.' : '';
    throw _fail('cargo build failed (exit ${result.exitCode}):\n$stderr$hint');
  }
  final lib = workspace.resolve('target/${crossCompile ? '$triple/' : ''}release/${cargoLibraryFileName(crate, os)}');
  if (!File.fromUri(lib).existsSync()) throw _fail('cargo reported success but ${lib.toFilePath()} is missing');

  // Re-run when the Rust sources change; cargo itself is incremental.
  output.dependencies.add(workspace.resolve('Cargo.toml'));
  output.dependencies.add(workspace.resolve('Cargo.lock'));
  for (final dir in const ['crates', 'platform']) {
    final d = Directory.fromUri(workspace.resolve('$dir/'));
    if (!d.existsSync()) continue;
    for (final entity in d.listSync(recursive: true, followLinks: false)) {
      if (entity is! File) continue;
      final path = entity.path;
      if (path.endsWith('.rs') || path.endsWith('.wgsl') || path.endsWith('Cargo.toml') || path.endsWith('.h')) {
        output.dependencies.add(entity.uri);
      }
    }
  }
  return lib;
}

String _hostTriple() {
  final arch = switch (Abi.current()) {
    Abi.macosArm64 || Abi.linuxArm64 || Abi.windowsArm64 => 'aarch64',
    _ => 'x86_64',
  };
  if (Platform.isMacOS) return '$arch-apple-darwin';
  if (Platform.isWindows) return '$arch-pc-windows-msvc';
  return '$arch-unknown-linux-gnu';
}

// ---- prebuilt artifacts ----------------------------------------------------

Future<Uri> _download(BuildInput input, RuntimeLock lock, String key, Uri url, _Log log) async {
  final artifact = lock.artifacts[key]!;
  final cacheDir = input.outputDirectoryShared.resolve('runtime-${lock.runtimeVersion}/');
  await Directory.fromUri(cacheDir).create(recursive: true);
  final cached = File.fromUri(cacheDir.resolve(artifact.file));
  if (cached.existsSync() && await _sha256(cached) == artifact.sha256) {
    log.info('using cached ${cached.path}');
    return cached.uri;
  }
  log.info('downloading $url');
  final client = HttpClient();
  try {
    final response = await (await client.getUrl(url)).close();
    if (response.statusCode != 200) {
      throw _fail('download of $url failed with HTTP ${response.statusCode}. '
          'If you are offline, install Rust to build from source or set hooks.user_defines.fynkel.native_lib.');
    }
    final tmp = File('${cached.path}.part');
    await response.pipe(tmp.openWrite());
    final sum = await _sha256(tmp);
    if (sum != artifact.sha256) {
      await tmp.delete();
      throw _fail('checksum mismatch for $url: expected ${artifact.sha256}, got $sum. Refusing to use it.');
    }
    await tmp.rename(cached.path);
  } on SocketException catch (e) {
    throw _fail('could not reach the release store ($url): ${e.message}. '
        'Install Rust to build from source or set hooks.user_defines.fynkel.native_lib.');
  } finally {
    client.close(force: true);
  }
  return cached.uri;
}

Future<String> _sha256(File f) async => (await sha256.bind(f.openRead()).first).toString();

// ---- diagnostics -----------------------------------------------------------

class _Log {
  _Log(this.package);
  final String package;
  void info(String msg) => stderr.writeln('[$package hook] $msg');
}

Exception _fail(String message) => Exception('fynkel hook: $message');
