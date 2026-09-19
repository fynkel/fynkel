/// Fynkel Engine — the behaviour generator (ADR-0012 pipeline (b)).
///
/// A `build_runner` aggregate builder that reads every `@FynkelBehaviour`
/// class in a package and emits, deterministically:
///
/// - `lib/generated/behaviours.g.dart` — one `BehaviourType` per class
///   (factory, `@Expose` field getters/setters over JSON values, Inspector
///   metadata), `behaviourTypes` and `registerBehaviours(registry)`;
/// - `lib/generated/behaviours.g.json` — the same metadata as data plus a
///   JSON Schema 2020-12 for each type's authored `data`, which
///   `fynkel codegen` splits into `schemas/behaviours/<id>.schema.json`.
///
/// Nothing is discovered at runtime and nothing uses mirrors: the generated
/// file is ordinary Dart the game registers explicitly. Errors (a class that
/// is not a `Behaviour`, a duplicate id, an unsupported field type) fail the
/// build with the file and member named.
library;

import 'package:build/build.dart';

import 'src/builder.dart';

export 'src/builder.dart' show BehaviourBuilder;
export 'src/emit.dart' show emitDart, emitJson, identifierFor, schemaFor, schemaIdFor;
export 'src/model.dart';

/// The builder factory `build.yaml` names.
Builder behaviourBuilder(BuilderOptions options) => BehaviourBuilder(options);
