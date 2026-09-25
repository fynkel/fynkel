/// The editor as a library (ADR-0021): `apps/editor`'s own `main` runs it
/// without a game; the project editor `fynkel editor <dir>` generates runs
/// it with the project's game compiled in ([EditorGame]).
library;

import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:path/path.dart' as p;

import 'src/app.dart';
import 'src/extensions/editor_extensions.dart';
import 'src/play/editor_game.dart';
import 'src/play/updating_code.dart';
import 'src/prefs/preferences.dart';
import 'src/shell/workspace.dart';

export 'src/extensions/editor_extensions.dart' show EditorPart;
export 'src/play/editor_game.dart';

/// `fynkel_editor [project-dir]` — the directory opens at start; [game] is
/// the project's game when this is its project editor, and [extensions]
/// the editor parts of the game and its packages (ADR-0028), in the order
/// they register.
void runFynkelEditor(List<String> args, {EditorGame? game, List<EditorPart> extensions = const []}) {
  final prefs = Preferences.load(Preferences.defaultFile());
  final layout = FileLayoutStore(File(p.join(Preferences.defaultDirectory().path, 'layout.json')));
  final initial = args.where((a) => !a.startsWith('-')).firstOrNull ?? game?.projectDir;
  runApp(
    UpdatingCodeBarrier(
      path: Platform.environment['FYNKEL_EDITOR_UPDATING'],
      child: FynkelEditorApp(
        prefs: prefs,
        layoutStore: layout,
        initialProject: initial == null ? null : Directory(initial).absolute.path,
        game: game,
        extensions: extensions,
      ),
    ),
  );
}
