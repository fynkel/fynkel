import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:fynkel/src/hook/resolve.dart';
import 'package:test/test.dart';

void main() {
  test('target keys and triples', () {
    expect(targetKey(OS.macOS, Architecture.arm64), 'macos-arm64');
    expect(targetKey(OS.iOS, Architecture.arm64, iosSdk: IOSSdk.iPhoneOS), 'ios-arm64');
    expect(targetKey(OS.iOS, Architecture.arm64, iosSdk: IOSSdk.iPhoneSimulator), 'ios-simulator-arm64');
    expect(rustTriple(OS.macOS, Architecture.arm64), 'aarch64-apple-darwin');
    expect(rustTriple(OS.macOS, Architecture.x64), 'x86_64-apple-darwin');
    expect(rustTriple(OS.iOS, Architecture.arm64, iosSdk: IOSSdk.iPhoneOS), 'aarch64-apple-ios');
    expect(rustTriple(OS.iOS, Architecture.arm64, iosSdk: IOSSdk.iPhoneSimulator), 'aarch64-apple-ios-sim');
    expect(rustTriple(OS.iOS, Architecture.x64, iosSdk: IOSSdk.iPhoneSimulator), 'x86_64-apple-ios');
    expect(rustTriple(OS.iOS, Architecture.x64, iosSdk: IOSSdk.iPhoneOS), isNull);
    expect(rustTriple(OS.android, Architecture.arm64), isNull);
  });

  test('crate and file names per OS', () {
    expect(crateFor(OS.macOS), 'fynkel_platform_apple');
    expect(crateFor(OS.iOS), 'fynkel_platform_apple');
    expect(crateFor(OS.linux), 'fynkel_native');
    expect(cargoLibraryFileName('fynkel_native', OS.macOS), 'libfynkel_native.dylib');
    expect(cargoLibraryFileName('fynkel_native', OS.windows), 'fynkel_native.dll');
    expect(cargoLibraryFileName('fynkel_native', OS.linux), 'libfynkel_native.so');
  });

  test('cargo arguments carry exactly the requested features', () {
    // A game: no feature, so the runtime has no editor code.
    expect(cargoBuildArgs('fynkel_platform_apple'), [
      'build',
      '--release',
      '-p',
      'fynkel_platform_apple',
      '--no-default-features',
    ]);
    // The editor: the authoring ABI, plus a cross-compile target.
    expect(cargoBuildArgs('fynkel_native', targetTriple: 'aarch64-apple-ios', features: ['authoring']), [
      'build',
      '--release',
      '-p',
      'fynkel_native',
      '--no-default-features',
      '--features',
      'authoring',
      '--target',
      'aarch64-apple-ios',
    ]);
    expect(knownFeatures, {'authoring'});
  });

  test('every module is built unless the project strips the ones it turns off (ADR-0029)', () {
    expect(moduleFeatures, ['physics', 'audio', 'animation']);
    expect(moduleFeaturesFor(null), moduleFeatures);
    expect(moduleFeaturesFor({'name': 'x'}), moduleFeatures);
    // Off without strip: off in behaviour only, the code stays.
    expect(
      moduleFeaturesFor({
        'modules': {'animation': false},
      }),
      moduleFeatures,
    );
    expect(
      moduleFeaturesFor({
        'modules': {'animation': false, 'audio': false, 'strip': true},
      }),
      ['physics'],
    );
    expect(
      moduleFeaturesFor({
        'modules': {'strip': true},
      }),
      moduleFeatures,
    );
    // A game's runtime: the modules as features, no editor code.
    expect(cargoBuildArgs('fynkel_platform_apple', features: moduleFeaturesFor(null)), [
      'build',
      '--release',
      '-p',
      'fynkel_platform_apple',
      '--no-default-features',
      '--features',
      'animation,audio,physics',
    ]);
  });

  test('lock file parses and resolves published artifacts only', () {
    final lock = RuntimeLock.parse('''
{
  "\$schema": "https://fynkel.dev/schemas/lock.schema.json",
  "formatVersion": 1,
  "runtime": {
    "version": "1.2.3",
    "abiVersion": 1,
    "store": "https://example.test/dl/v{version}",
    "artifacts": {
      "macos-arm64": {"file": "lib-macos-arm64.dylib", "sha256": "${'A' * 64}"},
      "ios-arm64": {"file": "lib-ios-arm64.dylib", "sha256": null}
    }
  },
  "tools": {"version": "1.2.3"}
}''');
    expect(lock.runtimeVersion, '1.2.3');
    expect(lock.artifacts['macos-arm64']!.sha256, 'a' * 64);
    expect(
      lock.artifactUrl('macos-arm64'),
      Uri.parse('https://example.test/dl/v1.2.3/lib-macos-arm64.dylib'),
    );
    expect(lock.artifacts['ios-arm64']!.isPublished, isFalse);
    expect(lock.artifactUrl('ios-arm64'), isNull);
    expect(lock.artifactUrl('windows-x64'), isNull);
  });

  test("a project's lock pins the package's runtime and may name another store", () {
    const package = RuntimeLock(
      runtimeVersion: '1.2.3',
      abiVersion: 1,
      store: 'https://example.test/v{version}/',
      artifacts: {'macos-arm64': LockArtifact(file: 'lib.dylib', sha256: 'ab')},
    );
    // A project's lock as `fynkel project lock` writes it: no store.
    final plain = RuntimeLock.parse('{"runtime": {"version": "1.2.3", "abiVersion": 1}}');
    expect(plain.store, isNull);
    expect(plain.artifacts, isEmpty);
    expect(
      package.under(plain).artifactUrl('macos-arm64'),
      Uri.parse('https://example.test/v1.2.3/lib.dylib'),
    );
    expect(package.under(null), same(package));
    // A mirror: the project's store, the package's libraries.
    const mirror = RuntimeLock(
      runtimeVersion: '1.2.3',
      abiVersion: 1,
      store: 'file:///store/runtime/{version}/',
    );
    expect(
      package.under(mirror).artifactUrl('macos-arm64'),
      Uri.parse('file:///store/runtime/1.2.3/lib.dylib'),
    );
    // Another runtime or ABI: refused, naming both versions.
    expect(
      () => package.under(const RuntimeLock(runtimeVersion: '1.2.2', abiVersion: 1)),
      throwsA(
        isA<FormatException>().having(
          (e) => e.message,
          'message',
          allOf(contains('1.2.2'), contains('1.2.3')),
        ),
      ),
    );
    expect(
      () => package.under(const RuntimeLock(runtimeVersion: '1.2.3', abiVersion: 2)),
      throwsFormatException,
    );
    // Without a store nothing downloads.
    expect(
      const RuntimeLock(
        runtimeVersion: '1',
        abiVersion: 1,
        artifacts: {'k': LockArtifact(file: 'f', sha256: 'ab')},
      ).artifactUrl('k'),
      isNull,
    );
  });

  test('lock file rejects malformed entries', () {
    expect(() => RuntimeLock.parse('[]'), throwsFormatException);
    expect(() => RuntimeLock.parse('{"runtime": {"version": "", "abiVersion": 1}}'), throwsFormatException);
    expect(
      () => RuntimeLock.parse('{"runtime": {"version": "1", "abiVersion": 1, "store": ""}}'),
      throwsFormatException,
    );
    expect(
      () => RuntimeLock.parse(
        '{"runtime": {"version": "1", "abiVersion": 1, "store": "x", '
        '"artifacts": {"macos-arm64": {"file": "x", "sha256": "nothex"}}}}',
      ),
      throwsFormatException,
    );
    expect(
      () => RuntimeLock.parse(
        '{"runtime": {"version": "1", "abiVersion": 1, "store": "x", "artifacts": {"macos-arm64": {"sha256": null}}}}',
      ),
      throwsFormatException,
    );
    // The old snake_case hook lock is gone (ADR-0029 §4, one lock shape).
    expect(
      () => RuntimeLock.parse(
        '{"runtime_version": "1", "abi_version": 1, "release_base_url": "", "artifacts": {}}',
      ),
      throwsFormatException,
    );
  });

  test('the source-build lock pins ABI v4 without incompatible prebuilt artifacts', () {
    final lock = RuntimeLock.parse(File('fynkel.lock.json').readAsStringSync());
    expect(lock.abiVersion, 4);
    expect(lock.store, isNull);
    expect(lock.artifacts, isEmpty);
  });
}
