/// What a package's editor part registers with the Fynkel editor
/// (ADR-0028). A package's editor code lives under `lib/editor/`; its
/// entry, `lib/editor/editor.dart`, defines
///
/// ```dart
/// import 'package:fynkel_editor_api/fynkel_editor_api.dart';
///
/// void registerEditor(EditorExtensions editor) {
///   editor.menuItem('Tools/Say Hello', id: 'game.sayHello', run: (context) => context.log('Hello'));
/// }
/// ```
///
/// `fynkel editor` finds it and calls it; so does every hot reload. Only
/// files under `lib/editor/` may import this library (`FYN-P0015`), so a
/// game never ships editor code.
library;

export 'src/context.dart';
export 'src/extensions.dart';
