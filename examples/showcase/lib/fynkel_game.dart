// The game's one entry point (ADR-0021): lib/main.dart runs it with the
// launch from the environment, the editor's Play mounts it in the Game view
// with the open project and scene. The whole app — its own theme and
// navigator — so the editor shows exactly what a build shows.

import 'package:flutter/widgets.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'main.dart';

Widget buildGame(FynkelLaunch launch) => GameApp(launch: launch);
