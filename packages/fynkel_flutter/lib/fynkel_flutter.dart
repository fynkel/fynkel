/// Fynkel's Flutter host: the viewport widget and the game loop.
///
/// `package:fynkel` stays widget-free (blueprint §2); everything that needs
/// Flutter — the Path A texture (ADR-0002), the Ticker-driven frame, the
/// input plumbing, the HUD bindings — lives here. Apps compose
/// [FynkelGameView] (a [FynkelViewport] plus the loop) or drive a
/// [FynkelViewportController] from their own loop, bind their HUD to
/// `GameModel` fields ([ModelBinding], [ModelBuilder]) and pin labels to
/// the scene ([ProjectionLayer], [WorldLabel]), and open the cooked cache
/// they bundle ([unpackBundledCache]) when the project directory is not on
/// this machine.
library;

export 'src/bundled_cache.dart';
export 'src/game_view.dart';
export 'src/input_capture.dart';
export 'src/model_binding.dart';
export 'src/projection_layer.dart';
export 'src/touch_overlay.dart';
export 'src/viewport.dart';
export 'src/viewport_controller.dart';
