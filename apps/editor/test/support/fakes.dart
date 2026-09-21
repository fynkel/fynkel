// Fakes for the shell tests: a project without the native runtime, a
// toolchain, a process runner, a directory chooser.
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/env/tools.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

final class FakeProject extends EditorProject {
  FakeProject({required this.dir, this.name = 'Fake Game', this.readOnly, List<Diagnostic> diagnostics = const []})
    : _diagnostics = List.of(diagnostics);

  @override
  final String dir;
  @override
  final String name;
  @override
  final String? readOnly;
  List<Diagnostic> _diagnostics;
  @override
  List<Diagnostic> get diagnostics => List.unmodifiable(_diagnostics);
  @override
  bool validating = false;
  @override
  DateTime? validatedAt;
  @override
  String? validationError;
  @override
  int documents = 3;
  @override
  int sources = 2;
  @override
  int references = 4;
  @override
  int assetCount = 5;
  @override
  MigrationPreview? migrations;
  int revalidations = 0;
  bool closed = false;

  set diagnostics(List<Diagnostic> d) {
    _diagnostics = List.of(d);
    notifyListeners();
  }

  @override
  Future<void> revalidate() async {
    revalidations++;
    validatedAt = DateTime(2026, 9, 21, 12, 30, 15);
    notifyListeners();
  }

  @override
  Future<void> close() async {
    closed = true;
  }
}

Diagnostic diag(
  String code, {
  Severity severity = Severity.error,
  String message = 'something is wrong',
  String? document,
  String? pointer,
  int? line,
  int? column,
  String? suggestion,
}) => Diagnostic(
  code: code,
  severity: severity,
  message: message,
  document: document,
  pointer: pointer,
  line: line,
  column: column,
  suggestion: suggestion,
);

Toolchain fakeToolchain({
  String? cli = '/fake/fynkel',
  String? root = '/fake/root',
  String? flutter = '/fake/flutter',
}) => Toolchain(
  fynkelCli: cli,
  fynkelRoot: root,
  flutter: flutter,
  flutterVersion: flutter == null ? null : '3.38.0',
  ideExecutable: '/usr/local/bin/zed',
  ideCommand: 'zed {file}:{line}:{column}',
  abiVersion: 1,
  abiError: null,
);

/// Records every command line; answers with the canned result.
final class RecordingRunner {
  RecordingRunner({this.exitCode = 0, this.stdout = '', this.stderr = ''});

  int exitCode;
  String stdout;
  String stderr;
  final calls = <(String, List<String>, String?)>[];

  Future<ProcessResult> call(String executable, List<String> args, {String? workingDirectory}) async {
    calls.add((executable, args, workingDirectory));
    return ProcessResult(1, exitCode, stdout, stderr);
  }
}

/// Creates a directory that passes `isFynkelProject` (a project file only).
Directory fakeProjectDir(String name) {
  final dir = Directory.systemTemp.createTempSync('fynkel_editor_$name');
  File('${dir.path}/fynkel.project.json').writeAsStringSync('{"name":"$name"}');
  return dir;
}

/// The shell with everything faked. [projects] maps a directory to the
/// project [openProject] returns for it.
Widget shellApp({
  Key? key,
  required Preferences prefs,
  LayoutStore? store,
  Map<String, EditorProject> projects = const {},
  String? chosenDirectory,
  Toolchain? toolchain,
  RecordingRunner? runner,
  VoidCallback? quit,
  String? initialProject,
}) {
  return FynkelEditorApp(
    key: key,
    prefs: prefs,
    layoutStore: store ?? MemoryLayoutStore(),
    openProject: (dir) async {
      final p = projects[dir];
      if (p == null) {
        throw AuthoringException('openProject', AuthoringStatus.loadFailed, [
          Diagnostic(code: 'FYN-A0001', severity: Severity.error, message: 'no fake project at $dir'),
        ]);
      }
      return p;
    },
    chooseDirectory: (_, {title, initialDirectory}) async => chosenDirectory,
    detectToolchain: (_) async => toolchain ?? fakeToolchain(),
    run: runner?.call ?? RecordingRunner().call,
    nativeMenuBar: false,
    quit: quit ?? () {},
    initialProject: initialProject,
  );
}

/// A large window, so menus and three columns of panels fit.
Future<void> pumpLarge(WidgetTester tester, Widget app) async {
  tester.view.physicalSize = const Size(1600, 1000);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(app);
  await tester.pumpAndSettle();
}
