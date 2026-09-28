// Writes the game's scenes, its coin prefabs, its materials and its kinds of
// coin (data assets, ADR-0027) from the
// specs in lib/game/level.dart (`package:fynkel/authoring.dart`, ADR-0026
// §2): `dart run tool/scenes.dart` from the project. The documents are
// what BuildMode.scenes loads; nothing in assets/ is written by hand.
//
// Run it again after changing a spec: the ids stay the same, a document
// is rewritten only when its bytes change, and objects added to a scene
// in the editor are kept. Then `fynkel cook` (and `fynkel codegen`, for
// a new document's `Assets` constant).

import 'dart:io';

import 'package:code_only/game/coin_kind.dart';
import 'package:code_only/game/level.dart';
import 'package:code_only/generated/packages.g.dart';
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
  final writer = ProjectWriter(projectDir, registerBehaviours: registerPackages);
  final written = <WrittenDocument>[];
  WrittenDocument add(WrittenDocument d) {
    written.add(d);
    return d;
  }

  final look = Look((name, material) => add(writer.writeMaterial('materials/$name', material)).ref);
  // Written first: the instances then know their assets, so the prefab
  // and the level name them.
  final kinds = CoinKinds();
  add(writer.writeData('data/coin', kinds.coin, name: 'Coin'));
  add(writer.writeData('data/big_coin', kinds.big, name: 'Big Coin'));
  // A prefab per kind: the coin and the big coin.
  final prefabs = {
    kinds.coin: add(writer.writePrefab('prefabs/coin', coin(look, kinds.coin, name: 'Coin'))).ref,
    kinds.big: add(writer.writePrefab('prefabs/big_coin', coin(look, kinds.big, name: 'Big Coin'))).ref,
  };
  add(writer.writeScene('scenes/menu', SceneSpec('Menu', menu(look))));
  add(
    writer.writeScene(
      'scenes/level',
      SceneSpec(
        'Level',
        level(
          look,
          kinds,
          coinAt: (name, at, kind) => ObjectSpec.prefab(prefabs[kind]!, name: name, transform: Pose(position: at)),
        ),
      ),
    ),
  );
  add(writer.writeScene('scenes/results', SceneSpec('Results', results(look))));
  return written;
}
