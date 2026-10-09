import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show AssetSource, GameObject;

import 'fynkel_view.dart';

/// The whole app for a game that is only its view:
/// `void main() => runApp(FynkelApp(Garden()));`. It shows
/// `FynkelView(root: root)` full screen, with assets from the app's bundle
/// under `assets/` unless [source] says otherwise. A game with menus or a
/// HUD puts a [FynkelView] in its own app instead.
class FynkelApp extends StatelessWidget {
  const FynkelApp(this.root, {super.key, this.source, this.title = ''});

  /// The game, added when the app starts.
  final GameObject root;

  final AssetSource? source;

  /// The app's name for the system, such as the task switcher's.
  final String title;

  @override
  Widget build(BuildContext context) => WidgetsApp(
    title: title,
    color: const Color(0xFF000000),
    debugShowCheckedModeBanner: false,
    builder: (context, _) => FynkelView(root: root, source: source),
  );
}
