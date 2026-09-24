// The editor's words in one table (blueprint item 9; editor-ux §22.18:
// "keep editor strings localizable even if 1.0 ships only in English").
// Every command's label and description by its stable id, the menus, the
// panels, and the templates the generated commands fill in. A translation
// is another table with the same keys; nothing outside this file names a
// command's words.
//
// Not yet here: the words inside each panel (its buttons, empty states,
// messages). They move in as their panels are touched.

/// One command's words: the label (menus, the palette, tooltips) and the
/// palette's longer description, if it has one.
typedef CommandText = ({String label, String? description});

abstract final class EditorStrings {
  /// Every command with fixed words, by id (`EditorCommand.id`).
  static const commands = <String, CommandText>{
    'app.preferences': (label: 'Preferences…', description: null),
    'app.quit': (label: 'Quit Fynkel Editor', description: null),
    'assets.copyId': (label: 'Copy Asset Id', description: null),
    'assets.delete': (
      label: 'Delete Asset',
      description: 'Asks first; to .fynkel/trash; refused while referenced unless you say "delete anyway"',
    ),
    'assets.duplicate': (
      label: 'Duplicate Asset',
      description: 'A copy beside it (coin 1, …) under a fresh id; ⌘D in the Project browser',
    ),
    'assets.move': (label: 'Move Asset to Folder…', description: null),
    'assets.newFolder': (label: 'New Folder', description: null),
    'assets.newMaterial': (label: 'New Material', description: 'A fynkel.pbr@1 material with the shader\'s defaults'),
    'assets.newPrefab': (label: 'New Prefab', description: null),
    'assets.newScene': (label: 'New Scene', description: 'An empty scene document in the Project browser\'s folder'),
    'assets.open': (
      label: 'Open Asset',
      description: 'The selected scene or prefab into a tab; any other asset into the Inspector',
    ),
    'assets.redo': (label: 'Redo Asset Operation', description: null),
    'assets.reimport': (
      label: 'Reimport Asset',
      description: 'Runs the selected source through its importer with the sidecar\'s settings',
    ),
    'assets.rename': (
      label: 'Rename Asset',
      description: 'The file and its sidecar move together; the id and every reference survive',
    ),
    'assets.reveal': (label: 'Reveal Asset in Finder', description: null),
    'assets.search': (label: 'Search Project', description: null),
    'assets.toggleView': (label: 'Toggle Project Folders / List', description: null),
    'assets.undo': (
      label: 'Undo Asset Operation',
      description: 'Takes back the last rename, move, delete, new document or import-settings change',
    ),
    'console.copy': (label: 'Copy Console', description: null),
    'console.focusSearch': (label: 'Search Console', description: null),
    'console.openInEditor': (
      label: 'Open Console Row in Editor',
      description: 'The diagnostic\'s scene or prefab into a tab, its entity selected',
    ),
    'console.openInIde': (label: 'Open Console Row in IDE', description: null),
    'document.close': (label: 'Close Document', description: null),
    'document.next': (label: 'Next Document', description: null),
    'document.open': (label: 'Open Document…', description: 'A scene or prefab of the project, into a tab'),
    'document.previous': (label: 'Previous Document', description: null),
    'document.redo': (label: 'Redo', description: null),
    'document.revert': (label: 'Revert Document to Saved', description: null),
    'document.save': (label: 'Save Document', description: null),
    'document.saveAll': (label: 'Save All Documents', description: null),
    'document.saveAs': (label: 'Save Document As…', description: null),
    'document.undo': (label: 'Undo', description: null),
    'help.about': (label: 'About Fynkel Editor', description: null),
    'help.documentation': (label: 'Editor Documentation', description: null),
    'hierarchy.collapseAll': (label: 'Collapse All', description: null),
    'hierarchy.copy': (label: 'Copy', description: 'The selected objects and everything under them'),
    'hierarchy.cut': (label: 'Cut', description: null),
    'hierarchy.paste': (label: 'Paste', description: 'Next to the selected object, or at the root'),
    'hierarchy.pasteAsChild': (label: 'Paste as Child', description: 'Under the selected object'),
    'hierarchy.createEmpty': (label: 'Create Empty', description: 'A new object at the root of the scene'),
    'hierarchy.createEmptyChild': (label: 'Create Empty Child', description: 'A new object under the selected one'),
    'hierarchy.createWith': (label: 'Create Entity with Component…', description: null),
    'hierarchy.delete': (label: 'Delete', description: null),
    'hierarchy.duplicate': (label: 'Duplicate', description: null),
    'hierarchy.expandAll': (label: 'Expand All', description: null),
    'hierarchy.keepWorldTransform': (label: 'Keep World Transform when Reparenting', description: null),
    'hierarchy.rename': (label: 'Rename Entity', description: null),
    'hierarchy.search': (label: 'Search Hierarchy', description: null),
    'inspector.addComponent': (
      label: 'Add Component…',
      description: 'A native component or a behaviour, on every selected entity',
    ),
    'inspector.checkCodegen': (
      label: 'Check Generated Behaviour Files',
      description: 'Runs `fynkel codegen --check` on the project (never writes)',
    ),
    'inspector.openScript': (label: 'Open Script', description: null),
    'play.buildAndRun': (
      label: 'Build and Run',
      description:
          'Run the game as its own app in its own window (`fynkel run`): release-like speed, or a crash that '
          'must not take the editor down. It plays the saved files',
    ),
    'play.clearConsole': (label: 'Clear Game Console', description: null),
    'play.gizmos': (
      label: 'Show Gizmos in Play',
      description:
          'The next Play draws the game\'s `Gizmos` calls in its window (`FYNKEL_GIZMOS=1`; debug and profile builds)',
    ),
    'play.pause': (label: 'Pause', description: 'Pause or resume the game in the Game view — its loop and its audio'),
    'play.restart': (
      label: 'Restart to Apply Code Changes',
      description:
          'Save every document and restart the editor with the current code (for a change a hot reload cannot apply); '
          'the open tabs and selection come back',
    ),
    'play.showConsole': (
      label: 'Game Console',
      description: 'The Console\'s Game tab: the running game\'s logs, diagnostics and state',
    ),
    'play.start': (
      label: 'Play',
      description:
          'Run the game in the Game view (a project editor, `fynkel editor`); the plain editor has no game '
          'compiled in and builds and runs it as its own app',
    ),
    'play.step': (label: 'Step', description: 'Pause the game and run one fixed step'),
    'play.stop': (label: 'Stop', description: 'Kill the game and everything it started'),
    'prefab.applyOverrides': (
      label: 'Apply to Prefab',
      description: 'The selected instance\'s overrides become the prefab\'s values, then are reverted here',
    ),
    'prefab.open': (
      label: 'Open Prefab',
      description: 'The selected instance\'s prefab in its own tab (an imported hierarchy read-only)',
    ),
    'prefab.revertOverrides': (
      label: 'Revert Prefab Overrides',
      description: 'Drops every override of the selected instance — back to the prefab',
    ),
    'prefab.selectAsset': (
      label: 'Select Prefab Asset',
      description: 'Reveals the selected instance\'s prefab (or model source) in the Project browser',
    ),
    'prefab.unpack': (
      label: 'Unpack Prefab Instance',
      description: 'The instance becomes plain entities of this document (one undo step)',
    ),
    'project.close': (label: 'Close Project', description: null),
    'project.migrations': (label: 'Migration Preview…', description: null),
    'project.new': (label: 'New Project…', description: null),
    'project.open': (label: 'Open Project…', description: null),
    'project.revalidate': (label: 'Validate Project', description: null),
    'project.reveal': (label: 'Reveal in Finder', description: null),
    'scene.alignCameraToView': (
      label: 'Align Selected Camera to View',
      description: 'The selected camera entity moves to where the view is, one transaction',
    ),
    'scene.showAll': (label: 'Show All Hidden', description: null),
    'scene.toggleHidden': (
      label: 'Hide / Show Selected',
      description: 'Editor-only: out of the picture and of picking, never in the scene file',
    ),
    'scene.toggleLocked': (
      label: 'Lock / Unlock Selected',
      description: 'Editor-only: a locked entity is not picked or dragged in the Scene view',
    ),
    'scene.toggleSnap': (
      label: 'Toggle Snapping',
      description: 'Move, rotate and scale in steps (hold ⌘ during a drag to invert)',
    ),
    'scene.toggleSpace': (
      label: 'Toggle Local / World Axes',
      description: 'The gizmo\'s axes: the entity\'s own or the world\'s',
    ),
    'view.alignToCamera': (
      label: 'Look Through Selected Camera',
      description: 'The view takes the selected camera\'s placement and lens — the picture is what it renders',
    ),
    'view.frameAll': (label: 'Frame All', description: null),
    'view.frameSelected': (
      label: 'Frame Selected',
      description: 'The camera swings to fit the selection (everything when nothing is selected)',
    ),
    'view.recook': (
      label: 'Rebuild Asset Cache and Reload',
      description: '`fynkel cook` in process, then every preview reloaded',
    ),
    'view.reload': (
      label: 'Reload Preview',
      description:
          'The preview world emptied and loaded fresh from the document (the fallback the sync uses on its own)',
    ),
    'view.reset': (label: 'Reset View', description: 'The editor camera back to its starting place'),
    'view.saveFrame': (
      label: 'Save Frame…',
      description:
          'One frame of the Scene view as a PNG at the viewport\'s size (the pixel gate against `fynkel render`)',
    ),
    'view.toggleGrid': (label: 'Toggle Grid and Axes', description: null),
    'view.toggleOverlays': (label: 'Toggle Light and Camera Icons', description: null),
    'view.toggleProjection': (label: 'Toggle Perspective / Orthographic', description: null),
    'view.toggleStats': (label: 'Toggle Stats Overlay', description: null),
    'window.commandPalette': (label: 'Command Palette…', description: null),
    'window.resetLayout': (label: 'Reset Layout', description: null),
  };

  /// The label of the command [id]; a command without one is a bug.
  static String label(String id) =>
      commands[id]?.label ?? (throw StateError('no label for the command $id in EditorStrings.commands'));

  static String? description(String id) => commands[id]?.description;

  // ── Generated commands ──
  static String toolCommand(String tool) => '$tool Tool';
  static String toolDescription(String tool, String? key) =>
      'The Scene view\'s ${tool.toLowerCase()} tool${key == null ? '' : ' (also $key from any panel)'}';
  static String togglePanel(String title) => 'Toggle $title';
  static String showOrHidePanel(String title, {required bool shown}) => '${shown ? 'Hide' : 'Show'} $title';
  static String newPanel(String name) => 'New $name';
  static String goToPanel(String name) => 'Go to $name';
  static String goToPanelDescription(String name) => 'Shows the $name and moves the keyboard into it';
  static String openRecent(String name) => 'Open Recent: $name';

  // ── Menus, in menu-bar order ──
  static const menuProject = 'Project';
  static const menuEdit = 'Edit';
  static const menuView = 'View';
  static const menuPlay = 'Play';
  static const menuWindow = 'Window';
  static const menuHelp = 'Help';

  // ── Panels: the tab's title, and the longer name where it is ambiguous ──
  static const panelHierarchy = 'Hierarchy';
  static const panelScene = 'Scene';
  static const panelSceneFull = 'Scene View';
  static const panelGame = 'Game';
  static const panelGameFull = 'Game View';
  static const panelInspector = 'Inspector';
  static const panelProject = 'Project';
  static const panelProjectFull = 'Project Browser';
  static const panelConsole = 'Console';
  static const panelProfiler = 'Profiler';
  static const panelAnimation = 'Animation';
}
