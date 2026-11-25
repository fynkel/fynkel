// The fynkel command, with a Flutter that isn't there: what it writes, and
// what it says when something is missing. test/real_test.dart makes a real
// game.

import 'dart:io';

import 'package:fynkel_cli/fynkel_cli.dart';
import 'package:fynkel_cli/src/tools.dart' show atLeast, oldestFlutter;
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

class FakeTools extends Tools {
  FakeTools(this.here);
  @override
  final Directory here;
  @override
  late Map<String, String> env = {'HOME': here.path};
  @override
  bool isMac = true;
  String? flutter = '3.47.2';
  final programs = <String, String>{'xcodebuild': 'Xcode 26.1\nBuild 1', 'dart': 'Dart SDK version: 3.13.2 (stable)'};
  final ran = <String>[];
  String gradle = 'android {\n    defaultConfig {\n        minSdk = flutter.minSdkVersion\n    }\n}\n';

  @override
  Future<({String dart, String flutter})?> flutterVersion() async =>
      flutter == null ? null : (flutter: flutter!, dart: '3.13.2');
  @override
  Future<String?> read(String program, List<String> args) async => programs[program];
  @override
  Future<void> run(String program, List<String> args, {Directory? folder}) async {
    ran.add('$program ${args.join(' ')}');
    if (args.first != 'create') return;
    final dir = args.last;
    void write(String path, String text) => File(p.join(dir, path))
      ..createSync(recursive: true)
      ..writeAsStringSync(text);
    write(
      'pubspec.yaml',
      'name: ${p.basename(dir)}\ndependencies:\n  flutter:\n    sdk: flutter\n\n'
          'dev_dependencies:\n  flutter_test:\n    sdk: flutter\n',
    );
    write('lib/main.dart', '');
    write('android/app/build.gradle.kts', gradle);
    write(
      'android/app/src/main/kotlin/com/example/my_game/MainActivity.kt',
      'package com.example.my_game\n\nimport io.flutter.embedding.android.FlutterActivity\n\n'
          'class MainActivity : FlutterActivity()\n',
    );
  }
}

void main() {
  late FakeTools tools;
  late StringBuffer out;
  late StringBuffer err;
  setUp(() {
    final dir = Directory.systemTemp.createTempSync('fynkel_cli');
    addTearDown(() => dir.deleteSync(recursive: true));
    tools = FakeTools(dir);
    out = StringBuffer();
    err = StringBuffer();
  });
  Future<int> fynkel(List<String> args) => FynkelCli(tools: tools, out: out, err: err).run(args);
  File file(String path) => File(p.join(tools.here.path, path));
  void write(String path, String text) => file(path)
    ..createSync(recursive: true)
    ..writeAsStringSync(text);

  test('its version is the package\'s', () async {
    expect(await fynkel(['--version']), 0);
    expect(File('pubspec.yaml').readAsStringSync(), contains('\nversion: $fynkelVersion\n'));
    expect(out.toString(), 'fynkel $fynkelVersion\n');
  });

  test("it asks for the Flutter and the systems fynkel_flutter's own files do", () {
    String other(String path) => File('../fynkel_flutter/$path').readAsStringSync();
    expect(other('pubspec.yaml'), contains('flutter: ">=$oldestFlutter"'));
    expect(other('android/build.gradle.kts'), contains('minSdk = 30'));
    expect(other('darwin/fynkel_flutter/Package.swift'), allOf(contains('.macOS("12.0")'), contains('.iOS("15.0")')));
  });

  test('versions compare by number', () {
    expect(atLeast('3.47.0', '3.47.0'), isTrue);
    expect(atLeast('3.50.0-1.0.pre', '3.47.0'), isTrue);
    expect(atLeast('3.9.9', '3.47.0'), isFalse);
    expect(atLeast('15', '15.0'), isTrue);
  });

  test('what it does not know it says', () async {
    expect(await fynkel(['make']), 64);
    expect(err.toString(), contains('there is no command "make"'));
    expect(await fynkel(['create']), 64);
    expect(await fynkel(['create', 'a', '--nope']), 64);
    expect(await fynkel([]), 64);
    expect(await fynkel(['--help']), 0);
    expect(await fynkel(['create', '--help']), 0);
    expect(tools.ran, isEmpty);
  });

  group('create', () {
    test('makes a game on the release', () async {
      expect(await fynkel(['create', 'my_game', '--physics', '--org', 'com.me']), 0, reason: '$err');
      expect(
        tools.ran.first,
        'flutter create --empty --no-pub --platforms macos,ios,android --org com.me ${file('my_game').path}',
      );
      expect(tools.ran.last, 'flutter pub get');
      expect(
        file('my_game/pubspec.yaml').readAsStringSync(),
        allOf(
          contains(
            '    sdk: flutter\n  fynkel: ^$fynkelVersion\n  fynkel_flutter: ^$fynkelVersion\n  fynkel_physics: ^$fynkelVersion\n\ndev',
          ),
          contains('uses-material-design: true'),
        ),
      );
      expect(file('my_game/lib/main.dart').readAsStringSync(), contains('FynkelApp(Game.new)'));
      expect(file('my_game/test/game_test.dart').readAsStringSync(), contains("'package:my_game/main.dart'"));
      expect(file('my_game/android/app/build.gradle.kts').readAsStringSync(), contains('minSdk = 30'));
      expect(
        file('my_game/android/app/src/main/kotlin/com/example/my_game/MainActivity.kt').readAsStringSync(),
        allOf(contains('import dev.fynkel.fynkel_flutter.FynkelActivity'), contains(': FynkelActivity()')),
      );
      expect(out.toString(), contains('cd my_game\n  flutter run -d macos'));
    });

    test('makes one on a clone', () async {
      write('clone/packages/fynkel/pubspec.yaml', '');
      expect(await fynkel(['create', 'g', '--path', 'clone', '--audio']), 0, reason: '$err');
      expect(
        file('g/pubspec.yaml').readAsStringSync(),
        contains('  fynkel_audio:\n    path: ${file('clone/packages/fynkel_audio').path}\n'),
      );
      expect(await fynkel(['create', 'h', '--path', 'elsewhere']), 1);
      expect(err.toString(), contains('not a clone'));
    });

    test('leaves what is there alone', () async {
      write('my_game/mine.txt', 'mine');
      expect(await fynkel(['create', 'my_game']), 1);
      expect(err.toString(), contains('is there already'));
      expect(tools.ran, isEmpty);
    });

    test('says what is in the way', () async {
      tools.flutter = '3.40.0';
      expect(await fynkel(['create', 'a']), 1);
      expect(err.toString(), contains('flutter upgrade'));
      tools.flutter = null;
      expect(await fynkel(['create', 'a']), 1);
      expect(err.toString(), contains('was not found'));
      expect(await fynkel(['create', 'a', '--platforms', 'web,macos']), 1);
      expect(err.toString(), contains('not on web'));
      expect(tools.ran, isEmpty);
    });

    test("says when flutter create's files are not as it knows them", () async {
      tools.gradle = 'minSdk = 99\n';
      expect(await fynkel(['create', 'a']), 1);
      expect(err.toString(), allOf(contains('build.gradle.kts'), contains('half set up')));
    });
  });

  group('doctor', () {
    test('finds nothing wrong', () async {
      Directory(p.join(tools.here.path, 'Library/Android/sdk/platform-tools')).createSync(recursive: true);
      expect(await fynkel(['doctor']), 0);
      expect(
        out.toString(),
        allOf(
          contains('[✓] Flutter 3.47.2'),
          contains('[✓] Xcode 26.1'),
          contains('[✓] Android SDK'),
          isNot(contains('[!]')),
        ),
      );
    });

    test('finds what the machine lacks', () async {
      tools
        ..isMac = false
        ..flutter = '3.9.0'
        ..programs['dart'] = 'Dart SDK version: 3.11.0 (stable)';
      expect(await fynkel(['doctor']), 1);
      expect(
        out.toString(),
        allOf(
          contains('[✗] Not a Mac'),
          contains('[✗] Flutter 3.9.0'),
          contains('[!] `dart` is 3.11.0'),
          contains('[!] No Android SDK'),
        ),
      );
      expect(out.toString(), contains('2 to put right'));
    });

    test('a game it made is right', () async {
      await fynkel(['create', 'my_game']);
      tools = FakeTools(Directory(p.join(tools.here.path, 'my_game')));
      expect(await fynkel(['doctor']), 0);
      expect(
        out.toString(),
        allOf(contains('my_game:'), contains('minSdk 30'), contains('[✓] Android: a FynkelActivity')),
      );
    });

    test('finds what a game lacks', () async {
      write('pubspec.yaml', 'name: g\ndependencies:\n  fynkel:\n    path: ../fynkel\n');
      write('android/app/build.gradle.kts', tools.gradle);
      write('android/app/src/main/kotlin/MainActivity.kt', 'class MainActivity : FlutterActivity()');
      write('ios/Runner.xcodeproj/project.pbxproj', 'IPHONEOS_DEPLOYMENT_TARGET = 13.0;');
      write('macos/Runner.xcodeproj/project.pbxproj', 'MACOSX_DEPLOYMENT_TARGET = 12.0;');
      expect(await fynkel(['doctor']), 1);
      expect(
        out.toString(),
        allOf([
          contains('[✗] No fynkel_flutter'),
          contains('[✗] No Rust'),
          contains('[✗] Android: minSdk is flutter.minSdkVersion'),
          contains('[!] Android: android/app/src/main/kotlin/MainActivity.kt is not a FynkelActivity'),
          contains('[✗] iOS: the deployment target is 13.0'),
          contains('[✓] macOS: 12.0 or later'),
        ]),
      );
    });

    test('a folder that is no game is left out', () async {
      write('pubspec.yaml', 'name: g\ndependencies:\n  http: any\n');
      expect(await fynkel(['doctor']), 0);
      expect(out.toString(), isNot(contains('The game in this folder')));
    });
  });
}
