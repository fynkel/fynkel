/// Scenes and prefabs written from code (ADR-0026 §2): the same
/// [ObjectSpec]s `World.build` spawns, saved as the `.scene.json` and
/// `.prefab.json` documents the editor opens and the cook reads, with
/// derived ids so writing again gives the same bytes. Materials and data
/// assets (`writeData`, ADR-0027) too. Pure Dart, for a
/// tool (`dart run tool/scenes.dart`); a game never imports it.
///
/// ```dart
/// import 'package:fynkel/authoring.dart';
///
/// void main() {
///   final w = ProjectWriter('.', registerBehaviours: registerBehaviours);
///   w.writePrefab('prefabs/coin', coin());
///   w.writeScene('scenes/level', SceneSpec('Level', [floor(), player(), for (var i = 0; i < 5; i++) coinAt(i)]));
/// }
/// ```
library;

export 'gameplay.dart' show ObjectSpec, Primitive;
export 'src/authoring/writer.dart' show ProjectWriter, SceneSpec, WrittenDocument;
