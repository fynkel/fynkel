import 'dart:io';

import 'package:path/path.dart' as p;
import 'package:yaml/yaml.dart';

import 'tools.dart';

/// `fynkel doctor`: what is missing on this machine, and in the game in this
/// folder, with what to do about it.
class Doctor {
  /// Looks with [tools], and writes to [out].
  Doctor(this.tools, this.out);

  /// What runs the programs it asks.
  final Tools tools;

  /// Where it writes.
  final StringSink out;

  var _wrong = 0;

  void _ok(String text) => out.writeln('[✓] $text');
  // Only one platform needs it, or it may be meant.
  void _note(String text) => out.writeln('[!] $text');
  void _bad(String text) {
    _wrong++;
    out.writeln('[✗] $text');
  }

  /// Looks, and answers with the exit code: 1 if something must be put right.
  Future<int> run() async {
    await _machine();
    await _game();
    out.writeln(
      _wrong == 0 ? '\nNothing in the way. `flutter doctor` checks the rest of Flutter.' : '\n$_wrong to put right.',
    );
    return _wrong == 0 ? 0 : 1;
  }

  Future<void> _machine() async {
    if (tools.isMac) {
      _ok('A Mac');
    } else {
      _bad('Not a Mac: for now Fynkel games are built on one only.');
    }

    final flutter = await tools.flutterVersion();
    if (flutter == null) {
      _bad('No Flutter on the PATH. Install it: https://docs.flutter.dev/get-started');
    } else if (atLeast(flutter.flutter, oldestFlutter)) {
      _ok('Flutter ${flutter.flutter}');
    } else {
      _bad('Flutter ${flutter.flutter}: Fynkel needs $oldestFlutter or later. Run `flutter upgrade`.');
    }

    final dart = await tools.read('dart', ['--version']);
    final version = RegExp(r'version: (\S+)').firstMatch(dart ?? '')?.group(1);
    if (flutter != null && version != null && version != flutter.dart) {
      _note(
        "`dart` is $version, not Flutter's ${flutter.dart}: another is first on the PATH. Use `flutter analyze` and "
        "`flutter test`, or put Flutter's bin folder first.",
      );
    }

    if (tools.isMac) {
      final xcode = await tools.read('xcodebuild', ['-version']);
      if (xcode == null) {
        _note('No Xcode: it builds for macOS and iOS. Install it from the App Store.');
      } else {
        _ok(xcode.split('\n').first);
      }
    }
    final sdk =
        tools.env['ANDROID_HOME'] ??
        tools.env['ANDROID_SDK_ROOT'] ??
        p.join(tools.env['HOME'] ?? '', 'Library', 'Android', 'sdk');
    if (Directory(p.join(sdk, 'platform-tools')).existsSync()) {
      _ok('Android SDK');
    } else {
      _note('No Android SDK in $sdk: it builds for Android. Install Android Studio.');
    }
  }

  Future<void> _game() async {
    final file = File(p.join(tools.here.path, 'pubspec.yaml'));
    if (!file.existsSync()) return;
    final Object? pubspec;
    try {
      pubspec = loadYaml(file.readAsStringSync());
    } on YamlException catch (e) {
      _bad('pubspec.yaml: ${e.message}');
      return;
    }
    final dependencies = pubspec is YamlMap ? pubspec['dependencies'] : null;
    if (dependencies is! YamlMap || !dependencies.containsKey('fynkel')) return;
    out.writeln('\nThe game in this folder, ${(pubspec as YamlMap)['name']}:');

    if (dependencies.containsKey('fynkel_flutter')) {
      _ok('fynkel and fynkel_flutter');
    } else {
      _bad('No fynkel_flutter in pubspec.yaml: run `flutter pub add fynkel_flutter`.');
    }
    // From a clone, the engine is built here; the release's is built already.
    if (dependencies['fynkel'] case YamlMap(:final nodes) when nodes.keys.any((k) => '$k' == 'path' || '$k' == 'git')) {
      final cargo = p.join(tools.env['HOME'] ?? '', '.cargo', 'bin', 'cargo');
      if (File(cargo).existsSync() || await tools.read('cargo', ['--version']) != null) {
        _ok('Rust, which a Fynkel from a clone is built with');
      } else {
        _bad('No Rust, which a Fynkel from a clone is built with. Install it: https://rustup.rs');
      }
    }

    _android();
    _apple('iOS', 'ios', 'IPHONEOS_DEPLOYMENT_TARGET', '15.0');
    _apple('macOS', 'macos', 'MACOSX_DEPLOYMENT_TARGET', '12.0');
  }

  void _android() {
    final app = p.join(tools.here.path, 'android', 'app');
    final gradle = ['build.gradle.kts', 'build.gradle'].map((n) => File(p.join(app, n))).where((f) => f.existsSync());
    if (gradle.isEmpty) return;
    final text = gradle.first.readAsStringSync();
    final min = RegExp(r'^\s*minSdk(?:Version)?\s*(?:=\s*)?(\S+)', multiLine: true).firstMatch(text)?.group(1);
    final number = int.tryParse(min ?? '');
    if (number != null && number >= 30) {
      _ok('Android: minSdk $number');
    } else if (number != null || min == null || min.startsWith('flutter.')) {
      _bad(
        'Android: minSdk is ${min ?? 'not set'}. Set `minSdk = 30` in ${p.relative(gradle.first.path, from: tools.here.path)}.',
      );
    } else {
      _note('Android: minSdk is $min. It must come to 30 or more.');
    }
    final main = Directory(p.join(app, 'src', 'main'));
    if (!main.existsSync()) return;
    final activities = main
        .listSync(recursive: true)
        .whereType<File>()
        .where((file) => const ['MainActivity.kt', 'MainActivity.java'].contains(p.basename(file.path)))
        .toList();
    if (activities.isEmpty) return;
    if (activities.any((file) => file.readAsStringSync().contains('FynkelActivity'))) {
      _ok('Android: a FynkelActivity');
    } else {
      _note(
        'Android: ${p.relative(activities.first.path, from: tools.here.path)} is not a FynkelActivity. The game runs, '
        'more slowly: see "A new game" in the guide.',
      );
    }
  }

  void _apple(String name, String folder, String setting, String oldest) {
    final project = File(p.join(tools.here.path, folder, 'Runner.xcodeproj', 'project.pbxproj'));
    if (!project.existsSync()) return;
    final targets = RegExp('$setting = ([0-9.]+);').allMatches(project.readAsStringSync()).map((m) => m.group(1)!);
    final older = targets.where((version) => !atLeast(version, oldest)).toSet();
    if (older.isEmpty) {
      _ok('$name: $oldest or later');
    } else {
      _bad('$name: the deployment target is ${older.join(', ')}. Set it to $oldest in $folder/Runner.xcworkspace.');
    }
  }
}
