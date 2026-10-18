// Every scene of the lab, in the order of docs/stage-4.md's and
// docs/stage-5.md's steps, then stage 6's, 7's, 8's, 9's and 10's. Each step adds its own.

import 'lab.dart';
import 'scenes/addons_basics.dart';
import 'scenes/camera_basics.dart';
import 'scenes/camera_rigs.dart';
import 'scenes/character_basics.dart';
import 'scenes/character_course.dart';
import 'scenes/colour_basics.dart';
import 'scenes/components.dart';
import 'scenes/contacts_basics.dart';
import 'scenes/controls_basics.dart';
import 'scenes/crowd.dart';
import 'scenes/eases.dart';
import 'scenes/fixed_step_basics.dart';
import 'scenes/glow.dart';
import 'scenes/flutter_basics.dart';
import 'scenes/theme_basics.dart';
import 'scenes/world_widgets_basics.dart';
import 'scenes/input.dart';
import 'scenes/joints_basics.dart';
import 'scenes/joints_bridge.dart';
import 'scenes/joints_chain.dart';
import 'scenes/joints_character.dart';
import 'scenes/lamps.dart';
import 'scenes/see_through.dart';
import 'scenes/shaders.dart';
import 'scenes/shadows.dart';
import 'scenes/sky.dart';
import 'scenes/layer_matrix.dart';
import 'scenes/layers_basics.dart';
import 'scenes/lifecycle.dart';
import 'scenes/look.dart';
import 'scenes/menus_basics.dart';
import 'scenes/mesh_colliders_basics.dart';
import 'scenes/motion.dart';
import 'scenes/motion_basics.dart';
import 'scenes/moving_basics.dart';
import 'scenes/physics_queries_basics.dart';
import 'scenes/pointers.dart';
import 'scenes/pooling_basics.dart';
import 'scenes/minimap_basics.dart';
import 'scenes/rebinding_basics.dart';
import 'scenes/split_screen_basics.dart';
import 'scenes/riding_basics.dart';
import 'scenes/rigidbody_basics.dart';
import 'scenes/rigidbody_stack.dart';
import 'scenes/rigidbody_walls.dart';
import 'scenes/routines.dart';
import 'scenes/shapes_basics.dart';
import 'scenes/signal_basics.dart';
import 'scenes/signals.dart';
import 'scenes/skinned.dart';
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
  ControlsBasicsScene.new,
  MotionBasicsScene.new,
  EasesScene.new,
  MotionScene.new,
  MovingBasicsScene.new,
  ShapesBasicsScene.new,
  PoolingBasicsScene.new,
  ColourBasicsScene.new,
  CameraBasicsScene.new,
  CameraRigsScene.new,
  LayersBasicsScene.new,
  FixedStepBasicsScene.new,
  PhysicsQueriesBasicsScene.new,
  RigidbodyBasicsScene.new,
  RigidbodyStackScene.new,
  RigidbodyWallsScene.new,
  ContactsBasicsScene.new,
  LayerMatrixScene.new,
  MeshCollidersBasicsScene.new,
  CharacterBasicsScene.new,
  CharacterCourseScene.new,
  RidingBasicsScene.new,
  JointsBasicsScene.new,
  JointsBridgeScene.new,
  JointsChainScene.new,
  JointsCharacterScene.new,
  AddonsBasicsScene.new,
  FlutterBasicsScene.new,
  ThemeBasicsScene.new,
  WorldWidgetsBasicsScene.new,
  MenusBasicsScene.new,
  RebindingBasicsScene.new,
  MinimapBasicsScene.new,
  SplitScreenBasicsScene.new,
  LookScene.new,
  LampsScene.new,
  ShadowsScene.new,
  SkyScene.new,
  SeeThroughScene.new,
  GlowScene.new,
  SkinnedScene.new,
  CrowdScene.new,
  ShadersScene.new,
];
