# package:fynkel_editor_api

What a package's editor part registers with the Fynkel editor
(ADR-0028). Any package can have one, the game included: its code lives
under `lib/editor/`, and `lib/editor/editor.dart` defines

```dart
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

void registerEditor(EditorExtensions editor) {
  editor.menuItem(
    'Tools/Say Hello',
    id: 'game.sayHello',
    run: (context) => context.log('Hello from ${context.package}'),
  );
}
```

`fynkel editor <dir>` finds every package in the project's package graph
that has `lib/editor/editor.dart` and calls its `registerEditor`:
dependencies first, then alphabetically, the game last. Every hot reload
calls them again, so a save in `lib/editor/` shows at once. Preferences ›
Extensions lists what each package registered.

A callback's `EditorContext` reads the scene in front (`scene`,
`selection`) and the project's assets (`assets`), and changes them as one
undo step:

```dart
editor.menuItem(
  'Tools/Level/Number Coins',
  id: 'game.numberCoins',
  run: (context) {
    final coins = [for (final o in context.scene!.objects) if (o.has('game.Collectible')) o];
    context.edit('Number coins', (edit) {
      for (final (i, coin) in coins.indexed) {
        edit.rename(coin, 'Coin ${i + 1}');
      }
    });
  },
);
```

`editor.window(...)` adds a dockable window whose `build` gets the
context and runs again when the scene, the selection or the assets
change. `hierarchyMenuItem`, `projectMenuItem` and `componentMenuItem`
add entries to the Hierarchy's, the Project panel's and a component
card's context menus.

`editor.inspector<T>(build)` replaces the Inspector's card of the script
`T`, or the page of the data type `T` (Unity's `CustomEditor`). Its
`InspectorContext` reads and writes the document's values (`value`,
`set`) and draws the standard editors (`field`, `defaultInspector`);
every write is one undo step, a prefab override where it should be, and
reaches the running game during Play:

```dart
editor.inspector<CoinKind>((context, inspector) => Column(
  children: [
    CoinPreview(color: inspector.value('color'), size: inspector.value('size')),
    inspector.defaultInspector(),
  ],
));
```

The rules:

- Only files under `lib/editor/` import this package or `lib/editor/`
  (`FYN-P0015`, reported by `fynkel validate` and `fynkel codegen
  --check`), so a game never ships editor code.
- An extension changes how things are shown and adds tools; everything
  stays editable without it.
- A failing extension fails alone: its error goes to the Console with the
  package's name and the editor carries on.

See `docs/editor.md` ("Editor extensions").
