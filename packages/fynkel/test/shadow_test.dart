// Playtest game 1: `late final Animator animator;` on a player compiled,
// and hid GameObject's own `animator` from every caller, with no more than
// a lint's info, off without lints. GameObject's shortcuts are
// @nonVirtual, so a field of the same name is the analyzer's warning.

import 'dart:io';

import 'package:test/test.dart';

const shortcuts = {
  'world': 'World',
  'loaded': 'Future<void>',
  'assets': 'Assets',
  'input': 'Input',
  'debug': 'Debug',
  'time': 'Time',
  'parent': 'GameObject?',
  'children': 'List<GameObject>',
  'name': 'String?',
  'worldTransform': 'Transform',
  'animator': 'Animator?',
  'transform': 'Transform',
};

void main() {
  test("a game's field named as one of GameObject's shortcuts is a warning", () async {
    // Inside the package, so that package:fynkel resolves.
    final dir = Directory('.dart_tool/shadow_test')..createSync(recursive: true);
    addTearDown(() => dir.deleteSync(recursive: true));
    final file = File('${dir.path}/shadow.dart')
      ..writeAsStringSync(
        [
          "import 'package:fynkel/fynkel.dart';",
          'class Player extends GameObject {',
          for (final MapEntry(key: name, value: type) in shortcuts.entries) '  late final $type $name;',
          '}',
        ].join('\n'),
      );
    final result = await Process.run('dart', ['analyze', '--format=machine', file.path]);
    final warned = {
      for (final line in '${result.stdout}${result.stderr}'.split('\n'))
        if (line.contains('INVALID_OVERRIDE_OF_NON_VIRTUAL_MEMBER')) RegExp("'(\\w+)'").firstMatch(line)!.group(1),
    };
    expect(warned, shortcuts.keys.toSet(), reason: '${result.stdout}');
  });
}
