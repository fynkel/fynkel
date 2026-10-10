/// Shows a Fynkel game in a Flutter app: [FynkelApp] for a game that is
/// the whole app (with a HUD over it, if any), [FynkelView] for one inside
/// a Flutter app, and [FynkelGame] to make its root and start it over.
library;

export 'src/fynkel_app.dart' show FynkelApp;
export 'src/fynkel_game.dart' show FynkelGame, FynkelGameState;
export 'src/fynkel_view.dart' show FynkelView, FynkelViewState, FynkelViewStats, bundleSource;
