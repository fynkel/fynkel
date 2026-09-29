// Unity parity: the Project browser's zoom slider and grid
// (docs/unity-parity.md § Tier 3). The slider at the status bar's right
// turns the list into tiles, bigger as it goes right, all the way left the
// list again; the size is remembered. A tile is the asset's picture over
// its name — a model, a prefab, a mesh or a material drawn by
// `fynkel thumbnail`, drawn again when its file changes — and clicks,
// double-clicks, renames, drags and expands like a row; the arrow keys
// move across and down the grid.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/project_browser/project_browser_model.dart';
import 'package:fynkel_editor/src/project_browser/thumbnails.dart';
import 'package:path/path.dart' as p;

import 'harness.dart';

const robot = 'c0000000-0000-4000-8000-000000000001';
const rusted = 'c0000000-0000-4000-8000-000000000020';
const crate = 'c0000000-0000-4000-8000-000000000040';
const spark = 'c0000000-0000-4000-8000-000000000041';
const coin = 'c0000000-0000-4000-8000-000000000042';

final _png = base64Decode(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
);

/// `fynkel thumbnail` as the contract has it: a PNG per asset, the coin
/// refused ("no thumbnail"), the answer as JSON.
final class FakeThumbnails {
  final calls = <List<String>>[];

  Future<ProcessResult> call(String exe, List<String> args, {String? workingDirectory}) async {
    calls.add(args);
    final out = args[args.indexOf('--out-dir') + 1];
    final assets = args.sublist(args.indexOf('--json') + 1);
    final results = <Object>[];
    for (final a in assets) {
      final [id, ...rest] = a.split('#');
      final sub = rest.isEmpty ? null : rest.single;
      if (id == coin) {
        results.add({'asset': id, 'sub': sub, 'error': 'the cook failed'});
        continue;
      }
      final file = p.join(out, ThumbnailCache.fileName(id, sub));
      File(file).writeAsBytesSync(_png);
      results.add({'asset': id, 'sub': sub, 'file': file});
    }
    return ProcessResult(1, 0, jsonEncode({'thumbnails': results}), '');
  }

  List<String> get requested => [for (final c in calls) ...c.sublist(c.indexOf('--json') + 1)];
}

/// A frame (the tiles ask for their pictures), the batch's wait, its answer.
Future<void> drawn(WidgetTester tester) async {
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 200));
  await settleCooks(tester);
}

void main() {
  testWidgets('the slider: a grid of tiles, remembered; all the way left, the list', (tester) async {
    final e = await Editor.open(tester);
    final browser = e.shell.browser!;
    browser.setFolder('prefabs');
    await settleCooks(tester);
    expect(find.byKey(const Key('project.list')), findsOneWidget);
    expect(find.byKey(const Key('project.columns')), findsOneWidget);

    final slider = find.byKey(const Key('project.zoom'));
    await tester.tapAt(tester.getCenter(slider));
    await settleCooks(tester);
    expect(browser.zoom, closeTo(.5, .15));
    expect(find.byKey(const Key('project.grid')), findsOneWidget);
    expect(find.byKey(const Key('project.list')), findsNothing);
    expect(find.byKey(const Key('project.columns')), findsNothing, reason: 'a grid has no columns');
    final tile = tester.getSize(find.byKey(const Key('project.asset.$coin')));
    expect(tile.height, greaterThan(browser.tileSize), reason: 'the picture over the name');
    expect(
      find.descendant(of: find.byKey(const Key('project.asset.$coin')), matching: find.text('coin')),
      findsOneWidget,
    );

    // Further right, bigger tiles; remembered by a new browser.
    await tester.tapAt(tester.getTopRight(slider) + Offset(-4, tester.getSize(slider).height / 2));
    await settleCooks(tester);
    expect(tester.getSize(find.byKey(const Key('project.asset.$coin'))).width, greaterThan(tile.width));
    final again = ProjectBrowserModel(project: browser.project)..recall(browser.remembered);
    addTearDown(again.dispose);
    expect(again.zoom, browser.zoom);
    expect(again.grid, isTrue);

    await tester.tapAt(tester.getTopLeft(slider) + Offset(1, tester.getSize(slider).height / 2));
    await settleCooks(tester);
    expect(browser.zoom, 0);
    expect(find.byKey(const Key('project.list')), findsOneWidget);
    expect(browser.remembered.containsKey('zoom'), isFalse);
  });

  testWidgets('tiles click, double-click, rename and step like rows; ← → ↑ ↓ move across the grid', (tester) async {
    final e = await Editor.open(
      tester,
      prepare: (dir) => File('${dir.path}/lib/game/spinner.dart')
        ..createSync(recursive: true)
        ..writeAsStringSync('// A script.\n'),
    );
    final browser = e.shell.browser!;
    browser.setFolder('prefabs');
    browser.setZoom(.3);
    await settleCooks(tester);

    final order = [for (final a in browser.visibleAssets) a.id];
    expect(order, containsAll([crate, spark, coin]));
    final width = tester.getSize(find.byKey(const Key('project.grid'))).width;
    final columns = ((width - 12) / (browser.tileSize + 16)).floor();
    expect(columns, greaterThan(1));

    await e.click(find.byKey(Key('project.asset.${order.first}')));
    expect(browser.lead, order.first);
    await e.press(LogicalKeyboardKey.arrowRight);
    expect(browser.lead, order[1]);
    await e.press(LogicalKeyboardKey.arrowRight, shift: true);
    expect(browser.selection, {order[1], order[2]}, reason: '⇧ extends');
    await e.press(LogicalKeyboardKey.arrowLeft);
    expect(browser.selection, {order[1]});
    await e.press(LogicalKeyboardKey.arrowDown);
    expect(browser.lead, order[(1 + columns).clamp(0, order.length - 1)], reason: 'a row down, or the last');
    await e.press(LogicalKeyboardKey.arrowUp);
    expect(browser.lead, order[0], reason: 'a row up, as far as the first');

    // Enter renames under the picture; a double-click opens the prefab.
    await e.click(find.byKey(const Key('project.asset.$coin')));
    await e.press(LogicalKeyboardKey.enter);
    expect(find.byKey(const Key('project.rename.$coin')), findsOneWidget);
    await e.press(LogicalKeyboardKey.escape);
    await tester.tap(find.byKey(const Key('project.asset.$crate')));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.byKey(const Key('project.asset.$crate')));
    await settleCooks(tester);
    expect(e.shell.tabs!.focused!.path, endsWith('crate.prefab.json'));

    // A folder is a tile too: a click goes into it.
    browser.setFolder('');
    await settleCooks(tester);
    await tester.tap(find.byKey(const Key('project.folder.row.models')));
    await settleCooks(tester);
    expect(browser.folder, 'models');

    // A model's ▸ at its tile's edge: its parts as tiles after it.
    await tester.tap(find.byKey(const Key('project.expand.$robot')));
    await settleCooks(tester);
    final part = find.byKey(const Key('project.part.$robot.mesh/Cube'));
    expect(part, findsOneWidget);
    await tester.tap(part);
    await settleCooks(tester);
    expect((browser.lead, browser.leadPart), (robot, 'mesh/Cube'));
    await e.press(LogicalKeyboardKey.arrowRight);
    expect(browser.lead, robot);
    expect(browser.leadPart, isNot('mesh/Cube'), reason: 'the next part');

    // Scripts: tiles too.
    browser.showScripts();
    await settleCooks(tester);
    expect(find.byKey(const Key('project.scripts.grid')), findsOneWidget);
    expect(find.byKey(const Key('project.scripts.row.game')), findsOneWidget, reason: 'lib/game as a tile');
  });

  testWidgets('thumbnails: drawn in the background, kept, drawn again when the file changes', (tester) async {
    final e = await Editor.open(tester);
    final browser = e.shell.browser!;
    final fake = FakeThumbnails();
    browser.thumbnails = ThumbnailCache(projectDir: e.dir.path, cli: () => '/fake/fynkel', run: fake.call);
    addTearDown(browser.thumbnails!.dispose);
    browser.setFolder('prefabs');
    browser.setZoom(.5);
    await settleCooks(tester);
    await drawn(tester);

    // One batch for what the folder shows that has a picture to draw.
    expect(fake.calls, hasLength(1));
    expect(fake.calls.single.take(8), [
      'thumbnail',
      '--project',
      e.dir.path,
      '--size',
      '128',
      '--out-dir',
      '${e.dir.path}/.fynkel/thumbnails',
      '--json',
    ]);
    expect(fake.requested, unorderedEquals([crate, spark, coin]));
    expect(find.byKey(const Key('project.thumbnail.$crate')), findsOneWidget);
    expect(find.byKey(const Key('project.thumbnail.$coin')), findsNothing, reason: 'refused: the icon');
    expect(File('${e.dir.path}/.fynkel/thumbnails/index.json').existsSync(), isTrue);

    // Shown again: nothing to draw, the refused one not asked again.
    browser.setZoom(.6);
    await drawn(tester);
    expect(fake.calls, hasLength(1));

    // The crate's file changes: drawn again, a new image.
    ImageProvider image() => tester.widget<Image>(find.byKey(const Key('project.thumbnail.$crate'))).image;
    final before = image();
    final file = File('${e.dir.path}/assets/prefabs/crate.prefab.json');
    file.setLastModifiedSync(file.lastModifiedSync().add(const Duration(seconds: 5)));
    browser.setZoom(.5);
    await drawn(tester);
    expect(fake.calls, hasLength(2));
    expect(fake.calls.last.sublist(8), [crate]);
    expect(image(), isNot(before));

    // A material, a model and its mesh part; a texture shows its own file.
    browser.setFolder('');
    browser.setView(BrowserView.list);
    await drawn(tester);
    browser.toggleExpanded(robot);
    await drawn(tester);
    expect(fake.requested, containsAll([rusted, robot, '$robot#mesh/Cube']));
    final textures = [
      for (final a in browser.entries)
        if (browserKindOf(a) == 'texture') a.id,
    ];
    expect(textures, isNotEmpty);
    expect(fake.requested.toSet().intersection(textures.toSet()), isEmpty, reason: 'a texture is its own picture');
    expect(find.byKey(const Key('project.thumbnail.$robot#mesh/Cube')), findsOneWidget);
    expect(File('${e.dir.path}/.fynkel/thumbnails/${robot}__mesh_Cube.png').existsSync(), isTrue);
  });
}
