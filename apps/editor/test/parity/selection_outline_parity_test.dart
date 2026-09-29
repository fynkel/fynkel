// Unity parity: the selection outline (docs/unity-parity.md § Tier 3). The
// selected objects' meshes are ringed in orange by the renderer, what is
// under them in blue; the bounding boxes are gone, an object without a
// mesh keeps its cross. A selected prefab instance rings its own mesh,
// its parts in blue.
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/hierarchy/presets.dart';
import 'package:fynkel_editor/src/theme/tokens.dart';

import 'harness.dart';

const robot = 'c0000000-0000-4000-8000-0000000000e1';
const crateA = 'c0000000-0000-4000-8000-0000000000e2';
const camera = 'c0000000-0000-4000-8000-0000000000e0';

void main() {
  testWidgets('orange around the selection, blue around its children, no boxes; a cross for what draws no mesh', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final sv = e.shell.scene!;
    final tab = e.shell.tabs!.focused!;
    if (sv.showOverlays) sv.toggleOverlays();

    expect(sv.overlay(sv.current!).selectionOutline, isNull, reason: 'nothing selected, nothing ringed');

    await e.click(e.row('Robot'));
    var o = sv.overlay(sv.current!);
    expect(o.selectionOutline?.selected, isNotEmpty);
    expect(o.selectionOutline?.children, isEmpty);
    expect(o.selectionOutline?.selectedColor, GizmoTokens.outlineSelected);
    expect(o.selectionOutline?.childrenColor, GizmoTokens.outlineChildren);
    expect(o.lineCount, 0, reason: 'the mesh is ringed, not boxed');
    final robotHandles = o.selectionOutline!.selected;

    // A child under the Robot: blue while the Robot is selected, orange once it is.
    expect(e.shell.hierarchy!.createPreset(GameObjectPreset.cube, parent: robot), isTrue);
    await settleCooks(tester);
    final cube = tab.selection.single;
    await e.click(e.row('Robot'));
    o = sv.overlay(sv.current!);
    // (a sync may respawn the Robot under a new handle: the outline follows it)
    expect(o.selectionOutline?.selected, hasLength(robotHandles.length));
    expect(o.selectionOutline?.children, hasLength(1));
    final cubeHandle = o.selectionOutline!.children.single;
    tab.setSelection([cube]);
    o = sv.overlay(sv.current!);
    expect(o.selectionOutline?.selected, [cubeHandle]);
    expect(o.selectionOutline?.children, isEmpty);

    // A selected instance: its root's own mesh orange, its parts blue — each once, apart.
    tab.setSelection([crateA]);
    o = sv.overlay(sv.current!);
    final crate = o.selectionOutline!;
    expect(crate.selected.toSet().intersection(crate.children.toSet()), isEmpty);
    expect([...crate.selected, ...crate.children], isNotEmpty);

    // The Camera draws no mesh: its cross stays.
    tab.setSelection([camera]);
    o = sv.overlay(sv.current!);
    expect(o.lineCount, greaterThan(0));
  });
}
