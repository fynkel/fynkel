// The licences of other people's code in the released libraries, which
// their terms ask to be handed on with them: what tool/release.dart writes
// to each package's THIRD_PARTY_LICENSES. Flutter adds it to the app's own
// licence page.

import 'dart:convert';
import 'dart:io';

import 'package:fynkel/src/prebuilt.dart';

/// The text for the libraries of [crate], which [package] ships. Throws
/// when the licence of something in them can't be found.
String notices({required String root, required String crate, required String package}) {
  final metadata =
      jsonDecode(_run(root, ['cargo', 'metadata', '--format-version', '1', '--locked'])) as Map<String, Object?>;
  final all = {
    for (final p in (metadata['packages']! as List<Object?>).cast<Map<String, Object?>>())
      '${p['name']} ${p['version']}': p,
  };
  // What is linked in, on any of the targets. Procedural macros and build
  // scripts' own crates run while it is built, and aren't.
  final used = <String>{
    for (final triple in prebuiltTriples)
      for (final line in const LineSplitter().convert(
        _run(root, [
          ...['cargo', 'tree', '--locked', '--package', crate, '--target', triple],
          ...['--edges', 'normal,no-proc-macro', '--prefix', 'none', '--format', '{p}'],
        ]),
      ))
        if (RegExp(r'^(\S+) v(\S+)').firstMatch(line) case final match? when !match[1]!.startsWith('fynkel_'))
          '${match[1]} ${match[2]}',
  }.toList()..sort();

  final licences = '${_run(root, ['rustc', '--print', 'sysroot']).trim()}/share/doc/rust/licenses';
  final apache = _read(File('$licences/Apache-2.0.txt'));
  final texts = <String, List<String>>{};
  void add(String text, String of) => (texts[text.trim()] ??= []).add(of);

  for (final name in used) {
    final p = all[name]!;
    final licence = p['license']! as String;
    final source = p['repository'] ?? 'https://crates.io/crates/${p['name']}';
    final note = name.startsWith('rapier3d ')
        ? ', with changes: third_party/rapier3d/FYNKEL.md in Fynkel\'s repository'
        : '';
    final of = '$name ($licence), $source$note';
    final files = File(p['manifest_path']! as String).parent.listSync().whereType<File>().where((file) {
      final base = file.uri.pathSegments.last.toUpperCase();
      return const ['LICEN', 'COPYING', 'NOTICE', 'COPYRIGHT', 'UNLICENSE'].any(base.startsWith);
    }).toList()..sort((a, b) => a.path.compareTo(b.path));
    if (files.isNotEmpty) {
      for (final file in files) {
        add(_read(file), of);
      }
    } else if (licence.contains('Apache-2.0')) {
      add(apache, of); // Named, its text not in the crate.
    } else if (licence == 'MIT') {
      final authors = (p['authors']! as List<Object?>).join(', ');
      // Without addresses: they are in the crate, for who needs them.
      add(_mit(authors.isEmpty ? 'the authors of ${p['name']}' : authors.replaceAll(RegExp(' <[^>]*>'), '')), of);
    } else {
      throw StateError('$name ($licence) has no licence file: find its text.');
    }
  }
  final rust = _run(root, ['rustc', '--version']).split(' ')[1];
  add(apache, 'the Rust standard library $rust (MIT OR Apache-2.0), https://github.com/rust-lang/rust');

  // Flutter reads the lines before the first empty one as the packages'
  // names, and 80 dashes as the end.
  final rule = '=' * 72;
  return [
        '$package\n',
        'The library in $package is built from $package\'s own source, under the MIT licence in LICENSE, and from the '
            'code of others below, each under the licence that follows it. Where code is offered under a choice of '
            'licences, any of them may be taken. Code under the Mozilla Public License 2.0 is unchanged, and its source is '
            'at the address beside it.',
        for (final MapEntry(key: text, value: of) in texts.entries) '$rule\n${of.join('\n')}\n$rule\n\n$text',
      ]
      .join('\n\n')
      .replaceAll('-' * 80, '-' * 72)
      .replaceAll('\r\n', '\n')
      .trim()
      .replaceFirst('$package\n\n\n', '$package\n\n');
}

String _read(File file) => utf8.decode(file.readAsBytesSync(), allowMalformed: true);

String _run(String root, List<String> command) {
  final result = Process.runSync(command.first, command.sublist(1), workingDirectory: root);
  if (result.exitCode != 0) throw StateError('${command.join(' ')}: ${result.stderr}');
  return result.stdout as String;
}

String _mit(String authors) =>
    '''
MIT License

Copyright (c) $authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
''';
