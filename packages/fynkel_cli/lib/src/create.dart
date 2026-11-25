import 'dart:io';

import 'package:path/path.dart' as p;

import 'tools.dart';

/// `fynkel create`: a Flutter app with Fynkel in it, set up as the guide's
/// §1 says, and a first game.
class Create {
  /// Makes the game in [folder].
  Create(
    this.tools,
    this.out, {
    required this.folder,
    required this.org,
    required this.platforms,
    required this.physics,
    required this.audio,
    required this.repository,
    required this.version,
  });

  /// What runs flutter.
  final Tools tools;

  /// Where it says what it does.
  final StringSink out;

  /// The folder to make, as it was typed.
  final String folder;

  /// The app's organisation.
  final String org;

  /// The platforms to make it for.
  final List<String> platforms;

  /// Whether it has physics.
  final bool physics;

  /// Whether it has sound.
  final bool audio;

  /// A clone of Fynkel to use, instead of the release.
  final String? repository;

  /// The release to depend on.
  final String version;

  /// Makes it.
  Future<void> run() async {
    final unknown = platforms.where((name) => !const ['macos', 'ios', 'android'].contains(name)).toList();
    if (unknown.isNotEmpty) {
      throw CliException('Fynkel runs on macos, ios and android, not on ${unknown.join(', ')}.');
    }
    final dir = Directory(p.normalize(p.join(tools.here.path, folder)));
    if (FileSystemEntity.isFileSync(dir.path) || (dir.existsSync() && dir.listSync().isNotEmpty)) {
      throw CliException('${dir.path} is there already.');
    }
    String? packages;
    if (repository case final repository?) {
      packages = p.join(p.normalize(p.join(tools.here.path, repository)), 'packages');
      if (!File(p.join(packages, 'fynkel', 'pubspec.yaml')).existsSync()) {
        throw CliException('$repository is not a clone of the Fynkel repository.');
      }
    }
    final flutter = await tools.flutterVersion();
    if (flutter == null) {
      throw CliException('`flutter` was not found. Install it: https://docs.flutter.dev/get-started');
    }
    if (!atLeast(flutter.flutter, oldestFlutter)) {
      throw CliException(
        'Fynkel needs Flutter $oldestFlutter or later, and this is ${flutter.flutter}: run `flutter upgrade`.',
      );
    }

    out.writeln('Making ${dir.path}');
    await tools.run('flutter', [
      'create',
      '--empty',
      '--no-pub',
      '--platforms',
      platforms.join(','),
      '--org',
      org,
      dir.path,
    ]);

    final pubspec = File(p.join(dir.path, 'pubspec.yaml'));
    var text = pubspec.readAsStringSync();
    final name = RegExp(r'^name: (\S+)', multiLine: true).firstMatch(text)!.group(1)!;
    text = _replace(
      text,
      'dependencies:\n  flutter:\n    sdk: flutter\n',
      (found) =>
          '$found${[
            for (final package in ['fynkel', 'fynkel_flutter', if (physics) 'fynkel_physics', if (audio) 'fynkel_audio']) packages == null ? '  $package: ^$version\n' : '  $package:\n    path: ${p.join(packages, package)}\n',
          ].join()}',
      pubspec,
    );
    // Some Flutters' --empty leaves it out, and every Icon is then a box.
    if (!text.contains('uses-material-design')) text = '$text\nflutter:\n  uses-material-design: true\n';
    pubspec.writeAsStringSync(text);

    File(p.join(dir.path, 'lib', 'main.dart')).writeAsStringSync(_main);
    File(p.join(dir.path, 'test', 'game_test.dart'))
      ..createSync(recursive: true)
      ..writeAsStringSync(_test(name));

    if (platforms.contains('android')) {
      final gradle = File(p.join(dir.path, 'android', 'app', 'build.gradle.kts'));
      gradle.writeAsStringSync(
        _replace(
          gradle.readAsStringSync(),
          'minSdk = flutter.minSdkVersion',
          (_) => 'minSdk = 30 // Fynkel needs Android 11.',
          gradle,
        ),
      );
      final activity = Directory(p.join(dir.path, 'android', 'app', 'src', 'main'))
          .listSync(recursive: true)
          .whereType<File>()
          .firstWhere((file) => p.basename(file.path) == 'MainActivity.kt');
      // The game then draws straight to the screen (the guide's §1).
      activity.writeAsStringSync(
        _replace(
          activity.readAsStringSync(),
          'import io.flutter.embedding.android.FlutterActivity\n\nclass MainActivity : FlutterActivity()',
          (_) => 'import dev.fynkel.fynkel_flutter.FynkelActivity\n\nclass MainActivity : FynkelActivity()',
          activity,
        ),
      );
    }

    await tools.run('flutter', ['pub', 'get'], folder: dir);
    final run = platforms.contains('macos') ? 'flutter run -d macos' : 'flutter run';
    out.writeln('''

Your game is ready:

  cd ${folder.contains(' ') ? '"$folder"' : folder}
  $run

It is in lib/main.dart, and the guide is at https://fynkel.com''');
  }

  // flutter create's text, which the next Flutter may write otherwise.
  String _replace(String text, String from, String Function(String found) to, File file) {
    if (!text.contains(from)) {
      throw CliException(
        '${file.path} is not as `flutter create` wrote it when Fynkel $version was made, so the game is half set up. '
        'Delete it and update this command (`dart pub global activate fynkel_cli`), or set it up by hand: '
        'https://fynkel.com',
      );
    }
    return text.replaceFirst(from, to(from));
  }
}

const _main = '''
import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() => runApp(FynkelApp(Game.new));

/// The game: a camera, a sun and a turning cube.
class Game extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(transform: Transform.lookAt(Vec3(0, 1.5, 4), Vec3.zero), components: [Camera()]));
    add(GameObject(components: [Light.sun(direction: Vec3(-0.4, -1, -0.3))]));
    add(Cube());
  }
}

class Cube extends GameObject {
  Cube() : super(components: [MeshRenderer(Mesh.cube(), Material(color: Color(0xFFE0613A)))]);

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, time.delta); // A radian a second.
  }
}
''';

String _test(String name) =>
    '''
// The game, run without a window.

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:$name/main.dart';

void main() {
  test('the cube turns', () {
    final world = World();
    addTearDown(world.dispose);
    world.add(Game());
    final cube = world.first<Cube>()!;
    final before = cube.rotation;
    world.run(1);
    expect(cube.rotation, isNot(before));
  });
}
''';
