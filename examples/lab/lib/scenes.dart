// Every scene of the lab, in the order of docs/stage-4.md's steps. Each
// step adds its own.

import 'lab.dart';
import 'scenes/components.dart';
import 'scenes/lifecycle.dart';
import 'scenes/routines.dart';
import 'scenes/signal_basics.dart';
import 'scenes/signals.dart';
import 'scenes/time.dart';

final List<LabScene Function()> scenes = [
  ComponentsScene.new,
  LifecycleScene.new,
  TimeScene.new,
  RoutinesScene.new,
  SignalBasicsScene.new,
  SignalsScene.new,
];
