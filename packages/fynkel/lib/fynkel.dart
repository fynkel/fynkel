/// Fynkel: a code-first game framework. Build a [World] of objects with
/// components; package:fynkel_flutter shows it on screen.
library;

export 'src/color.dart';
export 'src/error.dart';
export 'src/math.dart';
export 'src/view.dart' show HeadlessView, ViewStats, WorldView;
export 'src/world.dart' hide SendTransforms, WorldSystem, WorldSystems;
