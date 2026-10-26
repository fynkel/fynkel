// Hardening B1: the build hook works on any Mac with Flutter's own setup,
// not only on the one it was written on.

import 'dart:async';
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:fynkel/src/build_plan.dart';
import 'package:test/test.dart';

/// A fake NDK under a temporary SDK: [versions] in ndk/, each with the
/// clang wrappers for API [apis] on [host].
({String sdk, String Function(String version) bin}) fakeSdk(
  List<String> versions, {
  String host = 'linux-x86_64',
  List<int> apis = const [24, 30],
}) {
  final sdk = Directory.systemTemp.createTempSync('fynkel_sdk').path;
  addTearDown(() => Directory(sdk).deleteSync(recursive: true));
  String bin(String version) => '$sdk/ndk/$version/toolchains/llvm/prebuilt/$host/bin';
  for (final version in versions) {
    Directory(bin(version)).createSync(recursive: true);
    File('${bin(version)}/clang').writeAsStringSync('');
    for (final api in apis) {
      File('${bin(version)}/aarch64-linux-android$api-clang').writeAsStringSync('');
    }
  }
  return (sdk: sdk, bin: bin);
}

void main() {
  test('cargo builds with --locked, in release, for the target', () {
    expect(cargoArgs('fynkel_ffi', 'aarch64-apple-darwin'), [
      'build',
      '--release',
      '--locked',
      '--package',
      'fynkel_ffi',
      '--target',
      'aarch64-apple-darwin',
    ]);
  });

  test("macOS and iOS get Flutter's deployment target, others none", () {
    expect(deploymentTarget(OS.macOS, 13), {'MACOSX_DEPLOYMENT_TARGET': '13.0'});
    expect(deploymentTarget(OS.iOS, 13), {'IPHONEOS_DEPLOYMENT_TARGET': '13.0'});
    expect(deploymentTarget(OS.macOS, null), isEmpty, reason: 'nothing given: rustc picks');
    expect(deploymentTarget(OS.android, 30), isEmpty);
  });

  group('the newest NDK', () {
    test('is picked by number, not by name', () {
      expect(newestNdk(['9.1.2', '25.2.9519653', '27.0.12077973', '26.3.11579264']), '27.0.12077973');
      expect(newestNdk(['27.0.12077973', '27.1.12297006']), '27.1.12297006');
    });

    test('ignores what is not a version', () {
      expect(newestNdk(['.DS_Store', '25.2.9519653', 'temp']), '25.2.9519653');
      expect(newestNdk(['.DS_Store']), isNull);
      expect(newestNdk([]), isNull);
    });
  });

  group("Android's linker", () {
    test("is the wrapper next to Flutter's clang, for the config's API", () {
      final sdk = fakeSdk(['27.0.12077973']);
      final clang = Uri.file('${sdk.bin('27.0.12077973')}/clang');
      expect(
        androidLinker(triple: 'aarch64-linux-android', api: 24, compiler: clang, env: const {}),
        '${sdk.bin('27.0.12077973')}/aarch64-linux-android24-clang',
      );
    });

    test('without a compiler, comes from the newest NDK by number, on any host', () {
      final sdk = fakeSdk(['9.0.0', '27.0.12077973'], host: 'linux-x86_64');
      File('${sdk.sdk}/ndk/.DS_Store').writeAsStringSync('');
      expect(
        androidLinker(triple: 'aarch64-linux-android', api: 30, compiler: null, env: {'ANDROID_HOME': sdk.sdk}),
        '${sdk.bin('27.0.12077973')}/aarch64-linux-android30-clang',
      );
    });

    test('takes ANDROID_NDK_HOME first', () {
      final sdk = fakeSdk(['26.0.0', '27.0.12077973']);
      expect(
        androidLinker(
          triple: 'aarch64-linux-android',
          api: 30,
          compiler: null,
          env: {'ANDROID_HOME': sdk.sdk, 'ANDROID_NDK_HOME': '${sdk.sdk}/ndk/26.0.0'},
        ),
        '${sdk.bin('26.0.0')}/aarch64-linux-android30-clang',
      );
    });

    test('says what is missing when the NDK has no wrapper for the API', () {
      final sdk = fakeSdk(['27.0.12077973'], apis: [30]);
      final clang = Uri.file('${sdk.bin('27.0.12077973')}/clang');
      expect(
        () => androidLinker(triple: 'aarch64-linux-android', api: 21, compiler: clang, env: const {}),
        throwsA(isA<HookException>().having((e) => e.message, 'message', contains('aarch64-linux-android21-clang'))),
      );
    });

    test('says how to get an NDK when there is none', () {
      final sdk = fakeSdk([]);
      expect(
        () => androidLinker(triple: 'aarch64-linux-android', api: 30, compiler: null, env: {'ANDROID_HOME': sdk.sdk}),
        throwsA(isA<HookException>().having((e) => e.message, 'message', contains('SDK Manager'))),
      );
    });
  });

  test("cargo's output streams while it runs, and the error keeps its end", () async {
    final seen = <String>[];
    final result = await runStreaming(
      '/bin/sh',
      ['-c', 'echo one; echo two >&2; exit 3'],
      workingDirectory: Directory.systemTemp.path,
      environment: const {},
      onOutput: seen.add,
    );
    expect(result.exitCode, 3);
    expect(result.stderr, contains('two'));
    expect(seen.join(), allOf(contains('one'), contains('two')));
  });

  // Stage 14's GD1: another cargo build of the same crate (a second app,
  // or `flutter test` beside `flutter build`) takes the built library
  // away for an instant as it links it again; the copy waits it out.
  group('copying what cargo built', () {
    late Directory dir;
    setUp(() {
      dir = Directory.systemTemp.createTempSync('fynkel_copy');
      addTearDown(() => dir.deleteSync(recursive: true));
    });

    test('waits for a library another build is linking again', () async {
      final built = File('${dir.path}/libfynkel.dylib');
      final dest = '${dir.path}/copy.dylib';
      Timer(const Duration(milliseconds: 30), () => built.writeAsStringSync('linked'));
      await copyBuilt(built, dest);
      expect(File(dest).readAsStringSync(), 'linked');
    });

    test('says what is missing if it never comes back', () async {
      final built = File('${dir.path}/libfynkel.dylib');
      await expectLater(
        copyBuilt(built, '${dir.path}/copy.dylib', patience: const Duration(milliseconds: 100)),
        throwsA(isA<HookException>().having((e) => e.message, 'message', contains(built.path))),
      );
    });
  });
}
