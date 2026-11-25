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

/// The game: a sky, a camera, a sun, a stage and a cube that hops when it's tapped.
class Game extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    world.sky = Sky.gradient(zenith: Color(0xFF5B8DEF), horizon: Color(0xFFFFE3C2), ground: Color(0xFF8A7CA8));
    add(GameObject(transform: Transform.lookAt(Vec3(0, 2.8, 5.5), Vec3(0, 0.7, 0)), components: [Camera()]));
    add(GameObject(components: [Light.sun(direction: Vec3(0.5, -1, -0.4), intensity: 1.2)]));
    // The stage: its top is at height 0.
    add(
      GameObject(
        transform: Transform(position: Vec3(0, -0.15, 0), scale: Vec3(2.7, 0.15, 2.7)),
        components: [MeshRenderer(Mesh.cylinder(segments: 64), Material(color: Color(0xFFF4EDE4)))],
      ),
    );
    add(Cube());
  }
}

class Cube extends GameObject {
  Cube()
    : super(
        transform: Transform(position: Vec3(0, 0.5, 0)),
        components: [MeshRenderer(Mesh.cube(), Material(color: colors.first))],
      );

  static const colors = [Color(0xFFE0613A), Color(0xFF3A8DE0), Color(0xFFE0B83A), Color(0xFF9B59D0)];

  int hops = 0;
  Motion? _hop;

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, time.delta); // A radian a second.
  }

  /// A tap or a click on the cube.
  @override
  bool onClick(Finger finger) {
    if (_hop?.isRunning ?? false) return true; // Still in the air.
    hops++;
    _hop = moveBy(Vec3(0, 1.2, 0), seconds: 0.3, yoyo: true, loops: 2);
    punchScale(Vec3.one * 0.15, seconds: 0.6);
    colorTo(colors[hops % colors.length], seconds: 0.3);
    return true;
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
  test('the cube turns, and hops when it is tapped', () {
    final world = World();
    addTearDown(world.dispose);
    world.add(Game());
    world.input.simulate.resize(400, 400);
    final cube = world.first<Cube>()!;
    final before = cube.rotation;
    world.run(1);
    expect(cube.rotation, isNot(before));

    world.input.simulate.fingerDown(1, 200, 200);
    world.run(0.1);
    world.input.simulate.fingerUp(1, 200, 200);
    world.run(0.3);
    expect(cube.hops, 1);
    expect(cube.position.y, greaterThan(1));
    world.run(1);
    expect(cube.position.y, closeTo(0.5, 0.01));
  });
}
''';
