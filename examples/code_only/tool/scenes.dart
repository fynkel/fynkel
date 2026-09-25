// Writes the game's scenes, its coin prefab and its materials from the
// specs in lib/game/level.dart (`package:fynkel/authoring.dart`, ADR-0026
// §2): `dart run tool/scenes.dart` from the project. The documents are
// what BuildMode.scenes loads; nothing in assets/ is written by hand.
//
// Run it again after changing a spec: the ids stay the same, a document
// is rewritten only when its bytes change, and objects added to a scene
// in the editor are kept. Then `fynkel cook` (and `fynkel codegen`, for
// a new document's `Assets` constant).

import 'dart:io';

import 'package:code_only/game/level.dart';
import 'package:code_only/generated/behaviours.g.dart';
import 'package:fynkel/authoring.dart';
import 'package:fynkel/fynkel.dart';

void main(List<String> args) {
  for (final d in writeScenes(args.isEmpty ? Directory.current.path : args.first)) {
    stdout.writeln('${d.changed ? 'wrote' : 'same '} assets/${d.path}${d.kept.isEmpty ? '' : ' (kept ${d.kept.join(', ')})'}');
  }
}

/// Writes every document into the project at [projectDir]; what was
/// written, in order.
List<WrittenDocument> writeScenes(String projectDir) {
  final writer = ProjectWriter(projectDir, registerBehaviours: registerBehaviours);
  final written = <WrittenDocument>[];
  WrittenDocument add(WrittenDocument d) {
    written.add(d);
    return d;
  }

  final look = Look((name, material) => add(writer.writeMaterial('materials/$name', material)).ref);
  final coinPrefab = add(writer.writePrefab('prefabs/coin', coin(look))).ref;
  add(writer.writeScene('scenes/menu', SceneSpec('Menu', menu(look))));
  add(
    writer.writeScene(
      'scenes/level',
      SceneSpec(
        'Level',
        level(look, coinAt: (name, at) => ObjectSpec.prefab(coinPrefab, name: name, transform: Transform(position: at))),
      ),
    ),
  );
  add(writer.writeScene('scenes/results', SceneSpec('Results', results(look))));
  return written;
}
