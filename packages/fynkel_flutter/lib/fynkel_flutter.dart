/// Shows a Fynkel game in a Flutter app: [FynkelApp] for a game that is
/// the whole app (with a HUD over it, if any), [FynkelView] for one inside
/// a Flutter app, and [FynkelGame] to make its root and start it over.
/// [WorldWidget] puts a widget at a place in the world. [ListenableSignal]
/// lets game code listen to Flutter notifiers, and [WorldListenables]
/// lets widgets listen to the world. [WorldTheme] gives the game the app's
/// theme, and [FlutterColorToFynkel] and [FynkelColorToFlutter] turn one's
/// colours into the other's.
library;

export 'src/colors.dart' show FlutterColorToFynkel, FynkelColorToFlutter;
export 'src/fynkel_app.dart' show FynkelApp;
export 'src/fynkel_game.dart' show FynkelGame, FynkelGameState;
export 'src/fynkel_view.dart' show FynkelView, FynkelViewState, FynkelViewStats, bundleSource;
export 'src/listenable_signal.dart' show ListenableSignal;
export 'src/world_listenables.dart' show WorldListenables, WorldTheme;
export 'src/world_widget.dart' show WorldWidget;
