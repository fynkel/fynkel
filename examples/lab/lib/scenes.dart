// Every scene of the lab, in the order of docs/stage-4.md's and
// docs/stage-5.md's steps. Each step adds its own.

import 'lab.dart';
import 'scenes/camera_basics.dart';
import 'scenes/camera_rigs.dart';
import 'scenes/components.dart';
import 'scenes/eases.dart';
import 'scenes/fixed_step_basics.dart';
import 'scenes/input.dart';
import 'scenes/layers_basics.dart';
import 'scenes/lifecycle.dart';
import 'scenes/motion.dart';
import 'scenes/motion_basics.dart';
import 'scenes/pointers.dart';
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
  InputScene.new,
  PointersScene.new,
  MotionBasicsScene.new,
  EasesScene.new,
  MotionScene.new,
  CameraBasicsScene.new,
  CameraRigsScene.new,
  LayersBasicsScene.new,
  FixedStepBasicsScene.new,
];
