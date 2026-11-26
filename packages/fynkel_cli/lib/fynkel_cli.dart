/// The `fynkel` command: `fynkel create` and `fynkel doctor`.
library;

import 'dart:io';

import 'package:args/args.dart';

import 'src/create.dart';
import 'src/doctor.dart';
import 'src/tools.dart';

export 'src/tools.dart' show Tools;

/// The release of Fynkel this command is of, and makes games for.
const fynkelVersion = '0.1.1';

/// The command.
class FynkelCli {
  /// Runs other programs with [tools], and writes to [out] and [err].
  FynkelCli({Tools? tools, StringSink? out, StringSink? err})
    : tools = tools ?? Tools(),
      out = out ?? stdout,
      err = err ?? stderr;

  /// What runs `flutter` and the rest.
  final Tools tools;

  /// Where it writes what it did.
  final StringSink out;

  /// Where it writes what went wrong.
  final StringSink err;

  static final _create = ArgParser()
    ..addOption('org', help: "Your reversed domain, for the app's id.", defaultsTo: 'com.example')
    ..addOption('platforms', help: 'Which of macos, ios and android.', defaultsTo: 'macos,ios,android')
    ..addFlag('physics', help: 'Add fynkel_physics.', negatable: false)
    ..addFlag('audio', help: 'Add fynkel_audio.', negatable: false)
    ..addOption('path', help: 'Use a clone of the Fynkel repository instead of the release.', valueHelp: 'folder')
    ..addFlag('help', abbr: 'h', negatable: false, hide: true);

  static final _parser = ArgParser()
    ..addFlag('help', abbr: 'h', negatable: false, hide: true)
    ..addFlag('version', negatable: false, help: 'Print the version.')
    ..addCommand('create', _create)
    ..addCommand('doctor', ArgParser()..addFlag('help', abbr: 'h', negatable: false, hide: true));

  static final _usage =
      '''
Fynkel: code-first 3D games in Flutter. https://fynkel.com

  fynkel create <folder>   Make a new game.
${_create.usage.split('\n').map((line) => '      $line').join('\n')}
  fynkel doctor            Check this machine, and the game in this folder.
  fynkel --version''';

  /// Runs [args], and answers with the exit code.
  Future<int> run(List<String> args) async {
    try {
      final parsed = _parser.parse(args);
      final command = parsed.command;
      if (parsed.flag('version')) {
        out.writeln('fynkel $fynkelVersion');
        return 0;
      }
      if (command == null) {
        if (parsed.rest.isNotEmpty) throw FormatException('there is no command "${parsed.rest.first}"');
        out.writeln(_usage);
        return parsed.flag('help') ? 0 : 64;
      }
      if (command.flag('help')) {
        out.writeln(_usage);
        return 0;
      }
      if (command.name == 'doctor') {
        if (command.rest.isNotEmpty) throw FormatException('doctor takes nothing: "${command.rest.join(' ')}"');
        return await Doctor(tools, out).run();
      }
      if (command.rest.length != 1) throw const FormatException('say the folder to make, as in: fynkel create my_game');
      await Create(
        tools,
        out,
        folder: command.rest.single,
        org: command.option('org')!,
        platforms: command.option('platforms')!.split(',').map((name) => name.trim()).toList(),
        physics: command.flag('physics'),
        audio: command.flag('audio'),
        repository: command.option('path'),
        version: fynkelVersion,
      ).run();
      return 0;
    } on FormatException catch (e) {
      err.writeln('fynkel: ${e.message}\n\n$_usage');
      return 64;
    } on CliException catch (e) {
      err.writeln('fynkel: ${e.message}');
      return 1;
    }
  }
}
