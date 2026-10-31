/// Shows a Fynkel game in a Flutter app. Start with [FynkelApp] for a
/// game that is the whole app (with a HUD over it, if any), or with a
/// [FynkelGame] and the [FynkelView] in its builder for a game inside a
/// bigger app: the [FynkelGame] makes the root, has its world ready before
/// the builder runs, starts it over (F5) and reads its shaders and sounds
/// again on a hot reload. A [FynkelView] alone (`FynkelView(root:)`) is
/// for a second view of a game, or a quick tool; it can't start the game
/// over. [FynkelViewStats] is what a view did each second, and
/// [bundleSource] loads assets from the app's bundle.
///
/// [WorldWidget] puts a widget at a place in the world, and [Stick] is an
/// on-screen stick. [ListenableSignal] lets game code listen to Flutter
/// notifiers, and [WorldListenables] lets widgets listen to the world.
/// [WorldTheme] gives the game the app's theme, and [FlutterColorToFynkel]
/// and [FynkelColorToFlutter] turn one's colours into the other's.
/// [Controls] are the keys a player can change, [ControlsSettings] the
/// settings page's list of them, [askForKey] the dialog that asks for one,
/// and [keyCodeOf] turns a Flutter key into the game's.
library;

export 'src/colors.dart' show FlutterColorToFynkel, FynkelColorToFlutter;
export 'src/controls.dart' show Controls, ControlsSettings, askForKey;
export 'src/fynkel_app.dart' show FynkelApp;
export 'src/fynkel_game.dart' show FynkelGame, FynkelGameState;
export 'src/fynkel_view.dart' show FynkelView, FynkelViewState, FynkelViewStats, bundleSource;
export 'src/input_feed.dart' show keyCodeOf;
export 'src/listenable_signal.dart' show ListenableSignal;
export 'src/stick.dart' show Stick;
export 'src/world_listenables.dart' show WorldListenables, WorldTheme;
export 'src/world_widget.dart' show WorldWidget, WorldWidgetPlace;
