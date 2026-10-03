// Scenes and prefabs written from code (ADR-0026 §2): the `ObjectSpec`
// `World.build` spawns, written as the `.scene.json` / `.prefab.json`
// documents the editor opens and the cook reads. Pure Dart — no native
// library — for a tool (`dart run tool/scenes.dart`), never for the game.

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:crypto/crypto.dart';

import '../../advanced.dart';
import '../../generated/schema.g.dart' as gen;
import '../wire/support.dart' show docAssetRef, docQuat, docVec3, docVec4;
import '../world.dart' show bindWrittenData, specOfObject, writingReferences;

/// A scene written from code: its objects, in order.
final class SceneSpec {
  const SceneSpec(this.name, this.objects);

  final String name;
  final List<ObjectSpec> objects;
}

/// What [ProjectWriter.writeScene] / [ProjectWriter.writePrefab] did.
final class WrittenDocument {
  const WrittenDocument(this.ref, this.path, {required this.changed, required this.kept});

  /// The document's asset (its id is kept across writes).
  final AssetRef ref;

  /// The file, under the project's `assets/`.
  final String path;

  /// False when the file already held these bytes (it was not touched).
  final bool changed;

  /// Objects the document had that code did not make (added in the
  /// editor), kept as they were, by name.
  final List<String> kept;
}

/// Writes scene and prefab documents into the project at [projectDir]
/// (the folder with `fynkel.project.json`) from [ObjectSpec]s — or into a
/// Fynkel package (the folder with `fynkel.package.json`, ADR-0029): its
/// `assets` folder, its ids derived apart from any game's, and no layers
/// or tags but `default` (the game's are not the package's to know).
///
/// Every id is derived — the document's from its path (or kept from the
/// file already there), an object's from the path of names down to it, a
/// component's from its object and type — so writing the same specs again
/// gives the same bytes. The objects code makes belong to code: writing
/// again replaces them, edits in the editor included. Objects added in the
/// editor (their ids are not derived) are kept, after the code's, under
/// the same parent.
///
/// Scripts are written through their registered fields: pass the
/// generated `registerBehaviours` as [registerBehaviours]. A prefab
/// instance of an imported model (`AssetRef(glb, sub: 'prefab')`) needs
/// the project cooked (`fynkel cook`), since its ids come from the model.
class ProjectWriter {
  ProjectWriter(String projectDir, {void Function(BehaviourRegistry registry)? registerBehaviours, String? cacheDir})
      : projectDir = Directory(projectDir).absolute.path,
        registry = BehaviourRegistry() {
    registerBehaviours?.call(registry);
    _cacheDir = cacheDir ?? '${this.projectDir}/.fynkel/cache';
    final project = File('${this.projectDir}/fynkel.project.json');
    final package = File('${this.projectDir}/fynkel.package.json');
    if (project.existsSync()) {
      final json = jsonDecode(project.readAsStringSync()) as Map<String, Object?>;
      layersAndTags = LayersAndTags.fromJson(json);
      assetsDir = '${this.projectDir}/${json['assets'] as String? ?? 'assets'}';
      _ids = _namespace;
    } else if (package.existsSync()) {
      final json = jsonDecode(package.readAsStringSync()) as Map<String, Object?>;
      layersAndTags = LayersAndTags.fromJson(const {});
      assetsDir = '${this.projectDir}/${json['assets'] as String? ?? 'assets'}';
      final name = json['namespace'] as String? ?? _pubspecName(this.projectDir);
      _ids = _uuid5(_namespace, 'package/$name');
    } else {
      throw ArgumentError.value(projectDir, 'projectDir', 'no fynkel.project.json or fynkel.package.json');
    }
  }

  /// The namespace of the ids this writer derives: the project's, or one
  /// per package so a package's `materials/x` is not the game's.
  late final String _ids;

  static String _pubspecName(String dir) {
    final pubspec = File('$dir/pubspec.yaml');
    final m = pubspec.existsSync() ? RegExp(r'^name:\s*(\w+)', multiLine: true).firstMatch(pubspec.readAsStringSync()) : null;
    if (m == null) throw ArgumentError.value(dir, 'projectDir', 'a package without a namespace needs a pubspec.yaml name');
    return m.group(1)!;
  }

  /// The asset folders of the Fynkel packages the project resolves
  /// (`.dart_tool/package_config.json`, ADR-0029): where a prefab instance
  /// of a package's prefab finds its root.
  List<String> _packageAssetDirs() {
    final config = File('$projectDir/.dart_tool/package_config.json');
    if (!config.existsSync()) return const [];
    final out = <String>[];
    final packages = (jsonDecode(config.readAsStringSync()) as Map<String, Object?>)['packages'] as List? ?? const [];
    for (final p in packages.cast<Map<String, Object?>>()) {
      final root = config.uri.resolve(p['rootUri'] as String? ?? '');
      final dir = Directory.fromUri(root).path.replaceFirst(RegExp(r'/$'), '');
      if (dir == projectDir) continue;
      final manifest = File('$dir/fynkel.package.json');
      if (!manifest.existsSync()) continue;
      final json = jsonDecode(manifest.readAsStringSync()) as Map<String, Object?>;
      final assets = Directory('$dir/${json['assets'] as String? ?? 'assets'}');
      if (assets.existsSync()) out.add(assets.path);
    }
    return out;
  }

  final String projectDir;
  late final String assetsDir;
  late final String _cacheDir;
  late final LayersAndTags layersAndTags;
  final BehaviourRegistry registry;

  /// Writes [scene] to `assets/<path>` (`.scene.json` added when missing).
  WrittenDocument writeScene(String path, SceneSpec scene) {
    path = _withExtension(path, '.scene.json');
    return _write(path, 'scene', scene.name, scene.objects, prefab: false);
  }

  /// Writes [root] as a prefab to `assets/<path>` (`.prefab.json` added
  /// when missing); the prefab is named after the root.
  WrittenDocument writePrefab(String path, ObjectSpec root) {
    path = _withExtension(path, '.prefab.json');
    final name = root.name;
    if (name == null) throw ArgumentError.value(root, 'root', 'a prefab needs a name for its root');
    return _write(path, 'prefab', name, [root], prefab: true);
  }

  /// Writes [material] to `assets/<path>` (`.material.json` added when
  /// missing) as a `fynkel.pbr@1` material named [name] (the file's name
  /// by default): what `world.createMaterial` makes at runtime, as an
  /// asset a written scene's `MeshRenderer` can name.
  WrittenDocument writeMaterial(String path, gen.PbrMaterial material, {String? name}) {
    path = _withExtension(path, '.material.json');
    final file = File('$assetsDir/$path');
    final existing = file.existsSync() ? jsonDecode(file.readAsStringSync()) as Map<String, Object?> : null;
    final id = existing?['id'] as String? ?? _uuid5(_ids, 'document/$path');
    final out = <String, Object?>{
      r'$schema': 'https://fynkel.dev/schemas/material.schema.json',
      'formatVersion': 2,
      'id': id,
      'name': name ?? path.split('/').last.replaceFirst('.material.json', ''),
      'shader': 'fynkel.pbr@1',
      ...material.toJson(),
    };
    return _writeText(file, path, id, out, const []);
  }

  /// Writes [data] to `assets/<path>` (`.data.json` added when missing)
  /// as a data asset of its registered type named [name] (the file's name
  /// by default), every field through the type's `get` (ADR-0027 §8).
  /// The instance then knows its asset: a scene, prefab or data asset
  /// written afterwards stores that id for every field that holds it. A
  /// data-typed field holding an instance never written is an
  /// `ArgumentError` — write that one first (for two that name each
  /// other, write one, then the other, then the first again).
  WrittenDocument writeData(String path, DataAsset data, {String? name}) {
    path = _withExtension(path, '.data.json');
    final type = _dataType(data);
    final fields = dataOf(data).data;
    final file = File('$assetsDir/$path');
    final existing = file.existsSync() ? jsonDecode(file.readAsStringSync()) as Map<String, Object?> : null;
    final id = existing?['id'] as String? ?? _uuid5(_ids, 'document/$path');
    final out = <String, Object?>{
      r'$schema': 'https://fynkel.dev/schemas/data.schema.json',
      'formatVersion': 1,
      'id': id,
      'name': name ?? path.split('/').last.replaceFirst('.data.json', ''),
      'type': type.id,
      'version': type.version,
      'data': fields,
    };
    final written = _writeText(file, path, id, out, const []);
    bindWrittenData(data, written.ref, type);
    return written;
  }

  // ---- in memory, for the editor (ADR-0028)

  /// [roots] and everything under them as a document's `entities` map —
  /// what [writeScene] would write, without a file: the editor's paste
  /// inserts it (`EditorEdit.add`). [roots] are the ids of the top
  /// objects, in order. The ids derive from [key], as a document's from
  /// its id; the editor gives the objects fresh ones.
  ({Map<String, Object?> entities, List<String> roots}) entitiesOf(List<ObjectSpec> roots, {String key = 'editor'}) {
    final doc = _Document(this, _uuid5(_ids, 'memory/$key'))..addAll(roots);
    return (entities: doc.entities, roots: List.unmodifiable(doc.roots));
  }

  /// [data]'s registered type and its fields as a data document holds
  /// them (`data`), every field through the type's `get` — what
  /// [writeData] writes, without a file.
  ({String type, int version, Map<String, Object?> data}) dataOf(DataAsset data) {
    final type = _dataType(data);
    return (
      type: type.id,
      version: type.version,
      data: <String, Object?>{
        for (final f in type.fields)
          f.name: f.holdsData ? _dataValue(f.target!(data), f.get(data), '${type.id}.${f.name}') : f.get(data),
      },
    );
  }

  /// [data] is now the asset [ref] (written by the editor): a scene,
  /// prefab or data asset written afterwards names [ref] for every field
  /// that holds it, as after [writeData].
  void bindData(DataAsset data, AssetRef ref) => bindWrittenData(data, ref, _dataType(data));

  /// [value] as a document field holds it: a vector or quaternion as a
  /// list, an [AssetRef] as `{asset, sub?}`, a written [DataAsset] as its
  /// asset, an enum by name, a component record through its `toJson`;
  /// lists and maps value by value; numbers, strings, bools and null as
  /// they are.
  static Object? jsonOf(Object? value) => switch (value) {
        null || num() || String() || bool() => value,
        Vec2(:final x, :final y) => [x, y],
        Vec3 v => docVec3(v),
        Vec4 v => docVec4(v),
        Quat q => docQuat(q),
        AssetRef r => docAssetRef(r),
        DataAsset d => d.asset == null
            ? throw ArgumentError('a ${d.runtimeType} that was never written has no asset to name')
            : docAssetRef(d.asset!),
        Enum e => e.name,
        List l => [for (final v in l) jsonOf(v)],
        Map m => {for (final e in m.entries) '${e.key}': jsonOf(e.value)},
        WireRecord r => (r as dynamic).toJson() as Map<String, Object?>,
        _ => throw ArgumentError.value(value, 'value', 'not a value a document field holds'),
      };

  DataType _dataType(DataAsset data) {
    final type = data.dataType ?? registry.dataTypeOf(data);
    if (type == null) {
      throw ArgumentError('${data.runtimeType} is not a registered data type: pass the generated registerBehaviours '
          'to ProjectWriter (and annotate the class with @FynkelDataAsset)');
    }
    return type;
  }

  static String _withExtension(String path, String ext) => path.endsWith(ext) ? path : '$path$ext';

  WrittenDocument _write(String path, String kind, String name, List<ObjectSpec> roots, {required bool prefab}) {
    final file = File('$assetsDir/$path');
    final existing = file.existsSync() ? jsonDecode(file.readAsStringSync()) as Map<String, Object?> : null;
    final id = existing?['id'] as String? ?? _uuid5(_ids, 'document/$path');
    final doc = _Document(this, id)..addAll(roots);
    final kept = existing == null ? const <String>[] : doc.keep(existing, prefab: prefab);
    final out = <String, Object?>{
      r'$schema': 'https://fynkel.dev/schemas/$kind.schema.json',
      'formatVersion': 7,
      'id': id,
      'name': name,
      'entities': doc.entities,
      if (prefab) 'root': doc.roots.single,
    };
    return _writeText(file, path, id, out, kept);
  }

  /// Writes [doc] canonically (2-space indent, a trailing newline) unless
  /// the file holds those bytes already.
  static WrittenDocument _writeText(File file, String path, String id, Map<String, Object?> doc, List<String> kept) {
    final text = '${const JsonEncoder.withIndent('  ').convert(doc)}\n';
    final changed = !file.existsSync() || file.readAsStringSync() != text;
    if (changed) {
      file.parent.createSync(recursive: true);
      file.writeAsStringSync(text);
    }
    return WrittenDocument(AssetRef(id), path, changed: changed, kept: kept);
  }

  // ---- prefab roots

  final Map<String, _PrefabRoot> _roots = {};
  Map<String, String>? _prefabPaths;

  /// The root of the prefab [ref] as an instance sees it: its id and its
  /// components by type.
  _PrefabRoot _prefabRoot(AssetRef ref) {
    final key = '${ref.asset}#${ref.sub ?? ''}';
    return _roots[key] ??= (ref.sub ?? '').isEmpty ? _documentRoot(ref) : _cookedRoot(ref);
  }

  _PrefabRoot _documentRoot(AssetRef ref) {
    // The project's prefabs, then its packages' (the project's own win).
    final paths = _prefabPaths ??= {
      for (final dir in [..._packageAssetDirs().reversed, assetsDir])
        if (Directory(dir).existsSync())
          for (final f in Directory(dir).listSync(recursive: true))
            if (f is File && f.path.endsWith('.prefab.json'))
              if ((jsonDecode(f.readAsStringSync()) as Map<String, Object?>)['id'] case final String id) id: f.path,
    };
    final path = paths[ref.asset];
    if (path == null) throw ArgumentError.value(ref, 'prefab', 'no prefab document in $assetsDir or its packages has this id');
    return _PrefabRoot.of(jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>, path);
  }

  _PrefabRoot _cookedRoot(AssetRef ref) {
    final index = File('$_cacheDir/index.json');
    if (!index.existsSync()) {
      throw StateError('$ref: an imported model\'s prefab is read from the cooked cache; run `fynkel cook` first');
    }
    final assets = (jsonDecode(index.readAsStringSync()) as Map<String, Object?>)['assets'] as List;
    final entry = assets.cast<Map<String, Object?>>().where((a) => a['id'] == ref.asset).firstOrNull;
    final object = entry?['file'] as String?;
    if (object == null) throw ArgumentError.value(ref, 'prefab', 'not in the cooked cache; run `fynkel cook`');
    final json = _cookedJsonChunk(File('$_cacheDir/$object').readAsBytesSync(), ref.sub!);
    if (json == null) throw ArgumentError.value(ref, 'prefab', 'the cooked asset has no "${ref.sub}"');
    return _PrefabRoot.of(json, '${entry!['path']}#${ref.sub}');
  }
}

/// The `JSON` chunk named [sub] of a cooked container (`fynkel_cooked`,
/// version 1: a 48-byte header, the chunk table at `table_offset`, chunk
/// 0 the `META` object naming every chunk).
Map<String, Object?>? _cookedJsonChunk(Uint8List bytes, String sub) {
  final d = ByteData.sublistView(bytes);
  if (bytes.length < 48 || utf8.decode(bytes.sublist(0, 4)) != 'FYNC' || d.getUint32(4, Endian.little) != 1) {
    throw const FormatException('not a version 1 cooked container');
  }
  final count = d.getUint32(12, Endian.little);
  final table = d.getUint64(16, Endian.little);
  Uint8List chunk(int i) {
    final at = table + i * 32;
    final offset = d.getUint64(at + 8, Endian.little), length = d.getUint64(at + 16, Endian.little);
    return Uint8List.sublistView(bytes, offset, offset + length);
  }

  final meta = jsonDecode(utf8.decode(chunk(0))) as Map<String, Object?>;
  for (final c in (meta['chunks'] as List? ?? const []).cast<Map<String, Object?>>()) {
    final i = c['index'] as int?;
    if (c['kind'] == 'JSON' && c['sub'] == sub && i != null && i < count) {
      return jsonDecode(utf8.decode(chunk(i))) as Map<String, Object?>;
    }
  }
  return null;
}

/// A prefab's root: its prefab-local id and its components.
final class _PrefabRoot {
  _PrefabRoot(this.name, this.id, this.components);

  factory _PrefabRoot.of(Map<String, Object?> doc, String shown) {
    final id = doc['root'] as String?;
    final entities = doc['entities'] as Map<String, Object?>?;
    final root = id == null ? null : entities?[id] as Map<String, Object?>?;
    if (root == null) throw FormatException('$shown: no root entity');
    if (root['prefab'] != null) {
      throw UnsupportedError('$shown: its root is itself a prefab instance; place that prefab instead');
    }
    final components = <String, (String, Map<String, Object?>)>{};
    for (final MapEntry(key: cid, value: c) in (root['components'] as Map<String, Object?>).entries) {
      final m = c as Map<String, Object?>;
      components[m['type'] as String] = (cid, (m['data'] as Map<String, Object?>?) ?? const {});
    }
    return _PrefabRoot(doc['name'] as String? ?? '', id!, components);
  }

  /// The prefab's name, an instance's when the spec gives none.
  final String name;
  final String id;

  /// By type id: the component's id and data.
  final Map<String, (String, Map<String, Object?>)> components;
}

/// One document being written.
final class _Document {
  _Document(this.writer, this.id);

  final ProjectWriter writer;
  final String id;
  final Map<String, Object?> entities = {};
  final List<String> roots = [];

  /// The derived id of each object written, by its path of names.
  final Map<String, String> _byPath = {};

  /// Each spec's place (path of names) and id, depth first as [add]
  /// writes them; the id of each spec and script, for references.
  final List<(String, String)> _planned = [];
  var _next = 0;
  final Map<ObjectSpec, String?> _specIds = Map.identity();
  final Map<Behaviour, String> _scriptIds = Map.identity();

  /// Adds [roots] and everything under them: every id first, so a script
  /// or a component can reference an object written after it (ADR-0036
  /// §4).
  void addAll(List<ObjectSpec> roots) {
    for (final r in roots) {
      _plan(r, '');
    }
    writingReferences(_reference, () {
      for (final r in roots) {
        add(r, null);
      }
    });
  }

  void _plan(ObjectSpec spec, String parentPath) {
    final root = spec.prefab == null ? null : writer._prefabRoot(spec.prefab!);
    final name = spec.name ?? root?.name ?? '';
    final base = '$parentPath/$name';
    var path = base;
    for (var n = 1; _byPath.containsKey(path); n++) {
      path = '$base#$n';
    }
    final eid = _uuid5(id, 'entity$path');
    _byPath[path] = eid;
    _planned.add((path, eid));
    // A spec written twice (a const one) has no one object to name.
    _specIds[spec] = _specIds.containsKey(spec) ? null : eid;
    for (final b in spec.behaviours) {
      _scriptIds[b] = eid;
    }
    for (final c in spec.children) {
      _plan(c, path);
    }
  }

  /// A reference in the document: to a spec's object or a script of it.
  Map<String, Object?> _reference(Object target) {
    switch (target) {
      case final GameObject g when specOfObject(g) != null:
        final spec = specOfObject(g)!;
        if (!_specIds.containsKey(spec)) {
          throw ArgumentError('a reference to the object of $spec, which is not in this document');
        }
        final eid = _specIds[spec] ??
            (throw ArgumentError('a reference to the object of $spec, which is twice in this document'));
        return {'entity': eid};
      case final Behaviour b when _scriptIds.containsKey(b):
        final eid = _scriptIds[b]!;
        final type = b.behaviourType ?? _registered(b);
        return {'entity': eid, 'component': _uuid5(eid, 'component/${type.id}')};
      default:
        throw ArgumentError('$target is not in this document: a written reference names a spec\'s object '
            '(spec.object) or a script of a spec');
    }
  }

  /// Adds [spec] and everything under it below [parent] (an entity id),
  /// at the place [_plan] gave it.
  void add(ObjectSpec spec, String? parent) {
    final root = spec.prefab == null ? null : writer._prefabRoot(spec.prefab!);
    final name = spec.name ?? root?.name ?? '';
    final (_, eid) = _planned[_next++];
    final components = <String, Object?>{};
    final overrides = <Map<String, Object?>>[];

    // Everything the spec sets on the object, by type id: the transform,
    // native components, layer, tags, then scripts.
    final values = <String, (int, Map<String, Object?>)>{};
    values['fynkel.Transform'] = (1, spec.transform.toJson());
    for (final c in spec.components) {
      final type = _typeOf(c);
      if (type == 'fynkel.Name') {
        throw ArgumentError.value(c, spec.toString(), 'the name is the spec\'s own');
      }
      // The generated `toJson` of every component record.
      values[type] = (1, (c as dynamic).toJson() as Map<String, Object?>);
    }
    for (final component in spec.data) {
      if (values.containsKey(component.type.id)) throw ArgumentError('duplicate component ${component.type.id}');
      values[component.type.id] = (component.type.version, component.toJson());
    }
    if (spec.layer case final layer?) {
      final i = writer.layersAndTags.layer(layer);
      if (i == null) throw ArgumentError('$spec: unknown layer "$layer"; the project declares ${writer.layersAndTags.layers}');
      values['fynkel.Layer'] = (1, gen.Layer(layer: i).toJson());
    }
    var tagBits = 0;
    if (spec.tags.isNotEmpty) {
      final bits = writer.layersAndTags.tagBits(spec.tags);
      if (bits == null) throw ArgumentError('$spec: unknown tag in ${spec.tags}; the project declares ${writer.layersAndTags.tags}');
      tagBits = bits;
      values['fynkel.Tags'] = (1, gen.Tags(bits: bits).toJson());
    }
    // A document holds one component of a type per object (FYN-D0404):
    // a second script of one class is for `world.build` only.
    final behaviours = <(String, int, Map<String, Object?>)>[];
    for (final b in spec.behaviours) {
      final type = b.behaviourType ?? _registered(b);
      if (behaviours.any((s) => s.$1 == type.id) || (root?.components.containsKey(type.id) ?? false)) {
        throw ArgumentError('$spec: a second ${type.id} — a scene or prefab holds one script of a type per object');
      }
      behaviours.add((type.id, type.version, _scriptData(type, b)));
    }

    Map<String, Object?>? instance;
    if (root != null) {
      void component(String type, int version, Map<String, Object?> data) {
        final have = root.components[type];
        if (have == null) {
          overrides.add({
            'op': 'addComponent',
            'entity': root.id,
            'component': _uuid5(eid, 'component/$type'),
            'type': type,
            'version': version,
            'data': data,
          });
          return;
        }
        for (final MapEntry(key: field, value: v) in data.entries) {
          if (_same(have.$2[field], v)) continue;
          overrides.add({'op': 'setField', 'entity': root.id, 'component': have.$1, 'field': field, 'value': v});
        }
      }

      for (final MapEntry(key: type, value: (version, data)) in values.entries) {
        if (type == 'fynkel.Tags') {
          // Added to the prefab's own, as addTag does.
          final own = (root.components[type]?.$2['bits'] as int?) ?? 0;
          component(type, version, gen.Tags(bits: own | tagBits).toJson());
        } else {
          component(type, version, data);
        }
      }
      for (final (type, version, data) in behaviours) {
        component(type, version, data);
      }
      instance = {
        'asset': docAssetRef(spec.prefab!),
        'instance': _uuid5(eid, 'instance'),
        'overrides': overrides,
      };
    } else {
      for (final MapEntry(key: type, value: (version, data)) in values.entries) {
        components[_uuid5(eid, 'component/$type')] = {'type': type, 'version': version, 'data': data};
      }
      for (final (type, version, data) in behaviours) {
        components[_uuid5(eid, 'component/$type')] = {'type': type, 'version': version, 'data': data};
      }
    }
    // In the editor's order; an instance has no components of its own
    // (ADR-0022) — the prefab root's, with the overrides.
    entities[eid] = <String, Object?>{
      'name': name,
      if (!spec.active) 'active': false,
      'parent': parent,
      'children': <String>[],
      'components': components,
      'prefab': ?instance,
    };
    if (parent == null) {
      roots.add(eid);
    } else {
      ((entities[parent] as Map<String, Object?>)['children'] as List<String>).add(eid);
    }
    for (final c in spec.children) {
      add(c, eid);
    }
  }

  /// Keeps the objects of [existing] that code did not make: every entity
  /// whose id is not the one its place would derive, with its subtree.
  /// Returns their names.
  List<String> keep(Map<String, Object?> existing, {required bool prefab}) {
    final old = (existing['entities'] as Map<String, Object?>?) ?? const {};
    // The ids code made last time: derived from the path of names, as now.
    final made = <String>{};
    final seen = <String>{};
    final named = <String, int>{};
    for (final e in old.values) {
      named.update((e as Map<String, Object?>)['name'] as String? ?? '', (n) => n + 1, ifAbsent: () => 1);
    }
    void walk(String eid, String parentPath) {
      final e = old[eid] as Map<String, Object?>?;
      if (e == null) return;
      final name = e['name'] as String? ?? '';
      final base = '$parentPath/$name';
      // The first free path of this name whose id it is: same-named
      // siblings the editor added do not take a number.
      String? path;
      for (var n = 0; n < named[name]! && path == null; n++) {
        final p = n == 0 ? base : '$base#$n';
        if (!seen.contains(p) && _uuid5(id, 'entity$p') == eid) path = p;
      }
      if (path == null) return; // the editor's
      seen.add(path);
      made.add(eid);
      for (final c in (e['children'] as List? ?? const []).cast<String>()) {
        walk(c, path);
      }
    }

    for (final MapEntry(key: eid, value: e) in old.entries) {
      if ((e as Map<String, Object?>)['parent'] == null) walk(eid, '');
    }
    final kept = <String>[];
    void keepTree(String eid, String? parent) {
      final e = Map<String, Object?>.of(old[eid] as Map<String, Object?>);
      e['parent'] = parent;
      entities[eid] = e;
      kept.add(e['name'] as String? ?? '');
      final children = (e['children'] as List? ?? const []).cast<String>().where((c) => old.containsKey(c)).toList();
      e['children'] = children;
      for (final c in children) {
        keepTree(c, eid);
      }
    }

    for (final MapEntry(key: eid, value: e) in old.entries) {
      if (made.contains(eid) || entities.containsKey(eid)) continue;
      final parent = (e as Map<String, Object?>)['parent'] as String?;
      if (parent != null && !made.contains(parent)) continue; // under another kept object
      // Under an object code still makes: after its children; else a root
      // (a prefab's under its root).
      final at = parent != null && entities.containsKey(parent) ? parent : (prefab ? roots.single : null);
      keepTree(eid, at);
      if (at == null) {
        roots.add(eid);
      } else {
        ((entities[at] as Map<String, Object?>)['children'] as List<String>).add(eid);
      }
    }
    return kept;
  }

  BehaviourType _registered(Behaviour b) {
    for (final t in writer.registry.types) {
      if (t.create().runtimeType == b.runtimeType) return t;
    }
    throw ArgumentError('${b.runtimeType} is not a registered behaviour: pass the generated registerBehaviours '
        'to ProjectWriter (and annotate the class with @FynkelBehaviour)');
  }
}

/// A script's `data`, through its registered `get`: a reference only
/// when it holds something (a spec's object or a script of a spec,
/// ADR-0036 §4), so None leaves the key out.
Map<String, Object?> _scriptData(BehaviourType type, Behaviour b) => {
      for (final f in type.fields)
        if (!f.isReference || f.target?.call(b) != null)
          f.name: f.holdsData ? _dataValue(f.target!(b), f.get(b), '${type.id}.${f.name}') : f.get(b),
    };

/// A data-typed field's [json] (its `get`), once every instance its
/// [target] holds is known to be written.
Object? _dataValue(Object? target, Object? json, String where) {
  for (final d in target is List ? target : [target]) {
    if (d is DataAsset && d.asset == null) {
      throw ArgumentError('$where holds a ${d.runtimeType} that was never written: '
          'write it with ProjectWriter.writeData first');
    }
  }
  return json;
}

String _typeOf(WireRecord c) {
  for (final codec in gen.componentCodecs.values) {
    if (codec.type == c.runtimeType) return codec.name;
  }
  throw ArgumentError.value(c, 'component', 'not a native component');
}

bool _same(Object? a, Object? b) => jsonEncode(a) == jsonEncode(b);

/// The namespace of every id this writer derives.
const _namespace = 'a3f0c0de-5c7e-4b1d-9e2f-6a0d0c0de026';

/// UUID version 5 (SHA-1) of [name] in [namespace], as `uuid.uuid5` and
/// Rust's `Uuid::new_v5` make it.
String _uuid5(String namespace, String name) {
  final hexNs = namespace.replaceAll('-', '');
  final ns = [for (var i = 0; i < 32; i += 2) int.parse(hexNs.substring(i, i + 2), radix: 16)];
  final h = sha1.convert([...ns, ...utf8.encode(name)]).bytes.sublist(0, 16);
  h[6] = (h[6] & 0x0f) | 0x50;
  h[8] = (h[8] & 0x3f) | 0x80;
  final hex = h.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
  return '${hex.substring(0, 8)}-${hex.substring(8, 12)}-${hex.substring(12, 16)}-${hex.substring(16, 20)}-${hex.substring(20)}';
}
