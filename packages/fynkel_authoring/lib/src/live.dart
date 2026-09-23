// Live Play mode (ADR-0021 P7, docs/design/live-play-mode.md): a running
// game's world read and changed in the scene document's terms, through
// `fynkel_world_live_call`. No session: the world is the game's (the
// editor gets its address from the Game view's play scope), and nothing
// here touches a document. A write applies at once, at the current tick,
// without stepping — it shows while the game is paused — and is gone on
// Stop with the world.

part of 'session.dart';

/// One entity of the running world, parents before children.
final class LiveEntity {
  const LiveEntity({
    required this.id,
    required this.handle,
    required this.name,
    required this.parent,
    required this.spawned,
    required this.layer,
    required this.childCount,
    required this.types,
  });

  factory LiveEntity.fromJson(Map<String, Object?> j) => LiveEntity(
        id: j['id'] as String,
        handle: (j['entity'] as num?)?.toInt() ?? 0,
        name: j['name'] as String? ?? '',
        parent: j['parent'] as String?,
        spawned: j['spawned'] == true,
        layer: (j['layer'] as num?)?.toInt() ?? 0,
        childCount: (j['children'] as num?)?.toInt() ?? 0,
        types: [for (final t in j['types'] as List? ?? const []) t as String],
      );

  /// The authoring id when a loaded scene spawned it (a prefab instance's
  /// children carry the ids the cook derived), `runtime:<handle>` when the
  /// game did.
  final String id;

  /// The runtime entity handle (what package:fynkel's `GameObject.entity` is).
  final int handle;
  final String name;
  final String? parent;

  /// Spawned or instantiated by the game: no document has it.
  final bool spawned;
  final int layer;
  final int childCount;

  /// The native component types it carries, in registry order.
  final List<String> types;
}

/// One entity as the Inspector reads it: every native component as the
/// scene document stores it (`{type, data}`), and its world pose.
final class LiveEntityState {
  const LiveEntityState({
    required this.id,
    required this.handle,
    required this.name,
    required this.parent,
    required this.world,
    required this.components,
  });

  factory LiveEntityState.fromJson(Map<String, Object?> j) => LiveEntityState(
        id: j['id'] as String,
        handle: (j['entity'] as num?)?.toInt() ?? 0,
        name: j['name'] as String? ?? '',
        parent: j['parent'] as String?,
        world: _mapOf(j['world']),
        components: [
          for (final c in j['components'] as List? ?? const [])
            (type: _mapOf(c)['type'] as String, data: _mapOf(_mapOf(c)['data'])),
        ],
      );

  final String id;
  final int handle;
  final String name;
  final String? parent;
  final Map<String, Object?> world;
  final List<({String type, Map<String, Object?> data})> components;

  Map<String, Object?>? component(String type) {
    for (final c in components) {
      if (c.type == type) return c.data;
    }
    return null;
  }
}

/// A running world by its native `FynkelWorld*` address. Holds nothing:
/// the world belongs to the game, so drop this with it (a call on a
/// destroyed world is undefined — the play scope says when it goes).
final class LiveWorld {
  LiveWorld(this.worldAddress) {
    if (worldAddress == 0) throw ArgumentError.value(worldAddress, 'worldAddress', 'NULL world');
  }

  final int worldAddress;

  /// The running world's tick at the last [entities] read.
  int tick = 0;

  /// The table's revision at the last [entities] or [changedEntities]
  /// read (a hash of every row; it changes when an entity is spawned,
  /// destroyed, renamed, re-parented, re-layered or re-tagged or gains or
  /// loses a native component — not when a value moves).
  String? revision;

  /// Every entity, parents before children: the loaded scenes' roots in
  /// document order, then what the game made; children in sibling order.
  List<LiveEntity> entities() => _entities(null)!;

  /// [entities], or null when the table is the one [revision] names: the
  /// rows are not built or sent then, which is what makes polling cheap.
  List<LiveEntity>? changedEntities() => _entities(revision);

  List<LiveEntity>? _entities(String? since) {
    final r = call('entities', {'since': ?since});
    tick = (r['tick'] as num?)?.toInt() ?? tick;
    revision = r['revision'] as String?;
    if (r['unchanged'] == true) return null;
    return [for (final e in r['entities'] as List? ?? const []) LiveEntity.fromJson(_mapOf(e))];
  }

  /// [id]'s name, parent, world pose and native components; throws an
  /// [AuthoringException] (invalidArgument) when it is gone.
  LiveEntityState entity(String id) => LiveEntityState.fromJson(call('entity', {'id': id}));

  /// Replaces (or adds) [type] on [id] from its document JSON. The world's
  /// refusals come back as messages; empty when it applied.
  List<String> set(String id, String type, Map<String, Object?> data) =>
      _write('set', {'id': id, 'type': type, 'data': data});

  List<String> remove(String id, String type) => _write('remove', {'id': id, 'type': type});

  List<String> rename(String id, String name) => _write('rename', {'id': id, 'name': name});

  List<String> _write(String kind, Map<String, Object?> args) {
    final r = _live(kind, args, rejectedIsResult: true);
    return [for (final d in r['rejected'] as List? ?? const []) '${_mapOf(d)['message']}'];
  }

  /// One `fynkel_world_live_call`. Throws an [AuthoringException] unless
  /// it succeeded (or, with [rejectedIsResult], the world refused it).
  Map<String, Object?> call(String kind, [Map<String, Object?> args = const {}]) => _live(kind, args);

  Map<String, Object?> _live(String kind, Map<String, Object?> args, {bool rejectedIsResult = false}) {
    final kindBytes = utf8.encode(kind);
    final argBytes = utf8.encode(jsonEncode(args));
    final buf = calloc<Uint8>(kindBytes.length + argBytes.length + 1);
    final view = calloc<n.FynkelBufferView>();
    try {
      buf.asTypedList(kindBytes.length).setAll(0, kindBytes);
      final argPtr = buf + kindBytes.length;
      argPtr.asTypedList(argBytes.length).setAll(0, argBytes);
      final status = AuthoringStatus._of(n.fynkel_world_live_call(
          Pointer<n.FynkelWorld>.fromAddress(worldAddress), buf, kindBytes.length, argPtr, argBytes.length, view));
      final v = view.ref;
      final result = v.length == 0
          ? const <String, Object?>{}
          : _mapOf(jsonDecode(utf8.decode(v.data.asTypedList(v.length))));
      if (status == AuthoringStatus.ok || (rejectedIsResult && status == AuthoringStatus.commandsRejected)) {
        return result;
      }
      throw AuthoringException('live $kind', status, Diagnostic.listOf(result['diagnostics']));
    } finally {
      calloc.free(buf);
      calloc.free(view);
    }
  }
}
