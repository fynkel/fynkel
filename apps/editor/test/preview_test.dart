// The Inspector's preview area (ADR-0030): under an asset's page, a
// header with the asset's name and the preview's toolbar, the picture and
// its info line; folded and resized per preview, kept in the preferences.
// The texture preview: channels, mips, the GPU size line. A package's
// preview comes before the editor's own, and one that throws fails alone.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/extensions/editor_extensions.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';
import 'package:fynkel_editor/src/inspector/inspector_panel.dart';
import 'package:fynkel_editor/src/preview/clip_bar.dart';
import 'package:fynkel_editor/src/preview/preview_area.dart';
import 'package:fynkel_editor/src/preview/sound_preview.dart';
import 'package:fynkel_editor/src/preview/stage_previews.dart';
import 'package:fynkel_editor/src/preview/stage_view.dart';
import 'package:fynkel_editor/src/preview/texture_preview.dart';
import 'package:fynkel_editor/src/theme/icons.dart';

import 'parity/harness.dart';

const swatch = 'c0000000-0000-4000-8000-000000000002';

/// The file is read and decoded off the test's fake clock, one real step
/// at a time (read, buffer, descriptor, codec, frame).
Future<void> settleImage(WidgetTester tester) async {
  for (var i = 0; i < 8; i++) {
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 30)));
    await tester.pump();
  }
  await tester.pumpAndSettle();
}

void main() {
  setUp(() {
    PreviewLayout.read = (_) => null;
    PreviewLayout.write = (_, _) {};
  });

  Future<void> select(Editor e, String id) async {
    e.shell.browser!.select(id);
    await e.tester.pumpAndSettle();
    await settleImage(e.tester);
  }

  String pathId(Editor e, String path) => e.shell.browser!.entries.firstWhere((a) => a.path == path).id;

  String info(WidgetTester tester) => tester.widget<Text>(find.byKey(const Key('preview.info'))).data!;

  test('mip counts and GPU sizes, as the cook builds the chain', () {
    expect(mipCount(const Size(4, 3)), 3);
    expect(mipCount(const Size(512, 512)), 10);
    expect(gpuBytes(const Size(4, 3)), 48 + 8 + 4);
    // ADR-0061: ASTC blocks of 16 bytes; 1026² at 6×6 is 171² blocks at level 0.
    expect(gpuBytes(const Size(6, 6), block: 6), 16 * 3);
    expect(gpuBytes(const Size(8, 8), block: 4), 16 * (4 + 1 + 1 + 1));
    expect(compressionBlock({}), 6);
    expect(compressionBlock({'colorSpace': 'linear'}), 4);
    expect(compressionBlock({'compression': 'none'}), isNull);
    expect(compressionBlock({'compression': 'low', 'colorSpace': 'linear'}), 8);
    expect(blockMultiple(1024, 6), 1026);
    expect(blockMultiple(1024, 4), 1024);
    expect(blockMultiple(4, 6), 6);
    expect(channelMatrix('rgb'), isNull);
    expect(channelMatrix('g')!.sublist(0, 5), [0, 1, 0, 0, 0]);
  });

  testWidgets('a texture: the picture, the info line, channels and mips', (tester) async {
    final e = await Editor.open(tester, tag: 'preview');
    await select(e, swatch);
    expect(find.byKey(const Key('preview.area')), findsOneWidget);
    expect(tester.widget<Text>(find.byKey(const Key('preview.title'))).data, 'swatch');
    expect(find.byKey(const Key('preview.image')), findsOneWidget);
    expect(info(tester), '4x3  RGBA8  sRGB  60 B  mips 3');

    // One channel is drawn as grey.
    await tester.tap(find.byKey(const Key('preview.channel.a')));
    await tester.pumpAndSettle();
    expect(
      find.ancestor(of: find.byKey(const Key('preview.image')), matching: find.byType(ColorFiltered)),
      findsOneWidget,
    );
    await tester.tap(find.byKey(const Key('preview.channel.rgb')));
    await tester.pumpAndSettle();
    expect(
      find.ancestor(of: find.byKey(const Key('preview.image')), matching: find.byType(ColorFiltered)),
      findsNothing,
    );

    // The mip slider's last stop: 1x1.
    final slider = tester.getRect(find.byKey(const Key('preview.mip')));
    await tester.tapAt(slider.centerRight - const Offset(2, 0));
    await settleImage(tester);
    expect(info(tester), '4x3  (mip 2: 1x1)  RGBA8  sRGB  60 B  mips 3');

    // F fits it again after a zoom (the picture has focus once clicked).
    await tester.tap(find.byKey(const Key('preview.image')));
    await tester.sendKeyEvent(LogicalKeyboardKey.keyF);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('preview.image')), findsOneWidget);
  });

  testWidgets('folds and resizes, kept per preview', (tester) async {
    final e = await Editor.open(tester, tag: 'preview');
    // After the shell bound them to its preferences.
    final kept = <String, Map<String, Object?>>{};
    PreviewLayout.read = (p) => kept[p];
    PreviewLayout.write = (p, l) => kept[p] = l;
    await select(e, swatch);
    final before = tester.getSize(find.byKey(const Key('preview.picture'))).height;

    // Up by 60: taller by 60.
    await tester.drag(find.byKey(const Key('preview.resize')), const Offset(0, -60));
    await tester.pumpAndSettle();
    expect(kept['texture']!['height'], closeTo(PreviewLayout.defaultHeight + 60, 1));
    expect(tester.getSize(find.byKey(const Key('preview.picture'))).height, greaterThan(before + 50));

    await tester.tap(find.byKey(const Key('preview.fold')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('preview.picture')), findsNothing);
    expect(find.byKey(const Key('preview.channel.rgb')), findsNothing);
    expect(kept['texture']!['open'], isFalse);

    // Another texture opens folded too; unfolding shows it at the kept height.
    await select(e, pathId(e, 'textures/rusted_mr.png'));
    expect(find.byKey(const Key('preview.picture')), findsNothing);
    await tester.tap(find.byKey(const Key('preview.fold')));
    await tester.pumpAndSettle();
    expect(kept['texture']!['open'], isTrue);
  });

  testWidgets('a package\'s preview comes first; one that throws fails alone', (tester) async {
    final e = await Editor.open(
      tester,
      tag: 'preview',
      extensions: [
        EditorPart('tools', (editor) {
          editor.preview(
            kind: 'texture',
            accepts: (a) => a.name == 'swatch.png',
            build: (context, preview) => Text('mine ${preview.asset.name}', key: const Key('tools.picture')),
            toolbar: (context, preview) => const Text('tools'),
            info: (preview) => 'from tools',
          );
          editor.preview(kind: 'material', build: (context, preview) => throw StateError('broken'));
          editor.preview(build: (context, preview) => const SizedBox());
        }),
      ],
    );
    final host = EditorExtensionScope.maybeOf(tester.element(find.byType(InspectorPanel).first))!;
    expect(host.previews, hasLength(2));
    expect(host.log.map((d) => d.message), contains(contains('a preview left out')));

    await select(e, swatch);
    expect(find.text('mine swatch.png'), findsOneWidget);
    expect(info(tester), 'from tools');
    // The editor's own still draws the other textures.
    await select(e, pathId(e, 'textures/rusted_mr.png'));
    expect(find.byKey(const Key('tools.picture')), findsNothing);
    expect(find.byKey(const Key('preview.channel.rgb')), findsOneWidget);

    final material = e.shell.browser!.entries.firstWhere((a) => a.path == 'materials/rusted.material.json').id;
    await select(e, material);
    expect(find.byKey(const Key('preview.error')), findsOneWidget);
    expect(host.log.where((d) => d.message.contains('the material preview failed')), hasLength(1));
  });

  testWidgets('no preview for an asset none takes', (tester) async {
    final e = await Editor.open(tester, tag: 'preview');
    await select(e, e.shell.browser!.entries.firstWhere((a) => a.path == 'scenes/main.scene.json').id);
    expect(find.byKey(const Key('preview.area')), findsNothing);
  });

  test('a preview\'s settings outlive its height and fold', () {
    final prefs = <String, Map<String, Object?>>{};
    PreviewLayout.read = (p) => prefs[p];
    PreviewLayout.write = (p, l) => prefs[p] = l;
    PreviewLayout.setSetting('material', 'shape', 'cube');
    PreviewLayout.set('material', height: 300, open: false);
    expect(PreviewLayout.setting('material', 'shape'), 'cube');
    expect(PreviewLayout.heightOf('material'), 300);
    PreviewLayout.setSetting('material', 'shape', null);
    expect(PreviewLayout.setting('material', 'shape'), isNull);
    expect(PreviewLayout.isOpen('material'), isFalse);
  });

  test('the info line counts what the stage draws', () {
    expect(stageInfo(null, lead: 'Sphere'), 'Sphere');
    const cube = PreviewStageStats(meshes: 1, vertices: 24, triangles: 12, size: [1, 1, 1]);
    expect(stageInfo(cube, lead: 'Cube'), 'Cube  24 verts  12 tris  1.00 × 1.00 × 1.00 m');
    const model = PreviewStageStats(meshes: 3, vertices: 12345, triangles: 20000, skinned: 1, size: [0.5, 12, 2]);
    expect(stageInfo(model), '3 meshes  1 skinned  12,345 verts  20,000 tris  0.50 × 12.0 × 2.00 m');
    const rigged = PreviewStageStats(meshes: 1, vertices: 8, triangles: 12, skinned: 1, bones: 3);
    expect(stageInfo(rigged), '1 mesh  1 skinned  3 bones  8 verts  12 tris');
  });

  StageViewState stageOf(WidgetTester tester) => tester.state<StageViewState>(find.byType(StageView));

  testWidgets('a material on a shape, from the menu; the stage follows its edits', (tester) async {
    final e = await Editor.open(
      tester,
      tag: 'preview',
      // A second material: the rusted one under another id.
      prepare: (dir) {
        final rusted = File('${dir.path}/assets/materials/rusted.material.json').readAsStringSync();
        File('${dir.path}/assets/materials/hazard.material.json').writeAsStringSync(
          rusted.replaceFirst(RegExp(r'"id":\s*"[^"]+"'), '"id": "c0000000-0000-4000-8000-0000000000a1"'),
        );
      },
    );
    await select(e, pathId(e, 'materials/rusted.material.json'));
    expect(find.byKey(const Key('preview.stage')), findsOneWidget);
    expect(find.byKey(const Key('preview.stage.error')), findsNothing);
    final stage = stageOf(tester).stage!;
    expect(stage.state.subject, 'material');
    expect(info(tester), startsWith('Sphere  '));
    expect(info(tester), contains(' tris  1.00 × 1.00 × 1.00 m'));

    await tester.tap(find.byKey(const Key('preview.shape')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('preview.shape.cube')));
    await tester.pumpAndSettle();
    expect(identical(stageOf(tester).stage, stage), isTrue, reason: 'the same stage, reshaped');
    expect(info(tester), 'Cube  24 verts  12 tris  1.00 × 1.00 × 1.00 m');

    // As Unity's: the shape last picked is every material's, and kept.
    await select(e, pathId(e, 'materials/hazard.material.json'));
    expect(stageOf(tester).stage!.state.shape, 'cube');
    expect(info(tester), startsWith('Cube  '));
    expect(PreviewLayout.setting(materialPreview.title, 'shape'), 'cube', reason: 'in the preferences');
    await select(e, pathId(e, 'materials/rusted.material.json'));
    expect(stageOf(tester).stage!.state.shape, 'cube');

    // Its page's edits are drawn by the stage too (no Scene view needed).
    final edits = e.shell.assetEdits!;
    expect(edits.watchers, isNotEmpty);
  });

  testWidgets('a model whole or one mesh; a prefab as it is', (tester) async {
    final e = await Editor.open(tester, tag: 'preview');
    await select(e, pathId(e, 'models/robot.glb'));
    expect(stageOf(tester).stage!.state.subject, 'hierarchy');
    final whole = info(tester);
    expect(whole, contains('meshes'));

    await tester.tap(find.byKey(const Key('preview.part')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('preview.part.mesh/Cube')));
    await tester.pumpAndSettle();
    expect(stageOf(tester).stage!.state.subject, 'mesh');
    expect(info(tester), startsWith('1 mesh  24 verts  12 tris'));
    expect(
      tester.widget<Text>(find.descendant(of: find.byKey(const Key('preview.part')), matching: find.byType(Text))).data,
      'Cube',
    );

    await select(e, pathId(e, 'prefabs/crate.prefab.json'));
    expect(stageOf(tester).stage!.state.subject, 'hierarchy');
    expect(info(tester), contains('verts'));
  });

  test('the sound line reads as Unity\'s', () {
    const vorbis = PreviewSoundInfo(sampleRate: 44100, channels: 2, duration: 3.21, codec: 'vorbis', streamed: false);
    expect(soundInfo(vorbis, bytes: 212 * 1024), 'Vorbis, 44100 Hz, Stereo, 0:03.210, 212 KB');
    const long = PreviewSoundInfo(sampleRate: 22050, channels: 1, duration: 75.5, codec: 'pcm', streamed: true);
    expect(soundInfo(long), 'PCM, 22050 Hz, Mono, 1:15.500, Streamed');
  });

  testWidgets('a sound: the waveform, the line, play and stop, a click plays from there', (tester) async {
    final e = await Editor.open(tester, tag: 'preview');
    await select(e, pathId(e, 'audio/tone.wav'));
    expect(find.byKey(const Key('preview.waveform')), findsOneWidget);
    expect(find.byKey(const Key('preview.sound.error')), findsNothing);
    expect(info(tester), 'PCM, 8000 Hz, Stereo, 0:00.050, 2 KB');
    SoundViewState view() => tester.state<SoundViewState>(find.byType(SoundView));
    expect(view().stage!.state.sound!.frames, 400);

    await tester.tap(find.byKey(const Key('preview.loop')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('preview.play')));
    await tester.pump();
    expect(view().isPlaying, isTrue, reason: 'looping, it keeps playing');
    expect(find.descendant(of: find.byKey(const Key('preview.play')), matching: find.byIcon(FIcons.stop)), findsOneWidget);
    await tester.tap(find.byKey(const Key('preview.play')));
    await tester.pump();
    expect(view().isPlaying, isFalse);
    expect(view().time, 0);

    // A click three quarters in plays from there.
    final wave = tester.getRect(find.byKey(const Key('preview.waveform')));
    await tester.tapAt(Offset(wave.left + wave.width * 0.75, wave.center.dy));
    await tester.pump();
    expect(view().isPlaying, isTrue);
    expect(view().time, closeTo(0.0375, 0.005));
    expect(find.byKey(const Key('preview.sound.time')), findsOneWidget);
    await tester.tap(find.byKey(const Key('preview.play')));
    await tester.pumpAndSettle();

    // Not looping, it runs out by itself: the button is Play again.
    await tester.tap(find.byKey(const Key('preview.loop')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('preview.play')));
    await tester.pump();
    expect(view().isPlaying, isTrue);
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 120)));
    await tester.pump(const Duration(milliseconds: 16));
    await tester.pumpAndSettle();
    expect(view().isPlaying, isFalse);
    expect(find.descendant(of: find.byKey(const Key('preview.play')), matching: find.byIcon(FIcons.playArrow)), findsOneWidget);
  });

  test('the clip bar\'s time and speed labels', () {
    expect(clipTime(1.2), '0:01.20');
    expect(clipTime(75.456), '1:15.46');
    expect(speedLabel(0.25), '0.25×');
    expect(speedLabel(1), '1×');
  });

  testWidgets('a clip on its model: from the menu, played, paused, scrubbed, slowed, once', (tester) async {
    final e = await Editor.open(tester, tag: 'preview');
    await select(e, pathId(e, 'models/rig.glb'));
    expect(info(tester), contains(' bones  '));
    expect(find.byKey(const Key('preview.clip')), findsNothing);

    await tester.tap(find.byKey(const Key('preview.anim')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('preview.anim.anim/wave')));
    await tester.pump();
    final view = stageOf(tester);
    expect(view.clip!.sub, 'anim/wave');
    expect(view.isPlaying, isTrue, reason: 'as Unity, it starts playing');
    expect(find.byKey(const Key('preview.clip')), findsOneWidget);
    expect(find.byKey(const Key('preview.part')), findsNothing, reason: 'a clip poses the whole model');
    final duration = view.clip!.duration;
    expect(duration, greaterThan(0));

    // The clock runs with the frames.
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump(const Duration(milliseconds: 100));
    final t1 = view.clip!.time;
    expect(t1, greaterThan(0));

    await tester.tap(find.byKey(const Key('preview.clip.play')));
    await tester.pump();
    expect(view.isPlaying, isFalse);
    await tester.pump(const Duration(milliseconds: 200));
    expect(view.clip!.time, t1, reason: 'paused');

    // A scrub to the start pauses and poses there.
    final scrub = tester.getRect(find.byKey(const Key('preview.clip.scrub')));
    await tester.tapAt(scrub.centerLeft + const Offset(1, 0));
    await tester.pump();
    expect(view.clip!.time, lessThan(duration * 0.1));
    expect(tester.widget<Text>(find.byKey(const Key('preview.clip.time'))).data, startsWith('0:00.'));

    // Half speed, no loop: runs out at the end and stops there.
    await tester.tap(find.byKey(const Key('preview.clip.speed')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('preview.clip.speed.0.5')));
    await tester.pumpAndSettle();
    expect(view.speed, 0.5);
    await tester.tap(find.byKey(const Key('preview.clip.loop')));
    await tester.pump();
    expect(view.loop, isFalse);
    await tester.tap(find.byKey(const Key('preview.clip.play')));
    await tester.pump();
    for (var i = 0; i < (duration * 2 / 0.1).ceil() + 3; i++) {
      await tester.pump(const Duration(milliseconds: 100));
    }
    expect(view.isPlaying, isFalse);
    expect(view.clip!.time, closeTo(duration, 1e-3));

    // Root motion let move, then back in place.
    await tester.tap(find.byKey(const Key('preview.clip.move')));
    await tester.pump();
    expect(view.clip!.inPlace, isFalse);

    // No clip: the bar goes, the parts come back.
    await tester.tap(find.byKey(const Key('preview.anim')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('preview.anim.none')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('preview.clip')), findsNothing);
    expect(find.byKey(const Key('preview.part')), findsOneWidget);
  });

  testWidgets('a blend space plays on its model with its parameter', (tester) async {
    const rig = 'c0000000-0000-4000-8000-000000000005';
    final e = await Editor.open(
      tester,
      tag: 'preview',
      prepare: (dir) => File('${dir.path}/assets/models/loco.blend.json').writeAsStringSync(
        jsonEncode({
          r'$schema': 'https://fynkel.dev/schemas/blend.schema.json',
          'formatVersion': 1,
          'id': 'c0000000-0000-4000-8000-00000000b1e0',
          'name': 'Loco',
          'parameter': 'speed',
          'points': [
            {
              'at': 0.0,
              'clip': {'asset': rig, 'sub': 'anim/wave'},
            },
            {
              'at': 2.0,
              'clip': {'asset': rig, 'sub': 'anim/bob'},
            },
          ],
          'sync': 'phase',
        }),
      ),
    );
    // It plays at once: frames never settle while it does.
    e.shell.browser!.select(pathId(e, 'models/loco.blend.json'));
    for (var i = 0; i < 5; i++) {
      await tester.pump(const Duration(milliseconds: 50));
    }
    expect(find.byKey(const Key('preview.stage.error')), findsNothing);
    final view = stageOf(tester);
    expect(view.clip!.blend!.parameter, 'speed');
    expect(view.isPlaying, isTrue);
    final slider = tester.getRect(find.byKey(const Key('preview.clip.parameter')));
    await tester.tapAt(slider.centerRight - const Offset(2, 0));
    await tester.pump();
    expect(view.clip!.blend!.value, greaterThan(1.5));
    await tester.tap(find.byKey(const Key('preview.clip.play')));
    await tester.pump();
    expect(view.isPlaying, isFalse);
  });
}
