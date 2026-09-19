/// Fynkel's Flutter host: the viewport widget and the game loop.
///
/// `package:fynkel` stays widget-free (blueprint §2); everything that needs
/// Flutter — the Path A texture (ADR-0002), the Ticker-driven frame, the
/// input plumbing — lives here. Apps compose [FynkelGameView] (a
/// [FynkelViewport] plus the loop) or drive a [FynkelViewportController]
/// from their own loop.
library;

export 'src/game_view.dart';
export 'src/viewport.dart';
export 'src/viewport_controller.dart';
