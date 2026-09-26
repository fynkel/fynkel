// The Inspector's 3D previews draw for real (ADR-0030 §3): a preview
// stage through a viewport of its own — Metal offscreen, the texture
// channel mocked as in the Scene view's render test. The asset is in the
// middle of the picture over the background, lit from one side, and an
// unsaved material edit shows at once. With FYNKEL_STAGE_SHOTS set, the
// frames are written there as PNGs to look at.
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/preview/stage_backend.dart';
import 'package:fynkel_editor/src/preview/stage_view.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/scene_view/png.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'project_test.dart' show copyFixture;

const rusted = 'c0000000-0000-4000-8000-000000000020';
const robot = 'c0000000-0000-4000-8000-000000000001';
const w = 320, h = 240;

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() {
    const channel = MethodChannel('fynkel_flutter/viewport');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      switch (call.method) {
        case 'registerTexture':
          return 11;
        case 'rebindTexture' || 'unregisterTexture':
          return null;
        case 'stats':
          return <String, Object?>{};
      }
      throw MissingPluginException(call.method);
    });
    addTearDown(
      () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, null),
    );
  });

  /// One frame of [stage] from its framing.
  Future<Uint8List> draw(StageBackend stage, String name) async {
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    controller.requestSize(w, h);
    for (var i = 0; i < 10; i++) {
      await Future<void>.delayed(Duration.zero);
    }
    expect(controller.isAttached, isTrue, reason: '${controller.error}');
    final cam = cameraOf(stage.state.framing);
    controller.view = ViewportView(world: cam.world, fovY: cam.fovY, near: cam.distance * 0.01, far: cam.distance * 4);
    final slot = controller.renderHandle(stage.worldAddress);
    expect(slot, isNotNull);
    final pixels = controller.readFrame(slot!)!;
    final shots = Platform.environment['FYNKEL_STAGE_SHOTS'];
    if (shots != null) File('$shots/$name.png').writeAsBytesSync(encodePng(w, h, pixels));
    return pixels;
  }

  List<int> at(Uint8List px, int x, int y) {
    final i = (y * w + x) * 4;
    return px.sublist(i, i + 3);
  }

  int diff(List<int> a, List<int> b) => (a[0] - b[0]).abs() + (a[1] - b[1]).abs() + (a[2] - b[2]).abs();

  test('a material, a model: in the middle, lit from one side, edits drawn at once', () async {
    final dir = copyFixture('stage-render');
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);

    final material = project.openStage(rusted);
    final sphere = await draw(material, 'material-sphere');
    final corner = at(sphere, 2, 2), middle = at(sphere, w ~/ 2, h ~/ 2);
    expect(diff(corner, middle), greaterThan(30), reason: 'the sphere stands out from the background');
    // Lit from the upper left of the view: its left side brighter than its right.
    final r = (h * 0.28).round();
    final left = at(sphere, w ~/ 2 - r ~/ 2, h ~/ 2 - r ~/ 3), right = at(sphere, w ~/ 2 + r ~/ 2, h ~/ 2 + r ~/ 3);
    expect(left.reduce((a, b) => a + b), greaterThan(right.reduce((a, b) => a + b)));

    material.setMaterial(rusted, {
      'shader': 'fynkel.pbr@1',
      'base_color': [0, 1, 0, 1],
    });
    final green = await draw(material, 'material-green');
    final g = at(green, w ~/ 2 - r ~/ 2, h ~/ 2 - r ~/ 3);
    expect(g[1], greaterThan(g[0] + 20), reason: 'green now: $g');
    expect(g[1], greaterThan(g[2] + 20), reason: 'green now: $g');

    final model = project.openStage(robot);
    final robotFrame = await draw(model, 'model-robot');
    var covered = 0;
    final bg = at(robotFrame, 2, 2);
    for (var y = 0; y < h; y += 4) {
      for (var x = 0; x < w; x += 4) {
        if (diff(at(robotFrame, x, y), bg) > 30) covered++;
      }
    }
    // Framed: it fills a good part of the picture without spilling out.
    final share = covered / ((w ~/ 4) * (h ~/ 4));
    expect(share, inInclusiveRange(0.08, 0.8), reason: 'covers $share');
  });
}
