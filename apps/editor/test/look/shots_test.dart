// Screenshots of the whole editor for the UI refresh (docs/ui-refresh):
// the real shell over a copy of the fixture project, with the platform's
// own faces loaded (tests otherwise draw every glyph as a box), the
// Camera selected, saved as PNG at 2× — the pictures a before/after is
// judged by. Off unless FYNKEL_SHOTS names the folder to write into:
//
//   FYNKEL_SHOTS=/tmp/shots flutter test test/look/shots_test.dart
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart' show Dialog;
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/console/console_panel.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_panel.dart';
import 'package:fynkel_editor/src/inspector/inspector_panel.dart';
import 'package:fynkel_editor/src/play/game_panel.dart';
import 'package:fynkel_editor/src/shell/editor_selection.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';
import 'package:fynkel_editor/src/project_browser/project_panel.dart';

import '../parity/harness.dart';

final _out = Platform.environment['FYNKEL_SHOTS'];

const _controllerId = 'c0000000-0000-4000-8000-00000000c0de';
const _controller = '''
{
  "\$schema": "https://fynkel.dev/schemas/controller.schema.json",
  "formatVersion": 1,
  "id": "$_controllerId",
  "name": "Hero",
  "parameters": [
    { "name": "speed", "type": "float" },
    { "name": "grounded", "type": "bool", "default": true },
    { "name": "jump", "type": "trigger" }
  ],
  "layers": [
    {
      "name": "Base Layer",
      "defaultState": "Locomotion",
      "states": [
        { "name": "Locomotion", "motion": { "asset": "c0000000-0000-4000-8000-00000000b1e0" } },
        { "name": "Jump", "motion": { "asset": "c0000000-0000-4000-8000-000000000005", "sub": "anim/bob" }, "loop": false },
        { "name": "Wave", "motion": { "asset": "c0000000-0000-4000-8000-000000000005", "sub": "anim/wave" } }
      ],
      "transitions": [
        { "from": "Locomotion", "to": "Jump", "duration": 0.1, "conditions": [{ "parameter": "jump" }] },
        { "from": "Jump", "to": "Locomotion", "exitTime": 0.9, "duration": 0.2, "conditions": [{ "parameter": "grounded", "is": true }] },
        { "from": "Locomotion", "to": "Wave", "duration": 0.25, "conditions": [{ "parameter": "speed", "less": 0.1 }] },
        { "from": "Wave", "to": "Locomotion", "exitTime": 1 }
      ]
    },
    { "name": "Upper Body", "weight": 0.5, "root": "spine" }
  ],
  "editor": {
    "Base Layer": {
      "#entry": [-260, 0], "#any": [-260, 110],
      "Locomotion": [0, 0], "Jump": [280, -80], "Wave": [280, 80]
    }
  }
}
''';

Future<void> _font(String family, List<String> files) async {
  final loader = FontLoader(family);
  for (final f in files) {
    final file = File(f);
    if (file.existsSync()) loader.addFont(Future.value(ByteData.sublistView(file.readAsBytesSync())));
  }
  await loader.load();
  // Material's icons (the card menus' ⋯).
  await (FontLoader('MaterialIcons')..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
}

Future<void> _loadFonts() async {
  await _font('.AppleSystemUIFont', ['/System/Library/Fonts/SFNS.ttf']);
  await _font('SF Mono', ['/System/Library/Fonts/SFNSMono.ttf']);
  final lucide = rootBundle.load('packages/lucide_icons_flutter/assets/lucide.ttf');
  final loader = FontLoader('packages/lucide_icons_flutter/Lucide')..addFont(lucide);
  await loader.load();
}

void main() {
  testWidgets('the editor, the Camera selected', (tester) async {
    await tester.runAsync(_loadFonts);
    // The dark theme, the one the user works in (the editor follows the system).
    tester.platformDispatcher.platformBrightnessTestValue = Brightness.dark;
    addTearDown(tester.platformDispatcher.clearPlatformBrightnessTestValue);
    final e = await Editor.open(
      tester,
      tag: 'shots',
      prepare: (dir) {
        writeDemoScripts(dir);
        // A blend space over the rig's clips, for its preview.
        File('${dir.path}/assets/models/loco.blend.json').writeAsStringSync(
          '{"\$schema":"https://fynkel.dev/schemas/blend.schema.json","formatVersion":1,'
          '"id":"c0000000-0000-4000-8000-00000000b1e0","name":"Loco","parameter":"speed","points":['
          '{"at":0,"clip":{"asset":"c0000000-0000-4000-8000-000000000005","sub":"anim/wave"}},'
          '{"at":2,"clip":{"asset":"c0000000-0000-4000-8000-000000000005","sub":"anim/bob"}}],"sync":"phase"}',
        );
        // An Animator Controller over them, for the Animator window (ADR-0032).
        File('${dir.path}/assets/models/hero.controller.json').writeAsStringSync(_controller);
      },
    );
    tester.view.physicalSize = const Size(2880, 1800);
    tester.view.devicePixelRatio = 2;
    await tester.pumpAndSettle();
    await e.click(e.row('Camera'));

    Future<void> save(String name, [Finder? part]) async {
      // The view's own layer, in physical pixels (its transform is the
      // device pixel ratio).
      final layer = tester.binding.renderViews.first.debugLayer! as OffsetLayer;
      final dpr = tester.view.devicePixelRatio;
      final rect = part == null
          ? Offset.zero & tester.view.physicalSize
          : Rect.fromLTRB(
              tester.getRect(part).left * dpr,
              tester.getRect(part).top * dpr,
              tester.getRect(part).right * dpr,
              tester.getRect(part).bottom * dpr,
            );
      await tester.runAsync(() async {
        final image = await layer.toImage(rect);
        final png = await image.toByteData(format: ui.ImageByteFormat.png);
        File('$_out/$name.png')
          ..createSync(recursive: true)
          ..writeAsBytesSync(png!.buffer.asUint8List());
      });
    }

    await save('window');
    for (final (name, type) in [
      ('inspector', InspectorPanel),
      ('hierarchy', HierarchyPanel),
      ('project', ProjectPanel),
    ]) {
      final f = find.byType(type);
      if (f.evaluate().isNotEmpty) await save(name, f.first);
    }
    // The Project inside a folder (its path header), and the status bar.
    e.shell.browser!.setFolder('prefabs');
    await tester.pumpAndSettle();
    await save('project-folder', find.byType(ProjectPanel).first);
    e.shell.browser!.setFolder('');
    await tester.pumpAndSettle();
    await save('status', find.byKey(const Key('window.status')));
    // With a problem to show: a material that is not JSON.
    final broken = File('${e.dir.path}/assets/materials/broken.material.json')..writeAsStringSync('{');
    await tester.runAsync(() => e.shell.project!.revalidate());
    await tester.pumpAndSettle();
    await save('status-line', find.byKey(const Key('window.status')));
    broken.deleteSync();
    await tester.runAsync(() => e.shell.project!.revalidate());
    await tester.pumpAndSettle();
    // The Console shares the Project's dock: bring its tab forward.
    await tester.tap(find.text('Console').first);
    await tester.pumpAndSettle();
    await save('console', find.byType(ConsolePanel).first);
    // The Game view with its toolbar and the Stats box.
    e.shell.workspace!.show(PanelRegistry.game);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('game.stats')));
    await tester.pumpAndSettle();
    await save('game', find.byType(GamePanel).first);
    await save('window-game');
    // The Animator window with a transition selected, and the Inspector for it and for a state.
    e.shell.workspace!.show(PanelRegistry.animation);
    e.shell.selection!.inspect(const InspectAsset(_controllerId));
    await tester.pumpAndSettle();
    e.shell.animator!.selectTransition(0);
    await tester.pumpAndSettle();
    await save('animator', find.byKey(const Key('animator.panel')));
    await save('animator-transition', find.byType(InspectorPanel).first);
    await save('window-animator');
    e.shell.animator!.selectState('Locomotion');
    await tester.pumpAndSettle();
    await save('animator-state', find.byType(InspectorPanel).first);
    // The asset pages (docs/ui-refresh/images/asset-*).
    for (final (name, path) in [
      ('asset-prefab', 'prefabs/coin.prefab.json'),
      ('asset-scene', 'scenes/main.scene.json'),
      ('asset-material', 'materials/rusted.material.json'),
      ('asset-texture', 'textures/swatch.png'),
      ('asset-texture-normal', 'textures/rusted_normal.png'),
      ('asset-model', 'models/robot.glb'),
      ('asset-sound', 'audio/tone.wav'),
    ]) {
      final browser = e.shell.browser!;
      browser.select(browser.entries.firstWhere((a) => a.path == path).id);
      await tester.pumpAndSettle();
      // A preview's image is read and decoded off the fake clock.
      for (var i = 0; i < 8; i++) {
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 30)));
        await tester.pump();
      }
      await tester.pumpAndSettle();
      await save(name, find.byType(InspectorPanel).first);
    }
    // A clip on its model and a blend space (ADR-0030 step 7): paused
    // half-way, since a playing clip never lets the frames settle.
    Future<void> pause() async {
      for (var i = 0; i < 4; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }
      await tester.tap(find.byKey(const Key('preview.clip.play')));
      await tester.pump();
      final scrub = tester.getRect(find.byKey(const Key('preview.clip.scrub')));
      await tester.tapAt(scrub.center);
      await tester.pumpAndSettle();
    }
    {
      final browser = e.shell.browser!;
      browser.select(browser.entries.firstWhere((a) => a.path == 'models/rig.glb').id);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('preview.anim')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('preview.anim.anim/wave')));
      await pause();
      await save('asset-clip', find.byType(InspectorPanel).first);
      browser.select(browser.entries.firstWhere((a) => a.path == 'models/loco.blend.json').id);
      await pause();
      await save('asset-blend', find.byType(InspectorPanel).first);
    }
    // Info unfolded: the file's facts and who uses it.
    {
      final browser = e.shell.browser!;
      browser.select(browser.entries.firstWhere((a) => a.path == 'materials/rusted.material.json').id);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('section.Info')));
      await tester.pumpAndSettle();
      await tester.drag(find.byKey(const Key('section.Info')), const Offset(0, -600));
      await tester.pumpAndSettle();
      await save('asset-info', find.byType(InspectorPanel).first);
      await tester.tap(find.byKey(const Key('section.Info')));
      await tester.pumpAndSettle();
    }
    // The colour picker, from the material's base colour.
    {
      final browser = e.shell.browser!;
      browser.select(browser.entries.firstWhere((a) => a.path == 'materials/rusted.material.json').id);
      await tester.pumpAndSettle();
      final swatch = find.byKey(const Key('color.swatch')).first;
      await tester.ensureVisible(swatch);
      await tester.tap(swatch);
      await tester.pumpAndSettle();
      await save('picker', find.byType(Dialog));
      await tester.tap(find.text('Cancel'));
      await tester.pumpAndSettle();
    }
    // A script, as Unity's MonoScript page.
    e.shell.browser!.showScripts('game');
    e.shell.browser!.selectScript('game/player.dart');
    await tester.pumpAndSettle();
    await save('script', find.byType(InspectorPanel).first);
    e.shell.browser!.selectScript(null);
    await tester.pumpAndSettle();
    // With no document open: the empty panels.
    e.shell.registry.execute('document.close');
    await tester.pumpAndSettle();
    await save('empty');
  }, skip: _out == null);
}
