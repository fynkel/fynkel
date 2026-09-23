// A project's component specs by type (the Inspector's, and the Scene
// view's for declared gizmos, item 9h): a type's schema and, for a
// behaviour, its committed metadata, parsed once (`parseComponentSpec`).
// Cleared when the project rescans.
import '../project/editor_project.dart';
import 'field_spec.dart';

final class ComponentSpecs {
  ComponentSpecs(this.project);

  final EditorProject project;

  Map<String, ComponentSpec>? _specs;
  Map<String, TypeSchema>? _schemas;
  Map<String, Map<String, Object?>>? _metadata;

  /// Schemas or metadata may have changed on disk.
  void clear() {
    _specs = null;
    _schemas = null;
    _metadata = null;
  }

  Map<String, TypeSchema> get schemas => _schemas ??= {for (final s in project.typeSchemas()) s.type: s};

  /// The committed behaviour metadata, by type id.
  Map<String, Map<String, Object?>> get metadata => _metadata ??= {
    for (final b in project.behaviourMetadata()?['behaviours'] as List? ?? const [])
      if (b is Map) (b['id'] as String? ?? ''): Map<String, Object?>.from(b),
  };

  /// The spec of [type]: from its schema and, for a behaviour, the committed
  /// metadata; a type nobody knows gets an empty spec whose data shows as JSON.
  ComponentSpec of(String type) {
    final cache = _specs ??= {};
    return cache.putIfAbsent(type, () {
      final s = schemas[type];
      final meta = metadata[type];
      if (s == null && meta == null) {
        return ComponentSpec(
          type: type,
          fields: const [],
          isBehaviour: !type.startsWith('fynkel.'),
          missingSchema: true,
          additionalProperties: true,
        );
      }
      // A behaviour whose schema file is missing still has its committed metadata's schema.
      final schema = s?.schema ?? (meta?['schema'] is Map ? Map<String, Object?>.from(meta!['schema'] as Map) : null);
      return parseComponentSpec(
        type,
        schema,
        metadata: meta,
        allowed: s?.allowed ?? true,
        authored: s?.authored ?? true,
      );
    });
  }
}
