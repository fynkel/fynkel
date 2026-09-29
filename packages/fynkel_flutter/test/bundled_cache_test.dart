import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

/// An asset bundle over a map, with the `AssetManifest.bin` Flutter's
/// tool would generate for those assets.
class _FakeBundle extends CachingAssetBundle {
  _FakeBundle(this.assets);

  final Map<String, List<int>> assets;

  @override
  Future<ByteData> load(String key) async {
    if (key == 'AssetManifest.bin') {
      final manifest = {
        for (final k in assets.keys)
          k: [
            {'asset': k},
          ],
      };
      return const StandardMessageCodec().encodeMessage(manifest)!;
    }
    final bytes = assets[key];
    if (bytes == null) throw StateError('asset not found: $key');
    return ByteData.sublistView(Uint8List.fromList(bytes));
  }
}

void main() {
  late Directory tmp;

  setUp(() async {
    tmp = await Directory.systemTemp.createTemp('fynkel_bundled_');
  });

  tearDown(() async {
    await tmp.delete(recursive: true);
  });

  test('returns null when the app bundles no cache', () async {
    final bundle = _FakeBundle({
      'assets/logo.png': [1, 2, 3],
    });
    expect(await unpackBundledCache(into: tmp.path, bundle: bundle), isNull);
    expect(File('${tmp.path}/index.json').existsSync(), isFalse);
  });

  test('unpacks the flat bundle into a cache directory and prunes stale objects', () async {
    final index = utf8.encode('{"fynkel":"cache-index","assets":[]}');
    final bundle = _FakeBundle({
      'fynkel_cache/index.json': index,
      'fynkel_cache/aaaa.fyna': [1, 2, 3],
      'fynkel_cache/bbbb.fyna': [4, 5],
      'fynkel_cache/README.md': utf8.encode('keep file, not an object'),
    });
    // A leftover from an earlier unpack that the bundle no longer names.
    final objects = Directory('${tmp.path}/objects')..createSync();
    File('${objects.path}/stale.fyna').writeAsBytesSync([9]);
    // An object already present is kept as is (content addressed).
    File('${objects.path}/aaaa.fyna').writeAsBytesSync([1, 2, 3]);

    final dir = await unpackBundledCache(into: tmp.path, bundle: bundle);
    expect(dir, tmp.path);
    expect(File('${tmp.path}/index.json').readAsBytesSync(), index);
    expect(File('${objects.path}/aaaa.fyna').readAsBytesSync(), [1, 2, 3]);
    expect(File('${objects.path}/bbbb.fyna').readAsBytesSync(), [4, 5]);
    expect(File('${objects.path}/stale.fyna').existsSync(), isFalse);
    expect(File('${objects.path}/README.md').existsSync(), isFalse);
    expect(objects.listSync().length, 2);
  });

  // Foundation 5.6: an unpack cut short (the app killed mid-write) left a
  // short object that every later launch kept, since it existed.
  test('an object cut short is written again; a write in progress never looks whole', () async {
    final whole = _container(4096);
    final bundle = _FakeBundle({'fynkel_cache/index.json': utf8.encode('{}'), 'fynkel_cache/cccc.fyna': whole});
    final objects = Directory('${tmp.path}/objects')..createSync();
    // The earlier launch's write stopped a third of the way in.
    File('${objects.path}/cccc.fyna').writeAsBytesSync(whole.sublist(0, whole.length ~/ 3));
    // And one stopped before its rename.
    File('${objects.path}/dddd.fyna.part').writeAsBytesSync([1, 2]);

    await unpackBundledCache(into: tmp.path, bundle: bundle);
    expect(File('${objects.path}/cccc.fyna').readAsBytesSync(), whole);
    expect(File('${objects.path}/dddd.fyna.part').existsSync(), isFalse);
    expect(objects.listSync().length, 1);

    // A whole object is kept as it is (not read from the bundle again).
    final kept = File('${objects.path}/cccc.fyna').lastModifiedSync();
    await Future<void>.delayed(const Duration(milliseconds: 20));
    await unpackBundledCache(into: tmp.path, bundle: bundle);
    expect(File('${objects.path}/cccc.fyna').lastModifiedSync(), kept);
  });
}

/// A `.fyna` as far as the unpack looks: the `FYNC` magic and the header's
/// whole-file length (bytes 24..32, little-endian), then filler.
List<int> _container(int length) {
  final bytes = Uint8List(length);
  bytes.setAll(0, ascii.encode('FYNC'));
  ByteData.sublistView(bytes).setUint64(24, length, Endian.little);
  for (var i = 48; i < length; i++) {
    bytes[i] = i & 0xff;
  }
  return bytes;
}
