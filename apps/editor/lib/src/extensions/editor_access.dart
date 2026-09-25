// What an extension's `EditorContext` reads and changes (ADR-0028 step 2):
// the scene in front, its selection, edits as one undo step and the
// project's assets — all through the editor's own paths (the tab's
// transaction, the prefab overrides the Inspector writes, the Project
// panel's Create), so an extension's change is undone, overridden and
// shown exactly as the user's would be.
import 'dart:async' show scheduleMicrotask;

import 'package:flutter/foundation.dart' show Listenable;
import 'package:fynkel/authoring.dart' show ProjectWriter;
import 'package:fynkel/fynkel.dart' show AssetRef, BehaviourRegistry, DataAsset, ObjectSpec, World;
import 'package:fynkel_authoring/fynkel_authoring.dart' show AuthoringException, Command;
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import '../documents/document_tabs.dart';
import '../documents/open_document.dart';
import '../inspector/component_specs.dart';
import '../inspector/inspector_model.dart' show ComponentInstance, EntityInfo, readEntities;
import '../inspector/prefab_actions.dart' as prefab;
import '../play/game_panel.dart' show GameRun;
import '../project/editor_project.dart';
import '../project_browser/project_browser_model.dart';

/// The shell's parts an extension reaches, read when used (a project
/// opens and closes under a registered extension).
class EditorAccess {
  EditorAccess({
    required this.tabs,
    required this.project,
    required this.browser,
    required this.ping,
    required this.open,
    this.registerBehaviours,
    this.gameRun,
    this.stopPlay,
  });

  final DocumentTabs? Function() tabs;
  final EditorProject? Function() project;
  final ProjectBrowserModel? Function() browser;

  /// The Project panel's ping of an asset id.
  final void Function(String id) ping;

  /// Open Asset: a scene or prefab in its tab, anything else in the
  /// Inspector.
  final void Function(AssetEntry asset) open;

  /// The game's generated `registerBehaviours`: what `add` and
  /// `createData` need to write its behaviours and data types.
  final void Function(BehaviourRegistry registry)? registerBehaviours;

  /// In-editor Play (ADR-0021): the game's run, null without a game
  /// compiled in; and the toolbar's Stop.
  final GameRun? Function()? gameRun;
  final void Function()? stopPlay;

  /// The running game, or null when none runs (or it shows no world yet).
  EditorPlay? play() {
    final run = gameRun?.call();
    if (run == null || !run.running || run.control.world == null) return null;
    return _Play(run, stopPlay);
  }

  /// What a context reads, to listen to: the tabs, the document in front,
  /// the project and the Project panel (an extension window rebuilds on
  /// their changes).
  List<Listenable?> watch() {
    final t = tabs();
    final run = gameRun?.call();
    return [t, t?.shown, project(), browser(), run, run?.control.worldsChanged];
  }

  /// The document the Hierarchy shows (the running game's during Play).
  OpenDocument? get shown => tabs()?.shown;
  bool get playing => shown != null && identical(shown, tabs()?.live);

  /// A writer over the open project, fresh each time (its layers and tags
  /// as they are now).
  ProjectWriter writer() {
    final p = project();
    if (p == null) throw StateError('no project is open');
    return ProjectWriter(p.dir, registerBehaviours: registerBehaviours);
  }
}

// ── Play ──────────────────────────────────────────────────────────────────

final class _Play implements EditorPlay {
  _Play(this.run, this._stop);

  final GameRun run;
  final void Function()? _stop;
  bool _redrawing = false;

  @override
  World get world {
    // Whatever the caller changes, a paused Game view draws it (once,
    // after the callback).
    if (run.control.paused && !_redrawing) {
      _redrawing = true;
      scheduleMicrotask(() {
        _redrawing = false;
        run.control.redraw();
      });
    }
    return run.control.world ?? (throw StateError('Play has ended'));
  }

  @override
  List<World> get worlds => run.control.worlds;

  @override
  bool get isPaused => run.paused;

  @override
  void pause() {
    if (!run.paused) run.togglePause();
  }

  @override
  void resume() {
    if (run.paused) run.togglePause();
  }

  @override
  void step() => run.step();

  @override
  void stop() => _stop?.call();
}

// ── The scene ─────────────────────────────────────────────────────────────

/// [tab] read now: the objects in the Hierarchy's order, read once, on
/// first use.
EditorScene sceneOf(OpenDocument tab, {required bool playing}) => _Scene(tab, playing);

final class _Scene implements EditorScene {
  _Scene(this.tab, this.isPlaying);

  final OpenDocument tab;

  @override
  final bool isPlaying;

  @override
  String get id {
    final id = tab.document.id;
    return id.startsWith('play:') ? id.substring(5) : id;
  }

  @override
  String get path => tab.document.relativePath ?? tab.path;

  @override
  String get name => tab.title;

  @override
  bool get isPrefab => tab.kind == 'prefab';

  late final List<EditorObject> _objects = _read();
  late final Map<String, EditorObject> _byId = {for (final o in _objects) o.id: o};

  List<EditorObject> _read() {
    final t = tab.tree;
    final ids = [for (final r in t.roots) ...t.subtree(r)];
    final (infos, perEntity) = readEntities(tab, ids);
    return [for (final (i, info) in infos.indexed) objectOf(tab, info, perEntity[i])];
  }

  @override
  List<EditorObject> get objects => List.unmodifiable(_objects);

  @override
  List<EditorObject> get roots => [
    for (final o in _objects)
      if (o.parentId == null) o,
  ];

  @override
  EditorObject? byId(String id) => _byId[id];

  @override
  EditorObject? find(String name) {
    if (!name.contains('/')) return _objects.where((o) => o.name == name).firstOrNull;
    final parts = name.split('/');
    var level = roots;
    EditorObject? at;
    for (final part in parts) {
      at = level.where((o) => o.name == part).firstOrNull;
      if (at == null) return null;
      level = [for (final c in at.childIds) ?_byId[c]];
    }
    return at;
  }
}

/// One entity as an extension sees it.
EditorObject objectOf(OpenDocument tab, EntityInfo info, List<ComponentInstance> components) {
  final t = tab.tree;
  final asset = info.prefab?.assetId;
  return EditorObject(
    id: info.id,
    name: info.name,
    active: info.active,
    parentId: _parentOf(tab, info.id),
    childIds: List.unmodifiable(t.childrenOf(info.id)),
    components: List.unmodifiable([
      for (final c in components) EditorComponent(id: c.componentId, type: c.type, data: Map.unmodifiable(c.data)),
    ]),
    prefab: asset == null || asset.isEmpty ? null : AssetRef(asset, sub: info.prefab?.sub),
    fromPrefab: info.anchor != null,
  );
}

/// [id]'s parent as the Hierarchy shows it: a prefab's objects under the
/// instance (its root is the instance itself, ADR-0022).
String? _parentOf(OpenDocument tab, String id) {
  final t = tab.tree;
  final node = t[id];
  final parent = node?.parent;
  if (parent != null && t.childrenOf(parent).contains(id)) return parent;
  final anchor = node?.anchor;
  if (anchor == null) return parent;
  for (final x in t.subtree(anchor)) {
    if (t.childrenOf(x).contains(id)) return x;
  }
  return anchor;
}

// ── Edits ─────────────────────────────────────────────────────────────────

/// An extension's `edit`: [changes] collected into commands, then one
/// transaction on [tab]. A refused change, a throw in [changes] or a
/// refused transaction writes nothing; [logError] gets why.
EditResult runEdit(
  EditorAccess access,
  OpenDocument tab,
  String label,
  void Function(EditorEdit edit) changes, {
  required void Function(String message) logError,
}) {
  final edit = _Edit(access, tab, playing: identical(tab, access.tabs()?.live));
  EditResult refused(String why) {
    logError('"$label" changed nothing: $why');
    return EditResult(ok: false, error: why);
  }

  try {
    changes(edit);
  } on _Refused catch (e) {
    return refused(e.why);
  } catch (e) {
    return refused('$e');
  }
  if (edit.commands.isEmpty) return const EditResult(ok: true);
  final a = tab.transaction(edit.commands);
  if (a == null || !a.ok) return refused(tab.lastError ?? 'the editor refused it');
  // The top object of each `add`: a pasted root is the first id its
  // mutation reports; during Play the edit chose it.
  final pasted = [
    for (final m in a.mutations)
      if (m.command == 'pasteObjects' && m.affectedIds.isNotEmpty) m.affectedIds.first,
  ];
  final roots = edit.playRoots.isNotEmpty ? edit.playRoots : pasted;
  final added = <EditorObject>[];
  if (roots.isNotEmpty) {
    final (infos, perEntity) = readEntities(tab, roots);
    for (final (i, info) in infos.indexed) {
      added.add(objectOf(tab, info, perEntity[i]));
    }
  }
  return EditResult(ok: true, added: List.unmodifiable(added));
}

final class _Refused implements Exception {
  _Refused(this.why);
  final String why;
}

final class _Edit implements EditorEdit {
  _Edit(this.access, this.tab, {required this.playing});

  final EditorAccess access;
  final OpenDocument tab;
  final bool playing;
  final commands = <Map<String, Object?>>[];

  /// During Play: the root id each `add` gave its object.
  final playRoots = <String>[];

  /// What this edit has read: an entity and its components as they are
  /// with this edit's earlier overrides folded in.
  final _read = <String, (EntityInfo, List<ComponentInstance>)>{};
  final _folded = <String, Map<String, Object?>>{};
  late final _specs = ComponentSpecs(access.project()!);

  (EntityInfo, List<ComponentInstance>) _entity(EditorObject object) => _read.putIfAbsent(object.id, () {
    final (infos, perEntity) = readEntities(tab, [object.id]);
    if (infos.isEmpty) throw _Refused('$object is not in ${tab.title}');
    return (infos.single, perEntity.single);
  });

  /// The instance anchor an override on [info] goes to: its anchor when
  /// derived, itself when it is an instance (ADR-0022); null otherwise.
  String? _anchorOf(EntityInfo info) => info.anchor ?? (info.prefab != null ? info.id : null);

  void _structural(EditorObject object, String what) {
    if (object.fromPrefab) {
      throw _Refused('$object comes from its prefab, so it cannot be ${what}d here — open the prefab to change it');
    }
  }

  static Object? _json(Object? value) {
    try {
      return ProjectWriter.jsonOf(value);
    } on ArgumentError catch (e) {
      throw _Refused('${e.message}');
    }
  }

  @override
  void setField(EditorObject object, String type, String field, Object? value) {
    final (_, components) = _entity(object);
    final c = components.where((c) => c.type == type).firstOrNull;
    if (c == null) throw _Refused('$object has no $type');
    final json = _json(value);
    if (!c.isDerived) {
      commands.add(Command.setField(object.id, c.componentId, field, json));
      return;
    }
    final key = '${object.id}/${c.componentId}';
    final data = _folded[key] ?? c.data;
    final op = prefab.setFieldOverride(
      entity: c.entityLocal!,
      component: c.local ?? c.componentId,
      data: data,
      path: field,
      value: json,
    );
    _folded[key] = {...data, op['field'] as String: op['value']};
    commands.add(Command.setPrefabOverrideOp(c.anchor!, op));
  }

  @override
  void rename(EditorObject object, String name) {
    if (name.trim().isEmpty) throw _Refused('an object needs a name');
    _structural(object, 'rename');
    commands.add(Command.renameObject(object.id, name.trim()));
  }

  @override
  void setActive(EditorObject object, bool active) {
    final (info, _) = _entity(object);
    commands.add(
      info.anchor != null
          ? Command.setPrefabOverrideOp(info.anchor!, {'op': 'setActive', 'entity': info.local, 'value': active})
          : Command.setObjectActive(object.id, active),
    );
  }

  @override
  void addComponent(EditorObject object, String type, [Map<String, Object?> data = const {}]) {
    final (info, components) = _entity(object);
    if (components.any((c) => c.type == type)) {
      throw _Refused('$object already has a $type — an object holds one component per type (FYN-D0204)');
    }
    final spec = _specs.of(type);
    final Map<String, Object?> defaults;
    if (spec.isBehaviour) {
      if (spec.missingSchema) throw _Refused('$type is not a component this project knows');
      defaults = spec.schemaDefaults();
    } else {
      defaults = access.project()!.componentDefaults(type) ?? (throw _Refused('$type is not a native component'));
    }
    final full = {...defaults, for (final e in data.entries) e.key: _json(e.value)};
    final version = spec.version ?? 1;
    final anchor = _anchorOf(info);
    if (anchor == null) {
      commands.add(Command.addComponent(object.id, {'type': type, 'version': version, 'data': full}));
      return;
    }
    final local = tab.expansionOf(anchor)?.entity(object.id)?.local;
    if (local == null) throw _Refused('$object\'s prefab did not expand');
    commands.add(
      Command.setPrefabOverrideOp(
        anchor,
        prefab.addComponentOverride(entity: local, type: type, version: version, data: full),
      ),
    );
    // What a later `setField` on it would fold from is not known here.
    _read.remove(object.id);
  }

  @override
  void removeComponent(EditorObject object, String type) {
    final (info, components) = _entity(object);
    final c = components.where((c) => c.type == type).firstOrNull;
    if (c == null) throw _Refused('$object has no $type');
    if (_anchorOf(info) != null) {
      throw _Refused('$object\'s $type comes from its prefab — remove it there (open the prefab)');
    }
    commands.add(Command.removeComponent(object.id, c.componentId));
  }

  void _under(EditorObject? parent) {
    if (parent != null && parent.fromPrefab) {
      throw _Refused('nothing can be put under $parent: it comes from its prefab');
    }
  }

  @override
  void reparent(EditorObject object, EditorObject? parent) {
    _structural(object, 'move');
    _under(parent);
    commands.add(Command.reparentObject(object.id, parent: parent?.id));
  }

  @override
  void remove(EditorObject object) {
    _structural(object, 'delete');
    commands.add(Command.deleteObject(object.id));
  }

  @override
  void add(ObjectSpec spec, {EditorObject? parent}) {
    _under(parent);
    final ({Map<String, Object?> entities, List<String> roots}) built;
    try {
      built = access.writer().entitiesOf([spec]);
    } on ArgumentError catch (e) {
      throw _Refused('${e.message}');
    }
    if (!playing) {
      commands.add(
        Command.pasteObjects({
          for (final e in built.entities.entries) e.key: (e.value! as Map).cast<String, Object?>(),
        }, parent: parent?.id),
      );
      return;
    }
    // The running game has no paste: each object made, parents first,
    // under fresh ids.
    final fresh = {for (final id in built.entities.keys) id: prefab.newUuid()};
    for (final MapEntry(key: id, value: e) in built.entities.entries) {
      final entity = (e! as Map).cast<String, Object?>();
      if (entity['prefab'] != null) throw _Refused('a prefab instance cannot be added while the game runs');
      final up = entity['parent'] as String?;
      commands.add({
        'command': 'createObject',
        'id': fresh[id],
        'name': entity['name'] ?? '',
        'parent': up == null ? parent?.id : fresh[up],
        'components': [
          for (final c in (entity['components'] as Map? ?? const {}).values) (c as Map).cast<String, Object?>(),
        ],
      });
    }
    playRoots.add(fresh[built.roots.single]!);
  }
}

// ── Assets ────────────────────────────────────────────────────────────────

EditorAssets assetsOf(EditorAccess access, {required void Function(String message) logError}) =>
    _Assets(access, logError);

final class _Assets implements EditorAssets {
  _Assets(this.access, this.logError);

  final EditorAccess access;
  final void Function(String message) logError;

  late final List<AssetEntry> _entries = access.project()?.assets() ?? const [];
  late final List<EditorAsset> _all = [for (final e in _entries) assetOf(e)];

  @override
  List<EditorAsset> get all => List.unmodifiable(_all);

  @override
  List<EditorAsset> ofKind(String kind) => [
    for (final a in _all)
      if (a.kind == kind) a,
  ];

  @override
  List<EditorAsset> ofDataType(String typeId) => [
    for (final a in _all)
      if (a.dataType == typeId) a,
  ];

  @override
  EditorAsset? find(String path) {
    for (final a in _all) {
      if (a.path == path || _stem(a.path) == path) return a;
    }
    return _all.where((a) => a.name == path).firstOrNull;
  }

  /// `data/coin.data.json` → `data/coin`.
  static String _stem(String path) {
    final slash = path.lastIndexOf('/');
    final dot = path.indexOf('.', slash + 1);
    return dot > slash + 1 ? path.substring(0, dot) : path;
  }

  @override
  List<EditorAsset> get selected {
    final ids = access.browser()?.selection ?? const <String>{};
    return [
      for (final a in _all)
        if (ids.contains(a.id)) a,
    ];
  }

  @override
  EditorAsset? createData(String path, DataAsset value) {
    final project = access.project();
    if (project == null) {
      logError('createData("$path"): no project is open');
      return null;
    }
    final rel = path.endsWith('.data.json') ? path : '$path.data.json';
    try {
      final writer = access.writer();
      final d = writer.dataOf(value);
      final r = project.newDocument('data', rel, type: d.type, data: d.data);
      final id = r.id;
      if (id == null) throw StateError('the editor gave the new asset no id');
      writer.bindData(value, AssetRef(id));
      final entry = project.assets().where((a) => a.id == id).firstOrNull;
      return entry == null
          ? EditorAsset(id: id, path: rel, name: _stem(rel).split('/').last, kind: 'data', dataType: d.type)
          : assetOf(entry);
    } on AuthoringException catch (e) {
      logError('createData("$rel") wrote nothing: $e');
    } on ArgumentError catch (e) {
      logError('createData("$rel") wrote nothing: ${e.message}');
    } on StateError catch (e) {
      logError('createData("$rel") wrote nothing: ${e.message}');
    }
    return null;
  }

  @override
  void ping(EditorAsset asset) => access.ping(asset.id);

  @override
  void open(EditorAsset asset) {
    final entry = (access.project()?.assets() ?? const <AssetEntry>[]).where((e) => e.id == asset.id).firstOrNull;
    if (entry == null) {
      logError('open: $asset is not in the project');
      return;
    }
    access.open(entry);
  }
}

EditorAsset assetOf(AssetEntry e) =>
    EditorAsset(id: e.id, path: e.path, name: e.name, kind: kindOf(e), dataType: e.dataType);

/// An asset's kind as extensions name it: a document's own kind, a
/// source's by what imports it.
String kindOf(AssetEntry e) {
  if (!e.isSource) return e.document ?? 'document';
  return switch (e.importer) {
    'fynkel.gltf' || 'fynkel.fbx' => 'model',
    'fynkel.image' => 'texture',
    'fynkel.audio' => 'audio',
    final i? when i.startsWith('fynkel.') => i.substring(7),
    final i => i ?? 'source',
  };
}
