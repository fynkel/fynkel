import 'dart:io';

import 'package:flutter/material.dart';
import 'package:path/path.dart' as p;

import 'src/app.dart';
import 'src/prefs/preferences.dart';
import 'src/shell/workspace.dart';

/// `fynkel_editor [project-dir]` — the directory opens at start.
void main(List<String> args) {
  final prefs = Preferences.load(Preferences.defaultFile());
  final layout = FileLayoutStore(File(p.join(Preferences.defaultDirectory().path, 'layout.json')));
  final initial = args.where((a) => !a.startsWith('-')).firstOrNull;
  runApp(
    FynkelEditorApp(
      prefs: prefs,
      layoutStore: layout,
      initialProject: initial == null ? null : Directory(initial).absolute.path,
    ),
  );
}
