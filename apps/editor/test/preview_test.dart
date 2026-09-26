// The Inspector's preview area (ADR-0030): under an asset's page, a
// header with the asset's name and the preview's toolbar, the picture and
// its info line; folded and resized per preview, kept in the preferences.
// The texture preview: channels, mips, the GPU size line. A package's
// preview comes before the editor's own, and one that throws fails alone.
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/extensions/editor_extensions.dart';
import 'package:fynkel_editor/src/inspector/inspector_panel.dart';
import 'package:fynkel_editor/src/preview/preview_area.dart';
import 'package:fynkel_editor/src/preview/texture_preview.dart';

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
}
