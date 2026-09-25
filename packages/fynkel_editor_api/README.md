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

The rules:

- Only files under `lib/editor/` import this package or `lib/editor/`
  (`FYN-P0015`, reported by `fynkel validate` and `fynkel codegen
  --check`), so a game never ships editor code.
- An extension changes how things are shown and adds tools; everything
  stays editable without it.
- A failing extension fails alone: its error goes to the Console with the
  package's name and the editor carries on.

See `docs/editor.md` ("Editor extensions").
