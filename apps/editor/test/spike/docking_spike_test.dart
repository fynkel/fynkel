// The docking spike (blueprint §19.2e item 2; ADR-0016 "Resolutions at the
// start of M4" → "Docking package").
//
// One test per written criterion, against `package:docking` 1.16.2 as it
// resolves for Flutter 3.47.2. Each test is the evidence behind the pick
// recorded in the ADR; a failing criterion here is what would have sent the
// shell to the `multi_split_view` fallback. Findings the shell has to
// design around are asserted too (search "finding").
import 'package:docking/docking.dart';
import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

/// A stateful panel that counts taps — the stand-in for a panel with state
/// (a scroll position, a selection, an expanded tree).
class Counter extends StatefulWidget {
  const Counter({super.key, required this.label});

  final String label;

  @override
  State<Counter> createState() => CounterState();
}

class CounterState extends State<Counter> {
  int count = 0;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: TextButton(onPressed: () => setState(() => count++), child: Text('${widget.label}:$count')),
    );
  }
}

/// The stand-in for the Scene view: a widget whose state owns a live
/// platform texture. Disposing it would tear the surface down (M4 item 6
/// binds the edit-preview world's IOSurface to it), so it must be the same
/// State — and the same render object — before and after a re-dock.
class LiveTexture extends StatefulWidget {
  const LiveTexture({super.key, required this.textureId});

  final int textureId;

  @override
  State<LiveTexture> createState() => LiveTextureState();
}

class LiveTextureState extends State<LiveTexture> {
  static int created = 0;
  static int disposed = 0;

  @override
  void initState() {
    super.initState();
    created++;
  }

  @override
  void dispose() {
    disposed++;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Texture(textureId: widget.textureId);
}

/// Builds items by id when a serialised layout is loaded — the shell will do
/// the same from its panel registry.
class PanelBuilder with AreaBuilderMixin {
  PanelBuilder(this.panels);

  final Map<String, Widget> panels;

  @override
  DockingItem buildDockingItem({required dynamic id, required double? weight, required bool maximized}) {
    return DockingItem(
      id: id,
      name: id as String,
      keepAlive: true,
      widget: panels[id] ?? Center(child: Text('missing $id')),
      weight: weight,
      maximized: maximized,
    );
  }
}

class PanelParser extends LayoutParser with LayoutParserMixin {
  const PanelParser();
}

DockingItem panel(String id, Widget child, {double? weight, bool keepAlive = true}) {
  return DockingItem(id: id, name: id, widget: child, weight: weight, keepAlive: keepAlive);
}

Widget host(Widget child, {double width = 1200, double height = 800}) {
  return MaterialApp(
    home: Scaffold(
      body: TabbedViewTheme(
        data: TabbedViewThemeData.classic(),
        child: SizedBox(width: width, height: height, child: child),
      ),
    ),
  );
}

Future<void> pumpDocking(WidgetTester tester, DockingLayout layout, {Widget Function(Widget)? wrap}) async {
  tester.view.physicalSize = const Size(1200, 800);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  final docking = Docking(layout: layout);
  await tester.pumpWidget(host(wrap == null ? docking : wrap(docking)));
  await tester.pumpAndSettle();
}

/// Drags the tab titled [tab] and drops it at [target] with a real pointer.
Future<void> dragTabTo(WidgetTester tester, String tab, Offset target) async {
  final start = tester.getCenter(find.text(tab));
  final gesture = await tester.startGesture(start);
  // Past the touch slop so the Draggable starts, then to the target in steps
  // so the DragTarget under the pointer sees the drag before the drop.
  await gesture.moveBy(const Offset(30, 0));
  await tester.pump();
  const steps = 8;
  for (var i = 1; i <= steps; i++) {
    await gesture.moveTo(Offset.lerp(start + const Offset(30, 0), target, i / steps)!);
    await tester.pump();
  }
  await gesture.up();
  await tester.pumpAndSettle();
}

/// The rectangle a panel occupies: its TabbedView (strip + content).
Rect panelRect(WidgetTester tester, String bodyText) {
  return tester.getRect(find.ancestor(of: find.text(bodyText, skipOffstage: false), matching: find.byType(TabbedView)));
}

void main() {
  testWidgets('criterion 1: tabs — a tabbed group shows its tabs, switches on click, reports selection', (
    tester,
  ) async {
    final selected = <String>[];
    final tabs = DockingTabs([
      panel('Hierarchy', const Text('hierarchy body')),
      panel('Console', const Text('console body')),
    ]);
    final layout = DockingLayout(root: tabs);
    await pumpDocking(
      tester,
      layout,
      wrap: (d) => Docking(layout: layout, onItemSelection: (item) => selected.add(item.id as String)),
    );

    expect(find.text('Hierarchy'), findsOneWidget);
    expect(find.text('Console'), findsOneWidget);
    expect(find.text('hierarchy body'), findsOneWidget);
    // keepAlive content is built offstage; the visible body is the selected one.
    expect(find.text('console body', skipOffstage: true), findsNothing);
    expect(tabs.selectedIndex, 0);

    await tester.tap(find.text('Console'));
    await tester.pumpAndSettle();
    expect(find.text('console body'), findsOneWidget);
    expect(find.text('hierarchy body', skipOffstage: true), findsNothing);
    expect(tabs.selectedIndex, 1);
    expect(selected, ['Console']);
  });

  testWidgets('criterion 2: drag to re-dock — a real pointer drag of a tab onto another panel splits and re-tabs', (
    tester,
  ) async {
    final layout = DockingLayout(
      root: DockingRow([
        panel('Hierarchy', const Text('hierarchy body'), weight: .5),
        DockingTabs([
          panel('Console', const Text('console body')),
          panel('Profiler', const Text('profiler body')),
        ], weight: .5),
      ]),
    );
    await pumpDocking(tester, layout);
    expect(layout.hierarchy(nameInfo: true), 'R(IHierarchy,T(IConsole,IProfiler))');

    // Drop Profiler on the bottom half of Hierarchy: a vertical split appears
    // and the tab group collapses to the one remaining item.
    final hierarchyBody = tester.getRect(find.text('hierarchy body'));
    await dragTabTo(tester, 'Profiler', Offset(hierarchyBody.center.dx, 700));
    expect(layout.hierarchy(nameInfo: true), 'R(C(IHierarchy,IProfiler),IConsole)');
    expect(find.text('profiler body'), findsOneWidget);

    // Drop Console onto Profiler's tab: it joins the group, inserted before
    // the tab it was dropped on.
    final profilerTab = tester.getCenter(find.text('Profiler'));
    await dragTabTo(tester, 'Console', profilerTab + const Offset(60, 0));
    expect(layout.hierarchy(nameInfo: true), 'C(IHierarchy,T(IConsole,IProfiler))');

    // Programmatic moves use the same modifier the drop uses.
    layout.moveItem(
      draggedItem: layout.findDockingItem('Hierarchy')!,
      targetArea: layout.findDockingTabsWithItem('Console')!,
      dropPosition: DropPosition.right,
    );
    await tester.pumpAndSettle();
    expect(layout.hierarchy(nameInfo: true), 'R(T(IConsole,IProfiler),IHierarchy)');
  });

  testWidgets('criterion 3: splits — rows and columns honour weights and a divider drag resizes them', (tester) async {
    final hierarchy = panel('Hierarchy', const Text('hierarchy body'), weight: .3);
    final scene = panel('Scene', const Text('scene body'), weight: .7);
    final row = DockingRow([hierarchy, scene]);
    final layout = DockingLayout(root: row);
    await pumpDocking(tester, layout);

    const dividerThickness = MultiSplitViewThemeData.defaultDividerThickness;
    final usable = 1200 - dividerThickness;
    final hierarchyBox = panelRect(tester, 'hierarchy body');
    final sceneBox = panelRect(tester, 'scene body');
    expect(hierarchyBox.width, closeTo(usable * .3, 1));
    expect(sceneBox.width, closeTo(usable * .7, 1));

    // Drag the divider 150 px to the right.
    final dividerX = hierarchyBox.right + dividerThickness / 2;
    await tester.dragFrom(Offset(dividerX, 400), const Offset(150, 0));
    await tester.pumpAndSettle();
    expect(panelRect(tester, 'hierarchy body').width, closeTo(hierarchyBox.width + 150, 1));
    expect(panelRect(tester, 'scene body').width, closeTo(sceneBox.width - 150, 1));
    // The new weights live on the areas, so they serialise (criterion 6).
    expect(hierarchy.weight, closeTo((hierarchyBox.width + 150) / usable, .01));
    expect(scene.weight, closeTo((sceneBox.width - 150) / usable, .01));

    // A column splits vertically the same way.
    final column = DockingColumn([
      panel('Scene2', const Text('scene body'), weight: .75),
      panel('Console', const Text('console body'), weight: .25),
    ]);
    final layout2 = DockingLayout(root: column);
    await pumpDocking(tester, layout2);
    expect(panelRect(tester, 'console body').height, closeTo((800 - dividerThickness) * .25, 1));
  });

  testWidgets('criterion 4: hidden tabs keep state — keepAlive keeps a panel\'s State while another tab is shown', (
    tester,
  ) async {
    final tabs = DockingTabs([
      panel('Kept', const Counter(label: 'kept')),
      panel('Lost', const Counter(label: 'lost'), keepAlive: false),
      panel('Other', const Text('other body')),
    ]);
    final layout = DockingLayout(root: tabs);
    await pumpDocking(tester, layout);

    await tester.tap(find.text('kept:0'));
    await tester.pumpAndSettle();
    expect(find.text('kept:1'), findsOneWidget);
    final keptState = tester.state<CounterState>(find.byType(Counter));

    await tester.tap(find.text('Lost'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('lost:0'));
    await tester.pumpAndSettle();
    expect(find.text('lost:1'), findsOneWidget);

    await tester.tap(find.text('Other'));
    await tester.pumpAndSettle();
    expect(find.text('other body'), findsOneWidget);
    // The kept panel is still in the tree, offstage, with its state.
    expect(find.text('kept:1', skipOffstage: false), findsOneWidget);
    expect(find.text('lost:1', skipOffstage: false), findsNothing);

    await tester.tap(find.text('Kept'));
    await tester.pumpAndSettle();
    expect(find.text('kept:1'), findsOneWidget);
    expect(tester.state<CounterState>(find.widgetWithText(Counter, 'kept:1')), same(keptState));

    // Finding: without keepAlive the State is rebuilt from scratch — every
    // editor panel is registered with keepAlive: true.
    await tester.tap(find.text('Lost'));
    await tester.pumpAndSettle();
    expect(find.text('lost:0'), findsOneWidget);
  });

  testWidgets('criterion 5: a live Texture survives moving between tabs and splits — same State, same render object', (
    tester,
  ) async {
    LiveTextureState.created = 0;
    LiveTextureState.disposed = 0;
    final scene = panel('Scene', const LiveTexture(textureId: 42), weight: .6);
    final layout = DockingLayout(
      root: DockingRow([
        scene,
        DockingTabs([
          panel('Inspector', const Text('inspector body')),
          panel('Console', const Text('console body')),
        ], weight: .4),
      ]),
    );
    await pumpDocking(tester, layout);
    expect(LiveTextureState.created, 1);
    final state = tester.state<LiveTextureState>(find.byType(LiveTexture));
    final box = tester.renderObject<TextureBox>(find.byType(Texture));
    expect(box.textureId, 42);

    // Into the tab group, as a tab (the drop's tab-strip path).
    layout.moveItem(draggedItem: scene, targetArea: layout.findDockingTabsWithItem('Inspector')!, dropIndex: 2);
    await tester.pumpAndSettle();
    expect(layout.hierarchy(nameInfo: true), 'T(IInspector,IConsole,IScene)');
    // Select it so it is on stage, then check identity.
    await tester.tap(find.text('Scene'));
    await tester.pumpAndSettle();
    expect(tester.state<LiveTextureState>(find.byType(LiveTexture)), same(state));
    expect(tester.renderObject<TextureBox>(find.byType(Texture)), same(box));

    // Hidden behind another tab: still alive, not disposed.
    await tester.tap(find.text('Console'));
    await tester.pumpAndSettle();
    expect(find.byType(Texture, skipOffstage: false), findsOneWidget);
    expect(LiveTextureState.disposed, 0);

    // Out again into a split under the group, with a real pointer drag.
    final consoleBody = tester.getRect(find.text('console body'));
    await dragTabTo(tester, 'Scene', Offset(consoleBody.center.dx, 750));
    expect(layout.hierarchy(nameInfo: true), 'C(T(IInspector,IConsole),IScene)');
    expect(tester.state<LiveTextureState>(find.byType(LiveTexture)), same(state));
    expect(tester.renderObject<TextureBox>(find.byType(Texture)), same(box));

    // Maximise and restore around it.
    layout.maximizeDockingItem(scene);
    await tester.pumpAndSettle();
    expect(tester.state<LiveTextureState>(find.byType(LiveTexture)), same(state));
    layout.restore();
    await tester.pumpAndSettle();
    expect(tester.state<LiveTextureState>(find.byType(LiveTexture)), same(state));

    expect(LiveTextureState.created, 1);
    expect(LiveTextureState.disposed, 0);

    // Finding: an item without keepAlive is a fresh subtree after a move —
    // the Scene view must be a keepAlive item.
    LiveTextureState.created = 0;
    final plain = panel('Plain', const LiveTexture(textureId: 7), keepAlive: false);
    final other = panel('Other', const Text('other'));
    final layout2 = DockingLayout(root: DockingRow([plain, other]));
    await pumpDocking(tester, layout2); // replaces the tree: the first Scene is disposed here
    expect(LiveTextureState.created, 1);
    LiveTextureState.disposed = 0;
    layout2.moveItem(draggedItem: plain, targetArea: other, dropPosition: DropPosition.bottom);
    await tester.pumpAndSettle();
    expect(LiveTextureState.created, 2);
    expect(LiveTextureState.disposed, 1);
  });

  testWidgets(
    'criterion 6: layout serialisation — the §22.2 workspace round-trips through a string, weights included',
    (tester) async {
      final panels = <String, Widget>{
        'Hierarchy': const Text('hierarchy body'),
        'Scene': const Text('scene body'),
        'Inspector': const Text('inspector body'),
        'Project': const Text('project body'),
        'Console': const Text('console body'),
        'Profiler': const Text('profiler body'),
        'Animation': const Text('animation body'),
        'Import': const Text('import body'),
      };
      final builder = PanelBuilder(panels);
      DockingItem item(String id, {double? weight}) =>
          builder.buildDockingItem(id: id, weight: weight, maximized: false);

      final bottom = DockingTabs([
        item('Project'),
        item('Console'),
        item('Profiler'),
        item('Animation'),
        item('Import'),
      ], weight: .3);
      final workspace = DockingRow([
        item('Hierarchy', weight: .2),
        DockingColumn([item('Scene', weight: .7), bottom], weight: .6),
        item('Inspector', weight: .2),
      ]);
      final layout = DockingLayout(root: workspace);
      await pumpDocking(tester, layout);
      const parser = PanelParser();

      final saved = layout.stringify(parser: parser);
      expect(saved, startsWith('V1:'));
      expect(
        saved,
        'V1:11:1(R;0;;;2,3,11),2(I;9;Hierarchy;0.2;F),3(C;0;;0.6;4,5),4(I;5;Scene;0.7;F),'
        '5(T;0;;0.3;F;6,7,8,9,10),6(I;7;Project;;F),7(I;7;Console;;F),8(I;8;Profiler;;F),'
        '9(I;9;Animation;;F),10(I;6;Import;;F),11(I;9;Inspector;0.2;F)',
      );

      // Load into a fresh layout: same shape, same weights, same names.
      final restored = DockingLayout();
      restored.load(layout: saved, parser: parser, builder: PanelBuilder(panels));
      expect(restored.hierarchy(nameInfo: true), layout.hierarchy(nameInfo: true));
      expect(restored.stringify(parser: parser), saved);
      await pumpDocking(tester, restored);
      expect(find.text('scene body'), findsOneWidget);
      expect(find.text('project body'), findsOneWidget);

      // A divider drag changes weights; the string carries the new ones.
      final sceneBox = panelRect(tester, 'scene body');
      await tester.dragFrom(Offset(sceneBox.center.dx, sceneBox.bottom + 5), const Offset(0, -100));
      await tester.pumpAndSettle();
      final resized = restored.stringify(parser: parser);
      expect(resized, isNot(saved));
      final again = DockingLayout();
      again.load(layout: resized, parser: parser, builder: PanelBuilder(panels));
      expect(again.stringify(parser: parser), resized);
      expect(again.findDockingItem('Scene')!.weight, closeTo(restored.findDockingItem('Scene')!.weight!, 1e-9));

      // Maximised state is carried; the layout restores with it.
      restored.maximizeDockingItem(restored.findDockingItem('Console')!);
      final maximised = DockingLayout();
      maximised.load(
        layout: restored.stringify(parser: parser),
        parser: parser,
        builder: PanelBuilder(panels),
      );
      expect(maximised.maximizedArea?.id, 'Console');

      // Finding: the selected tab of a group is NOT in the string. Selecting
      // Import leaves it unchanged, so the shell stores selected ids beside it.
      restored.restore();
      final before = restored.stringify(parser: parser);
      restored.findDockingTabsWithItem('Import')!.selectedIndex = 4;
      expect(restored.stringify(parser: parser), before);

      // Finding: a string for a panel the registry no longer knows still loads
      // (the builder decides what to show) and a corrupt string throws.
      final partial = DockingLayout();
      partial.load(layout: saved, parser: parser, builder: PanelBuilder({}));
      expect(partial.layoutAreas().length, 11);
      expect(() => DockingLayout().load(layout: 'V2:nope', parser: parser, builder: builder), throwsStateError);
      expect(() => DockingLayout().load(layout: 'garbage', parser: parser, builder: builder), throwsStateError);
    },
  );

  testWidgets(
    'criterion 7: keyboard focus — hidden tabs cannot hold focus, focus and app shortcuts survive a re-dock',
    (tester) async {
      final nodeA = FocusNode(debugLabel: 'A');
      final nodeB = FocusNode(debugLabel: 'B');
      addTearDown(nodeA.dispose);
      addTearDown(nodeB.dispose);
      var paletteOpened = 0;
      final fieldA = panel('Hierarchy', TextField(key: const Key('fieldA'), focusNode: nodeA));
      final fieldB = panel('Inspector', TextField(key: const Key('fieldB'), focusNode: nodeB));
      final tabs = DockingTabs([fieldA, fieldB]);
      final layout = DockingLayout(root: DockingRow([panel('Scene', const Text('scene body')), tabs]));
      await pumpDocking(
        tester,
        layout,
        wrap: (docking) => CallbackShortcuts(
          bindings: {const SingleActivator(LogicalKeyboardKey.keyP, meta: true, shift: true): () => paletteOpened++},
          child: Focus(autofocus: true, child: docking),
        ),
      );

      await tester.tap(find.byKey(const Key('fieldA')));
      await tester.pumpAndSettle();
      expect(nodeA.hasFocus, isTrue);
      await tester.enterText(find.byKey(const Key('fieldA')), 'robot');
      expect(find.text('robot'), findsOneWidget);

      // The app-level shortcut fires while a text field has focus.
      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyP);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      expect(paletteOpened, 1);

      // Showing the other tab drops focus from the hidden field, and Tab
      // traversal never reaches it.
      await tester.tap(find.text('Inspector'));
      await tester.pumpAndSettle();
      expect(nodeA.hasFocus, isFalse);
      expect(nodeA.canRequestFocus, isFalse);
      for (var i = 0; i < 4; i++) {
        await tester.sendKeyEvent(LogicalKeyboardKey.tab);
        await tester.pumpAndSettle();
        expect(nodeA.hasFocus, isFalse);
      }
      await tester.tap(find.byKey(const Key('fieldB')));
      await tester.pumpAndSettle();
      expect(nodeB.hasFocus, isTrue);

      // Re-dock the focused panel with a real drag: the field keeps its text
      // and can be focused again; the shortcut still reaches the app.
      final sceneBody = tester.getRect(find.text('scene body'));
      await dragTabTo(tester, 'Inspector', Offset(sceneBody.center.dx, 750));
      expect(layout.hierarchy(nameInfo: true), 'R(C(IScene,IInspector),IHierarchy)');
      await tester.tap(find.byKey(const Key('fieldB')));
      await tester.pumpAndSettle();
      expect(nodeB.hasFocus, isTrue);
      await tester.enterText(find.byKey(const Key('fieldB')), 'crate');
      expect(find.text('crate'), findsOneWidget);
      await tester.tap(find.byKey(const Key('fieldA')));
      await tester.pumpAndSettle();
      expect(nodeA.hasFocus, isTrue);
      expect(find.text('robot'), findsOneWidget);

      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyP);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      expect(paletteOpened, 2);

      // Finding: the drag itself does not move keyboard focus — whoever had it
      // before the drop still has it (the shell need not restore focus).
      expect(nodeA.hasFocus, isTrue);
    },
  );
}
