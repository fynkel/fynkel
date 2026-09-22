// What the Inspector shows (item 9c): the last thing selected anywhere,
// coming back to a panel, the padlock, and what goes away underneath it.
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/inspector/inspector_model.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/project_browser/project_browser_model.dart';
import 'package:fynkel_editor/src/shell/editor_selection.dart';

import 'support/fakes.dart';

const steel = 'm1-0000-0000-0000-000000000001';
const brick = 't1-0000-0000-0000-000000000001';

({DocumentTabs tabs, InspectorModel inspector, ProjectBrowserModel browser, EditorSelection selection}) rig({
  FakeProject? into,
}) {
  final project = into ?? FakeProject(dir: '/fake/selection');
  project.assetEntries = [
    const AssetEntry(
      id: steel,
      path: 'materials/steel.material.json',
      name: 'steel',
      kind: 'document',
      document: 'material',
    ),
    const AssetEntry(id: brick, path: 'textures/brick.png', name: 'brick', kind: 'source', importer: 'fynkel.image'),
  ];
  project.assetCount = project.assetEntries.length;
  project.fakeDocuments.add(
    FakeDocument(
      path: '/fake/selection/assets/main.scene.json',
      id: 'main',
      roots: ['a', 'b'],
      entities: {'a': FakeDocument.entity('A'), 'b': FakeDocument.entity('B')},
    ),
  );
  final tabs = DocumentTabs(project: project);
  final inspector = InspectorModel(tabs: tabs, project: project);
  final browser = ProjectBrowserModel(project: project);
  final selection = EditorSelection(inspector: inspector, browser: browser);
  addTearDown(() {
    selection.dispose();
    browser.dispose();
    inspector.dispose();
    tabs.dispose();
  });
  return (tabs: tabs, inspector: inspector, browser: browser, selection: selection);
}

void main() {
  test('the last selection wins, wherever it was made; coming back to a panel shows its selection again', () {
    final r = rig();
    expect(r.selection.target, isNull);
    final tab = r.tabs.openDocument('main')!;
    tab.setSelection(['a']);
    expect(r.selection.target, const InspectEntities());
    r.browser.select(steel);
    expect(r.selection.target, const InspectAsset(steel));
    // A click back in the Hierarchy, the same entity still selected.
    r.selection.panelFocused('hierarchy');
    expect(r.selection.target, const InspectEntities());
    r.selection.panelFocused('project');
    expect(r.selection.target, const InspectAsset(steel));
    r.browser.setFolder('textures');
    expect(r.selection.target, const InspectFolder('textures'));
    // Clearing the browser selection empties an asset page, not an entity page.
    r.browser.select(brick);
    r.browser.clearSelection();
    expect(r.selection.target, isNull);
    tab.setSelection(['b']);
    r.browser.select(steel);
    r.browser.clearSelection();
    expect(r.selection.target, isNull);
    // An empty entity selection does not take the Inspector.
    r.browser.select(brick);
    tab.setSelection([]);
    expect(r.selection.target, const InspectAsset(brick));
  });

  test('the padlock holds an asset page; unlocking shows the latest selection', () {
    final r = rig();
    final tab = r.tabs.openDocument('main')!;
    r.browser.select(steel);
    r.selection.setLocked(true);
    expect(r.selection.locked, isTrue);
    tab.setSelection(['a']);
    r.browser.select(brick);
    r.selection.panelFocused('hierarchy');
    expect(r.selection.target, const InspectAsset(steel));
    r.selection.setLocked(false);
    expect(r.selection.target, const InspectEntities(), reason: 'the Hierarchy was the last to speak');
  });

  test('locking the entity page pins those entities; the tab moving on does not move the page', () {
    final r = rig();
    final tab = r.tabs.openDocument('main')!;
    tab.setSelection(['a']);
    r.selection.setLocked(true);
    expect(r.inspector.locked, isTrue);
    tab.setSelection(['b']);
    expect(r.inspector.selection, ['a']);
    r.browser.select(steel);
    expect(r.selection.target, const InspectEntities());
    // In-panel navigation (a part clicked on the page) moves a locked Inspector and lets the entities go;
    // "Show in Inspector" from outside goes to an unlocked copy instead (item 9e, the shell).
    r.selection.inspect(const InspectAsset(brick));
    expect(r.selection.target, const InspectAsset(brick));
    expect(r.selection.locked, isTrue);
    expect(r.inspector.locked, isFalse);
    expect(r.inspector.selection, ['b']);
    r.selection.setLocked(false);
    expect(r.selection.target, const InspectAsset(brick));
  });

  test('unlocking a pinned entity page goes to what was selected meanwhile', () {
    final r = rig();
    final tab = r.tabs.openDocument('main')!;
    tab.setSelection(['a']);
    r.selection.setLocked(true);
    tab.setSelection(['b']);
    r.browser.select(steel);
    r.selection.setLocked(false);
    expect(
      r.selection.target,
      const InspectAsset(steel),
      reason: 'the entities coming unpinned are not a new selection',
    );
    expect(r.inspector.selection, ['b']);
    r.selection.panelFocused('hierarchy');
    expect(r.selection.target, const InspectEntities());
  });

  test('closing the tab of a pinned entity page lets the padlock go', () {
    final r = rig();
    final tab = r.tabs.openDocument('main')!;
    tab.setSelection(['a']);
    r.selection.setLocked(true);
    r.tabs.closeDiscarding(tab);
    expect(r.inspector.locked, isFalse);
    expect(r.selection.locked, isFalse);
  });

  test('a deleted asset leaves the page, locked or not', () {
    final r = rig();
    r.browser.select(brick);
    r.selection.setLocked(true);
    r.browser.delete(ids: [brick]);
    expect(r.browser.entry(brick), isNull);
    expect(r.selection.locked, isFalse);
    expect(r.selection.target, isNot(const InspectAsset(brick)));
  });

  test('nothing to lock before anything was selected', () {
    final r = rig();
    r.selection.setLocked(true);
    expect(r.selection.locked, isFalse);
  });

  test('item 9e: an Inspector follows every Project browser copy, and stops following a closed one', () {
    final r = rig();
    final second = ProjectBrowserModel(project: r.browser.project);
    addTearDown(second.dispose);
    r.selection.addBrowser(second);
    second.select(steel);
    expect(r.selection.target, const InspectAsset(steel));
    r.browser.select(brick);
    expect(r.selection.target, const InspectAsset(brick));
    second.setFolder('materials');
    expect(r.selection.target, const InspectFolder('materials'));
    // Coming back to a copy shows that copy's lead.
    r.selection.panelFocused('project', from: second);
    expect(r.selection.target, const InspectAsset(steel));
    r.selection.removeBrowser(second);
    second.clearSelection();
    second.select(brick);
    r.browser.select(steel);
    second.select(brick);
    expect(r.selection.target, const InspectAsset(steel), reason: 'the closed copy no longer moves it');
  });

  test('item 9e: a lock on an asset, a part or a folder is remembered; entities are not', () {
    final r = rig();
    expect(r.selection.remembered, isNull);
    r.browser.select(brick);
    expect(r.selection.remembered, isNull, reason: 'unlocked: nothing to bring back');
    r.selection.setLocked(true);
    expect(r.selection.remembered, {'asset': brick});
    r.selection.inspect(const InspectAsset(steel, sub: 'mesh/0'));
    expect(r.selection.remembered, {'asset': steel, 'sub': 'mesh/0'});
    r.selection.inspect(const InspectFolder('textures'));
    expect(r.selection.remembered, {'folder': 'textures'});
    r.selection.setLocked(false);
    final tab = r.tabs.openDocument('main')!;
    tab.setSelection(['a']);
    r.selection.setLocked(true);
    expect(r.selection.target, const InspectEntities());
    expect(r.selection.remembered, isNull, reason: 'a project opens with no documents: nothing to pin');

    // Recall: a target that still exists comes back locked; a gone one does not.
    final again = rig();
    again.selection.recall({'asset': steel, 'sub': 'mesh/0'});
    expect(again.selection.locked, isTrue);
    expect(again.selection.target, const InspectAsset(steel, sub: 'mesh/0'));
    again.browser.select(brick);
    expect(again.selection.target, const InspectAsset(steel, sub: 'mesh/0'));
    final gone = rig();
    gone.selection.recall({'asset': 'nope'});
    gone.selection.recall({'folder': 'nowhere'});
    gone.selection.recall('garbage');
    expect(gone.selection.locked, isFalse);
    expect(gone.selection.target, isNull);
    final root = rig();
    root.selection.recall({'folder': ''});
    expect(root.selection.target, const InspectFolder(''));
  });
}
