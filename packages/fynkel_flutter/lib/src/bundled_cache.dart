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
/// are kept, the others copied, `index.json` always rewritten. [into]
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
    if (await file.exists()) continue;
    final data = await assets.load(key);
    await file.writeAsBytes(data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes), flush: true);
  }
  await for (final entity in objects.list()) {
    if (entity is File && !wanted.contains(entity.uri.pathSegments.last)) {
      await entity.delete();
    }
  }
  final data = await assets.load(index);
  await File('${dir.path}/index.json')
      .writeAsBytes(data.buffer.asUint8List(data.offsetInBytes, data.lengthInBytes), flush: true);
  return dir.path;
}
