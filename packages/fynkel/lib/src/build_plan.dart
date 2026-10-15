// What lib/hooks.dart asks cargo for, worked out from the hook's config and
// the machine, apart from running it so each part can be tested.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:code_assets/code_assets.dart';

/// cargo's arguments for [crate] on [triple]. `--locked`: the build uses
/// the versions in Cargo.lock, never newer ones it would pick by itself.
List<String> cargoArgs(String crate, String triple) => [
  'build',
  '--release',
  '--locked',
  '--package',
  crate,
  '--target',
  triple,
];

/// The deployment target [version] (a major version, as Flutter gives it) as
/// the variable rustc and cc read for [os]; empty off Apple, or without one.
Map<String, String> deploymentTarget(OS os, int? version) => switch (os) {
  _ when version == null => const {},
  OS.macOS => {'MACOSX_DEPLOYMENT_TARGET': '$version.0'},
  OS.iOS => {'IPHONEOS_DEPLOYMENT_TARGET': '$version.0'},
  _ => const {},
};

/// The newest of the NDK folders [names] by version number ("27.1" over
/// "9.0"), skipping anything that isn't a version; null if none is.
String? newestNdk(Iterable<String> names) {
  List<int>? parse(String name) {
    final parts = name.split('.');
    final numbers = parts.map(int.tryParse).toList();
    return numbers.contains(null) ? null : numbers.cast<int>();
  }

  int compare(List<int> a, List<int> b) {
    for (var i = 0; i < a.length && i < b.length; i++) {
      if (a[i] != b[i]) return a[i].compareTo(b[i]);
    }
    return a.length.compareTo(b.length);
  }

  String? best;
  List<int>? bestVersion;
  for (final name in names) {
    final version = parse(name);
    if (version == null) continue;
    if (bestVersion == null || compare(version, bestVersion) > 0) (best, bestVersion) = (name, version);
  }
  return best;
}

/// The NDK's clang wrapper that links for [triple] at Android API [api]:
/// the one beside [compiler] (the NDK's clang, which Flutter gives the
/// hook), or else the one in ANDROID_NDK_HOME or the newest NDK in the SDK,
/// whatever machine this is.
String androidLinker({
  required String triple,
  required int api,
  required Uri? compiler,
  required Map<String, String> env,
}) {
  final wrapper = '$triple$api-clang';
  final String bin;
  if (compiler != null) {
    bin = File.fromUri(compiler).parent.path;
  } else {
    var ndk = env['ANDROID_NDK_HOME'];
    if (ndk == null) {
      final sdk = env['ANDROID_HOME'] ?? env['ANDROID_SDK_ROOT'] ?? '${env['HOME'] ?? ''}/Library/Android/sdk';
      final dir = Directory('$sdk/ndk');
      final newest = dir.existsSync()
          ? newestNdk(dir.listSync().map((e) => e.uri.pathSegments.lastWhere((s) => s.isNotEmpty)))
          : null;
      if (newest == null) {
        throw HookException(
          'Android builds need the Android NDK, and there is none in $sdk/ndk. '
          "Install it with Android Studio's SDK Manager (SDK Tools → NDK), or set ANDROID_NDK_HOME.",
        );
      }
      ndk = '$sdk/ndk/$newest';
    }
    // One folder, named for the host: darwin-x86_64 (Apple silicon too),
    // linux-x86_64 or windows-x86_64.
    final prebuilt = Directory('$ndk/toolchains/llvm/prebuilt');
    final hosts = prebuilt.existsSync() ? prebuilt.listSync().whereType<Directory>().toList() : <Directory>[];
    if (hosts.isEmpty) throw HookException('the NDK at $ndk has no ${prebuilt.path}.');
    bin = '${hosts.first.path}/bin';
  }
  final linker = '$bin/$wrapper';
  if (!File(linker).existsSync()) {
    throw HookException('the NDK has no $wrapper in $bin: Android API $api needs an NDK that links for it.');
  }
  return linker;
}

/// Runs [executable], handing its output to [onOutput] as it comes (the
/// first build takes minutes), and keeps the end of its stderr for an error.
Future<({int exitCode, String stderr})> runStreaming(
  String executable,
  List<String> args, {
  required String workingDirectory,
  required Map<String, String> environment,
  required void Function(String text) onOutput,
}) async {
  final process = await Process.start(executable, args, workingDirectory: workingDirectory, environment: environment);
  const keep = 16 * 1024;
  final tail = StringBuffer();
  void err(String text) {
    onOutput(text);
    tail.write(text);
    if (tail.length > 2 * keep) {
      final s = tail.toString();
      tail
        ..clear()
        ..write(s.substring(s.length - keep));
    }
  }

  final done = [
    process.stdout.transform(utf8.decoder).listen(onOutput).asFuture<void>(),
    process.stderr.transform(utf8.decoder).listen(err).asFuture<void>(),
  ];
  final exitCode = await process.exitCode;
  await Future.wait(done);
  final s = tail.toString();
  return (exitCode: exitCode, stderr: s.length > keep ? s.substring(s.length - keep) : s);
}

/// What a build hook throws when it can't build.
class HookException implements Exception {
  /// The build failed for the reason in [message].
  HookException(this.message);

  /// What went wrong, in a sentence.
  final String message;
  @override
  String toString() => 'fynkel build hook: $message';
}
