// The project's layers and tags (ADR-0020): every object has one layer
// (`fynkel.Layer`, absent = `default`) and a set of tags (`fynkel.Tags`,
// bit `i` = `tags[i]`). The names live in `fynkel.project.json`; the load
// report carries them (`LoadReport.layersAndTags`).

/// The project's `layers` (index `i` is bit `i` of a mask, `default` first,
/// at most 32) and `tags` (bit `i` of an object's tag set, at most 32).
class LayersAndTags {
  const LayersAndTags({this.layers = const ['default'], this.tags = const []});

  /// Reads the top-level `layers` and `tags` of a load report or project
  /// file; either may be absent.
  factory LayersAndTags.fromJson(Map<String, Object?> j) => LayersAndTags(
        layers: [for (final l in (j['layers'] as List?) ?? const ['default']) l as String],
        tags: [for (final t in (j['tags'] as List?) ?? const []) t as String],
      );

  final List<String> layers;
  final List<String> tags;

  /// The bit mask of the named layers, or null when a name is unknown.
  int? mask(Iterable<String> names) => _bits(layers, names);

  /// The index of the named layer, or null when the project lacks it.
  int? layer(String name) {
    final i = layers.indexOf(name);
    return i < 0 ? null : i;
  }

  /// The bit set of the named tags, or null when a name is unknown.
  int? tagBits(Iterable<String> names) => _bits(tags, names);

  static int? _bits(List<String> list, Iterable<String> names) {
    var bits = 0;
    for (final n in names) {
      final i = list.indexOf(n);
      if (i < 0) return null;
      bits |= 1 << i;
    }
    return bits;
  }
}
