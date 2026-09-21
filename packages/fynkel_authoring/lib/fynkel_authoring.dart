/// Fynkel Engine — the editor's Dart surface over the authoring ABI
/// (`crates/fynkel_authoring` through `fynkel_native`'s `authoring` feature,
/// `include/fynkel_authoring.h`): an [AuthoringSession] over one project,
/// a [DocumentHandle] per open scene, prefab or material, typed [Command]
/// builders, and the ADR-0014 [Diagnostic] record.
///
/// Games never depend on this package; the runtime an app builds carries
/// the authoring entry points only when its pubspec lists the `authoring`
/// feature under `hooks.user_defines.fynkel.features` (blueprint §19.2e).
library;

export 'src/commands.dart';
export 'src/diagnostic.dart';
export 'src/models.dart';
export 'src/session.dart';
