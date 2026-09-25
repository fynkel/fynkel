// Screenshots of the whole editor for the UI refresh (docs/ui-refresh):
// the real shell over a copy of the fixture project, with the platform's
// own faces loaded (tests otherwise draw every glyph as a box), the
// Camera selected, saved as PNG at 2× — the pictures a before/after is
// judged by. Off unless FYNKEL_SHOTS names the folder to write into:
//
//   FYNKEL_SHOTS=/tmp/shots flutter test test/look/shots_test.dart
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/console/console_panel.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_panel.dart';
import 'package:fynkel_editor/src/inspector/inspector_panel.dart';
import 'package:fynkel_editor/src/project_browser/project_panel.dart';

import '../parity/harness.dart';

final _out = Platform.environment['FYNKEL_SHOTS'];

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
    final e = await Editor.open(tester, tag: 'shots');
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
    // The Console shares the Project's dock: bring its tab forward.
    await tester.tap(find.text('Console').first);
    await tester.pumpAndSettle();
    await save('console', find.byType(ConsolePanel).first);
    // With no document open: the empty panels.
    e.shell.registry.execute('document.close');
    await tester.pumpAndSettle();
    await save('empty');
  }, skip: _out == null);
}
