// The cooked cache an app carries as Flutter assets, so it runs on a device
// that cannot see the project directory (an iPhone, a shipped build).
//
// `fynkel cook` / `fynkel run` stage `<project>/.fynkel/cache` into
// `<project>/fynkel_cache/` (`index.json` and the `*.fyna` objects flat
// beside it), the project's `pubspec.yaml` lists that directory under
// `assets:`, and at
// start-up the app calls [unpackBundledCache] to write the bundle to a
// writable directory the native runtime can open by path — Flutter assets
// are readable through `rootBundle` only, never as files, and the runtime
// reads a cache directory (`World.loadProject(dir, cacheDir: …)`).

import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/services.dart';

/// The asset directory a created project stages its cache into
/// (`fynkel cook`); the same name inside the app bundle.
const bundledCacheAssetDir = 'fynkel_cache';

/// Writes the bundled cache to a directory and returns its path, or `null`
/// when the app bundles none (no `fynkel_cache/index.json` asset — the
/// project was never cooked before the build).
///
/// The result is a cache directory (`index.json`, `objects/<key>.fyna`).
/// The objects are content-addressed: those already present under [into]
/// are kept when whole, the others copied, `index.json` always rewritten.
/// Every file is written under a temporary name and renamed, and a kept
/// object's header must give its own length, so an unpack cut short (the
/// app killed mid-write) is repaired by the next launch (foundation 5.6). [into]
/// defaults to `<system temp>/fynkel_cache` (the app's own tmp directory
/// on iOS and sandboxed macOS): unpacking again after an update is a few
/// milliseconds, and nothing needs to be cleaned up.
Future<String?> unpackBundledCache({String? into, AssetBundle? bundle}) async {
  final assets = bundle ?? rootBundle;
  final manifest = await AssetManifest.loadFromAssetBundle(assets);
  final index = '$bundledCacheAssetDir/index.json';
  final keys = manifest.listAssets().where((k) => k.startsWith('$bundledCacheAssetDir/')).toList();
  if (!keys.contains(index)) return null;
  final dir = Directory(into ?? '${Directory.systemTemp.path}/$bundledCacheAssetDir');
  final objects = Directory('${dir.path}/objects');
  await objects.create(recursive: true);
  final wanted = <String>{};
  for (final key in keys) {
    final name = key.substring(bundledCacheAssetDir.length + 1);
    if (!name.endsWith('.fyna') || name.contains('/')) continue;
    wanted.add(name);
    final file = File('${objects.path}/$name');
    if (await _isWhole(file)) continue;
    await _writeWhole(file, await assets.load(key));
  }
  await for (final entity in objects.list()) {
    if (entity is File && !wanted.contains(entity.uri.pathSegments.last)) {
      await entity.delete();
    }
  }
  await _writeWhole(File('${dir.path}/index.json'), await assets.load(index));
  return dir.path;
}

/// Writes [data] to `<file>.part` and renames it over [file]: a write cut
/// short leaves only the `.part`, which the next unpack removes.
Future<void> _writeWhole(File file, ByteData data) async {
  final part = File('${file.path}.part');
  await part.writeAsBytes(data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes), flush: true);
  await part.rename(file.path);
}

/// Whether [file] is a whole cooked container: the `FYNC` magic and the
/// header's whole-file length (bytes 24..32, little-endian; see
/// `fynkel_cooked`) equal to its size. 32 bytes are read.
Future<bool> _isWhole(File file) async {
  if (!await file.exists()) return false;
  final length = await file.length();
  if (length < 48) return false;
  final raf = await file.open();
  try {
    final head = await raf.read(32);
    if (head.length < 32 || String.fromCharCodes(head.sublist(0, 4)) != 'FYNC') return false;
    return ByteData.sublistView(head).getUint64(24, Endian.little) == length;
  } finally {
    await raf.close();
  }
}
