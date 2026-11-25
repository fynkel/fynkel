// The libraries of a released package, built already: what lib/hooks.dart
// takes when the package isn't in the Fynkel repository, so a game needs no
// Rust. tool/prebuilt.dart builds them and writes each package's
// prebuilt.json, which says where they are and the hash each must have.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';

import 'build_plan.dart';

/// The seven targets Fynkel is released for.
const prebuiltTriples = [
  'aarch64-apple-darwin',
  'x86_64-apple-darwin',
  'aarch64-apple-ios',
  'aarch64-apple-ios-sim',
  'x86_64-apple-ios',
  'aarch64-linux-android',
  'x86_64-linux-android',
];

/// The file [crate]'s library for [triple] is released as.
String prebuiltName(String crate, String triple) => '$crate-$triple.${triple.endsWith('-android') ? 'so' : 'dylib'}';

/// A package's prebuilt.json.
class Prebuilt {
  /// The libraries [files] of [crate], at [url].
  Prebuilt({required this.crate, required this.url, required this.files});

  /// Reads [json], the text of [path] (named in errors).
  factory Prebuilt.parse(String json, String path) {
    try {
      final map = jsonDecode(json) as Map<String, Object?>;
      return Prebuilt(
        crate: map['crate'] as String,
        url: map['url'] as String,
        files: {
          for (final MapEntry(:key, :value) in (map['files'] as Map<String, Object?>).entries)
            key: (sha256: (value as Map<String, Object?>)['sha256'] as String, bytes: value['bytes'] as int),
        },
      );
    } on Object catch (e) {
      throw HookException('$path is damaged ($e). Run `dart pub cache repair`.');
    }
  }

  /// The crate the libraries are built from.
  final String crate;

  /// The folder they are downloaded from.
  final String url;

  /// Each target's library: its SHA-256, in hex, and its size.
  final Map<String, ({String sha256, int bytes})> files;

  /// The text [Prebuilt.parse] reads.
  String toJson() =>
      '${const JsonEncoder.withIndent('  ').convert({
        'crate': crate,
        'url': url,
        'files': {
          for (final MapEntry(:key, :value) in files.entries) key: {'sha256': value.sha256, 'bytes': value.bytes},
        },
      })}\n';

  /// The library for [triple], in [cache] (downloaded on first use, from
  /// [mirror] if given), checked against its hash every time.
  Future<File> fetch(
    String triple, {
    required Directory cache,
    String? mirror,
    void Function(String text)? onOutput,
    Duration stall = const Duration(seconds: 30),
    Duration retryAfter = const Duration(seconds: 1),
  }) async {
    final want = files[triple];
    if (want == null) throw HookException('this release of Fynkel has no $crate for $triple.');
    final name = prebuiltName(crate, triple);
    // Under its hash: another version's library is another file.
    final file = File('${cache.path}/${want.sha256}/$name');
    if (file.existsSync()) {
      if (await _hash(file) == want.sha256) return file;
      file.deleteSync(); // Damaged on disk.
    }

    final base = mirror ?? url;
    final from = Uri.parse('${base.endsWith('/') ? base : '$base/'}$name');
    onOutput?.call('fynkel: downloading $from (${(want.bytes / 1e6).toStringAsFixed(1)} MB, once)\n');
    file.parent.createSync(recursive: true);
    // Beside it, then renamed: a build stopped half way leaves nothing that
    // looks whole, and two at once don't write into each other's.
    final part = File('${file.path}.$pid.part');
    try {
      for (var attempt = 1; ; attempt++) {
        try {
          await _download(from, part, want.bytes, stall);
          break;
        } on _Refused catch (e) {
          if (attempt == 3 || !e.worthRetrying) throw HookException('$from: the server answered ${e.status}.');
        } on FileSystemException {
          rethrow; // The disk's trouble, not the network's.
        } on Exception catch (e) {
          if (attempt == 3) {
            throw HookException(
              'could not download $from ($e).\n'
              'The first build for a platform downloads the engine: check the connection (or HTTPS_PROXY, behind '
              'a proxy) and build again. Later builds need no network.',
            );
          }
        }
        await Future<void>.delayed(retryAfter * attempt);
      }
      final got = await _hash(part);
      if (got != want.sha256) {
        throw HookException(
          '$from is not the file this package was released with, so it was not used: its SHA-256 is $got, '
          'not ${want.sha256}.',
        );
      }
      part.renameSync(file.path);
    } finally {
      if (part.existsSync()) part.deleteSync();
    }
    return file;
  }
}

Future<String> _hash(File file) async => '${await sha256.bind(file.openRead()).first}';

/// An HTTP answer other than the file.
class _Refused implements Exception {
  _Refused(this.status);
  final int status;
  bool get worthRetrying => status >= 500 || status == 429 || status == 408;
}

Future<void> _download(Uri from, File to, int bytes, Duration stall) async {
  final client = HttpClient()
    ..connectionTimeout = stall
    ..findProxy = HttpClient.findProxyFromEnvironment;
  try {
    final request = await client.getUrl(from).timeout(stall);
    final response = await request.close().timeout(stall);
    if (response.statusCode != HttpStatus.ok) throw _Refused(response.statusCode);
    final out = await to.open(mode: FileMode.write);
    try {
      var got = 0;
      await for (final chunk in response.timeout(stall)) {
        got += chunk.length;
        // Not the file: don't fill the disk with it.
        if (got > bytes) throw HttpException('it is longer than the $bytes bytes it should be', uri: from);
        await out.writeFrom(chunk);
      }
      if (got < bytes) throw HttpException('it stopped after $got of $bytes bytes', uri: from);
    } finally {
      await out.close();
    }
  } finally {
    client.close(force: true);
  }
}
