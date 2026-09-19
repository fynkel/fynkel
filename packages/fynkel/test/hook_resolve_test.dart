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

  test('lock file parses and resolves published artifacts only', () {
    final lock = RuntimeLock.parse('''
{
  "runtime_version": "1.2.3",
  "abi_version": 1,
  "release_base_url": "https://example.test/dl/v{version}",
  "artifacts": {
    "macos-arm64": {"file": "lib-macos-arm64.dylib", "sha256": "${'A' * 64}"},
    "ios-arm64": {"file": "lib-ios-arm64.dylib", "sha256": null}
  }
}''');
    expect(lock.runtimeVersion, '1.2.3');
    expect(lock.artifacts['macos-arm64']!.sha256, 'a' * 64);
    expect(lock.artifactUrl('macos-arm64'), Uri.parse('https://example.test/dl/v1.2.3/lib-macos-arm64.dylib'));
    expect(lock.artifacts['ios-arm64']!.isPublished, isFalse);
    expect(lock.artifactUrl('ios-arm64'), isNull);
    expect(lock.artifactUrl('windows-x64'), isNull);
  });

  test('lock file rejects malformed entries', () {
    expect(() => RuntimeLock.parse('[]'), throwsFormatException);
    expect(() => RuntimeLock.parse('{"runtime_version": "", "abi_version": 1, "release_base_url": "", "artifacts": {}}'),
        throwsFormatException);
    expect(
        () => RuntimeLock.parse('{"runtime_version": "1", "abi_version": 1, "release_base_url": "", '
            '"artifacts": {"macos-arm64": {"file": "x", "sha256": "nothex"}}}'),
        throwsFormatException);
    expect(
        () => RuntimeLock.parse('{"runtime_version": "1", "abi_version": 1, "release_base_url": "", '
            '"artifacts": {"macos-arm64": {"sha256": null}}}'),
        throwsFormatException);
  });

  test('the shipped lock file is valid and pins ABI v1', () {
    final lock = RuntimeLock.parse(File('fynkel.lock.json').readAsStringSync());
    expect(lock.abiVersion, 1);
    expect(lock.artifacts.keys, containsAll(['macos-arm64', 'macos-x64', 'ios-arm64', 'ios-simulator-arm64']));
  });
}
