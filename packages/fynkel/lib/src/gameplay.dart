// Ordinary SDK adapter over the native F2 journal. Handles and cache epochs live
// here; component values, hierarchy and validation belong to the native journal.
part of 'world.dart';

final Object _gameplayZoneKey = Object();

final class _GameplayCallbackEntrySkipped implements Exception {
  const _GameplayCallbackEntrySkipped();
}

extension _WorldGameplay on World {
  _GameplayAdapter? get _gameplay {
    // A rejected async callback retains its expired scope. It must not resume
    // through a fresh gameplay edit after the synchronous edit has ended.
    final scoped = Zone.current[_gameplayZoneKey] as _GameplayAdapter?;
    final edit = scoped ?? _activeGameplay;
    if (edit == null) return null;
    edit.checkOpen();
    if (!identical(edit.world, this)) edit.reject('cross-world gameplay access');
    return edit;
  }

  R _withGameplay<R>(R Function() callback) {
    final edit = _gameplay;
    return edit == null ? _editGameplay(callback) : edit.guard(callback);
  }

  void _rejectDuringEdit(String operation) {
    _gameplay?.reject('$operation is not supported inside World.edit');
  }

  void _reconcileGameplayState() {
    final state = _native.gameplayStateDelta(_gameplayStateRevision);
    final changed = state.changes;
    // Reset only when this reader is new or older than retained history.
    final affected = state.reset ? _byEntity.keys.toList() : changed.keys;
    for (final entity in affected) {
      final go = _byEntity[entity];
      final name = changed[entity];
      if (name == null) {
        if (go != null) {
          go._alive = false;
          go._observed = false;
          _byEntity.remove(entity);
          _observed.remove(go);
          _behaviours.objectDied();
        }
        _toSubscribe.remove(entity);
      } else if (go != null) {
        go._name = name;
      }
    }
    if (state.reset) {
      _toSubscribe.removeWhere((entity) => !changed.containsKey(entity));
    }
    _gameplayStateRevision = state.revision;
    // Native deltas include hierarchy/activity mutations, not just names.
    // Empty/pose-only transactions must not restart a full lifecycle scan.
    if (state.reset || changed.isNotEmpty) _behaviours.objectChanged();
  }

  R _callbackGameplay<R>(R Function() callback) {
    final edit = _gameplay;
    if (edit != null) return edit.guard(callback);
    if (_callbackEntryBlocked || isDisposed) throw const _GameplayCallbackEntrySkipped();
    var entered = false;
    try {
      return _editGameplay(() {
        entered = true;
        return callback();
      }, automatic: true);
    } catch (error, stack) {
      if (entered) rethrow;
      // A failed entry belongs to the host boundary, not the next script.
      // Skipped callbacks retain lifecycle state for a later successful entry.
      if (!_inFixedPhase && !_inUpdatePhase && !_callbackPreflight) rethrow;
      _callbackEntryBlocked = true;
      _deferredErrors.add((error, stack));
      throw const _GameplayCallbackEntrySkipped();
    }
  }

  R _editGameplay<R>(R Function() callback, {bool automatic = false}) {
    final current = _gameplay;
    if (current != null) {
      if (!current.automatic || current.explicitDepth != 0) current.reject('nested edit');
      current.explicitDepth++;
      try {
        return current.guard(() {
          final value = callback();
          if (value is Future) {
            value.then<void>((_) {}, onError: (Object _, StackTrace _) {});
            current.reject('gameplay edits must be synchronous');
          }
          return value;
        });
      } finally {
        current.explicitDepth--;
      }
    }
    _reconcileGameplayState();
    _GameplayAdapter? adapter;
    var published = false;
    try {
      final commit = _native.editGameplay((native) {
        final edit = adapter = _GameplayAdapter(this, native, automatic: automatic);
        _activeGameplay = edit;
        return Zone.current.fork(zoneValues: {_gameplayZoneKey: edit}).run(() {
          final value = callback();
          // NativeWorld rejects Future results too; do not prepare publication or
          // let a caught SDK-side error escape the native transaction boundary.
          if (value is! Future) {
            edit.checkOpen();
            edit.prepareCommit();
          }
          return value;
        });
      });
      _activeGameplay = null;
      adapter!.closed = true;
      published = true;
      adapter!.publish(commit);
      adapter!.completeBoundaryActions();
      return commit.value;
    } catch (_) {
      _activeGameplay = null;
      if (!published) adapter?.abort();
      rethrow;
    } finally {
      _activeGameplay = null;
      if (adapter != null) adapter!.closed = true;
    }
  }
}

class _GameplayAdapter {
  _GameplayAdapter(this.world, this.native, {this.automatic = false});
  final bool automatic;
  int explicitDepth = 0;
  double? timeScale;
  bool _bindingsChanged = false;
  final List<void Function()> _commitActions = [];
  final List<void Function()> _abortActions = [];
  void afterCommit(void Function() action) => guard(() => _commitActions.add(action));
  void onAbort(void Function() action) => guard(() => _abortActions.add(action));
  void completeBoundaryActions() {
    (Object, StackTrace)? firstError;
    for (final action in _commitActions) {
      try {
        action();
      } catch (error, stack) {
        firstError ??= (error, stack);
      }
    }
    _commitActions.clear();
    if (firstError case final failure?) Error.throwWithStackTrace(failure.$1, failure.$2);
  }

  final World world;
  final NativeGameplayEdit native;
  final Map<NativeGameplayRef, GameObject> handles = {};
  final Map<GameObject, NativeGameplayRef> pending = {};
  final Set<NativeGameplayRef> _removed = {};
  final Map<GameObject, String?> _names = {};
  final Map<ObjectSpec, GameObject> _specObjects = Map.identity();
  NativeGameplayPrefab? _lastPrefab;
  List<Behaviour> _lastPrefabAttachments = [];
  final Map<String, DataAsset> _dataAssets = {};
  final Map<String, bool> _runtimeClips = {};
  final Map<String, gen.PbrMaterial?> _materials = {};
  final Map<String, MeshData?> _meshes = {};
  final Map<String, Map<String, Object?>?> _shaderMaterials = {};
  final Map<String, Map<String, Object?>?> _codeValues = {};

  final Set<DataAsset> _createdData = Set.identity();
  final Set<String> _loadingData = {};
  // Scripts have one Dart owner: the behaviour scheduler. This journal holds
  // attachment/lifecycle intent only, never copies of user script fields.
  final Map<Behaviour, GameObject> _attachments = Map.identity();
  final Map<Behaviour, int> _attachmentOrders = Map.identity();
  final Map<Behaviour, bool> _enabledChanges = Map.identity();
  final Set<Behaviour> _behaviourRemovals = Set.identity();
  final Set<Coroutine> _scheduled = Set.identity();
  final Set<Coroutine> _stoppedRoutines = Set.identity();
  final Set<Coroutine> _preparedRoutines = Set.identity();
  List<Behaviour> _preparedBehaviours = [];
  final List<(Behaviour, GameObject, int)> _preparedAttachments = [];
  int _preparedSequence = 0;
  bool closed = false, failed = false;

  Iterable<Coroutine> routinesFor(Behaviour owner) => guard(() => _scheduled.where((c) => identical(c._owner, owner)));

  bool routineStopped(Coroutine c) => guard(() => _stoppedRoutines.contains(c));

  Coroutine schedule(Coroutine c) => guard(() {
    if (c._owner case final owner?) {
      final go = _behaviourOwner(owner);
      if (owner._destroyRequested || owner._destroyed || behaviourRemoved(owner) || !native.contains(ref(go))) {
        reject('cannot schedule work on a removed behaviour');
      }
    }
    if (c._target case final target?) {
      if (!native.contains(ref(target))) reject('cannot schedule work on a destroyed object');
    }
    c._system = world._behaviours;
    world._behaviours._arm(c, c._initialWait);
    _scheduled.add(c);
    return c;
  });

  void stopRoutine(Coroutine c) => guard(() {
    if (!identical(c._system, world._behaviours)) reject('coroutine belongs to another world');
    _stoppedRoutines.add(c);
  });

  Never reject(String message) {
    failed = true;
    throw StateError(message);
  }

  void checkOpen() {
    if (closed) throw StateError('World.edit scope has expired');
    if (failed) throw StateError('World.edit previously failed');
  }

  T guard<T>(T Function() operation) {
    checkOpen();
    try {
      return operation();
    } catch (_) {
      failed = true;
      rethrow;
    }
  }

  NativeGameplayRef ref(GameObject object) => guard(() {
    final ownPending = pending[object];
    if (ownPending != null) return ownPending;
    final go = object;
    if (!identical(go._world, world)) reject('object belongs to another world');
    if (!go._alive) reject('object has been destroyed or its edit was aborted');
    final staged = pending[go];
    if (staged != null) return staged;
    final entity = go._entity;
    if (entity == null) reject('object is pending outside this edit');
    return NativeGameplayRef.live(entity);
  });

  GameObject object(NativeGameplayRef reference, {bool validatedPopulation = false}) => guard(() {
    if (!validatedPopulation && !native.contains(reference)) reject('cannot attach a dead object');
    final known = handles[reference];
    if (known != null) return known;
    final live = reference.live;
    if (live == null) reject('pending object is not owned by this edit');
    final existing = world._byEntity[live];
    if (existing != null) return handles[reference] = existing;
    // Cache only identity. A lazy committed name prevents a staged rename from
    // leaking if this live handle is first discovered inside an aborted edit.
    final go = GameObject._(world, identityTransform, '').._entity = live;
    world._byEntity[live] = go;
    return handles[reference] = go;
  });

  GameObject spawn(Pose transform, GameObject? parent, String name, [GameObject? handle]) => guard(() {
    final reference = native.spawn(transform: transform, parent: parent == null ? null : ref(parent), name: name);
    final go = handle ?? GameObject._(world, transform, name);
    pending[go] = reference;
    handles[reference] = go;
    return go;
  });

  LoadReport bindActions(Map<String, List<String>> rebinds) => guard(() {
    // The native request encodes the caller's map synchronously; retaining or
    // changing its lists later cannot alter this compiled staged table.
    final report = LoadReport.fromJson(native.bindActions(rebinds));
    _bindingsChanged = true;
    afterCommit(() {
      if (EditorChannel.enabled) world._loadedForEditor(report, 'bindActions');
      if (report.warnings.isNotEmpty) world.onLoadDiagnostics(report.warnings.toList(growable: false));
    });
    return report;
  });

  GameObject clone(GameObject source, {GameObject? parent, Pose? transform, String? name}) => guard(() {
    final result = native.cloneTree(
      ref(source),
      parent: parent == null ? null : ref(parent),
      transform: transform,
      name: name,
    );
    final copies = <GameObject, GameObject>{};
    // Every copied identity exists before any user factory or field setter runs.
    for (final entry in result.entities.entries) {
      final original = object(entry.key);
      final copy = GameObject._(world, identityTransform, '');
      pending[copy] = entry.value;
      handles[entry.value] = copy;
      copies[original] = copy;
    }
    world._copyBehaviours(copies);
    return handles[result.root]!;
  });

  GameObject build(ObjectSpec spec, GameObject? parent, Pose transform) =>
      buildAll([spec], parent, transform: transform).single;

  List<GameObject> buildAll(List<ObjectSpec> specs, GameObject? parent, {Pose? transform}) => guard(() {
    // Validate topology without recursively traversing an unchecked user list.
    // A spec has one identity in an edit, including across separate build calls.
    final seen = Set<ObjectSpec>.identity();
    void check(ObjectSpec node) {
      if (!seen.add(node) || _specObjects.containsKey(node)) {
        reject('an ObjectSpec can appear only once in an edit');
      }

      for (final child in node.children) {
        check(child);
      }
    }

    for (final spec in specs) {
      check(spec);
    }
    world._checkSpecs(specs);
    final made = <GameObject>[];
    GameObject spawnTree(ObjectSpec node, GameObject? owner, Pose pose) {
      final handle = node._unboundObject;
      final go = node.prefab == null
          ? spawn(pose, owner, node.name ?? '', handle)
          : instantiate(node.prefab!, pose, owner, node.name, handle);
      _specObjects[node] = go;
      made.add(go);
      for (final child in node.children) {
        spawnTree(child, go, child.transform);
      }
      return go;
    }

    final roots = [for (final spec in specs) spawnTree(spec, parent, transform ?? spec.transform)];
    // Allocate the entire forest before resolving references, including references
    // between top-level objects. This is the same path as a single-tree build.
    final iterator = made.iterator;
    for (final spec in specs) {
      iterator.moveNext();
      world._fillSpec(spec, iterator);
    }
    // References already hold canonical handles. Validate ownership without
    // invoking setters on caller-owned script instances.
    for (final node in seen) {
      for (final behaviour in node.behaviours) {
        final type = behaviour._type ?? world._registeredTypeOf(behaviour);
        for (final field in type?.fields ?? const <BehaviourField>[]) {
          if (!field.isReference) continue;
          final target = field.target?.call(behaviour);
          final go = switch (target) {
            GameObject g => g,
            Behaviour b => _behaviourOwner(b),
            NativeComponent c => c.gameObject,
            _ => null,
          };
          if (go != null && !native.contains(ref(go))) reject('script reference names a destroyed object');
          if (target is Behaviour && behaviourRemoved(target)) {
            reject('script reference names a removed behaviour');
          }
        }
      }
    }
    return roots;
  });

  GameObject instantiate(Object prefab, Pose transform, GameObject? parent, String? name, [GameObject? handle]) =>
      guard(() {
        final id = switch (prefab) {
          AssetRef r when (r.sub ?? '').isEmpty || r.sub == World.importedPrefabSub => r.asset,
          String s => s,
          _ => throw ArgumentError.value(prefab, 'prefab', 'a prefab asset id or prefab AssetRef'),
        };
        final instance = native.instantiate(id, transform, parent == null ? null : ref(parent), name ?? '');
        final objects = <String, GameObject>{};
        for (final entry in instance.entities.entries) {
          final go = entry.value == instance.root && handle != null
              ? handle
              : GameObject._(world, identityTransform, '');
          pending[go] = entry.value;
          handles[entry.value] = go;
          objects[entry.key] = go;
        }
        final scripts = <String, Behaviour>{};
        final authored = <(Map<String, Object?>, Behaviour)>[];
        for (final raw in instance.metadata['behaviours'] as List) {
          final data = (raw as Map).cast<String, Object?>();
          final type = data['type'] as String;
          final fields = (data['data'] as Map).cast<String, Object?>();
          final descriptor = world.registry.lookup(type);
          if (descriptor == null) reject('prefab behaviour type $type is not registered');

          final behaviour = world.registry.instantiate(type, fields: fields, version: data['version'] as int);
          final component = data['component'] as String;
          behaviour._authoredComponent = component;
          attachBehaviour(objects[data['authoringId']]!, behaviour);
          scripts[component] = behaviour;
          authored.add((data, behaviour));
        }
        // These are newly constructed, unpublished instances. Field setters execute
        // before commit; their failure discards ownership and all native changes.
        // Arbitrary constructor/setter side effects remain outside rollback.
        for (final (data, behaviour) in authored) {
          final fields = data['data'] as Map;
          for (final field in behaviour.behaviourType!.fields) {
            if (field.holdsData && fields[field.name] != null) {
              field.set(behaviour, resolveData(fields[field.name], field.dataType!));
            }
            if (!field.isReference || fields[field.name] == null) continue;
            final value = fields[field.name];
            if (value is! Map) reject('invalid authored reference ${field.name}');
            final target = value['component'] == null ? objects[value['entity']] : scripts[value['component']];
            if (target == null) reject('unresolved authored reference ${field.name}');
            if (target is Behaviour && !identical(target.gameObject, objects[value['entity']])) {
              reject('authored component reference belongs to another object');
            }
            field.set(behaviour, target);
          }
        }
        _lastPrefab = instance;
        _lastPrefabAttachments = authored.map((pair) => pair.$2).toList();
        return handles[instance.root]!;
      });

  T data<T extends DataAsset>(AssetRef ref) => guard(() {
    final value = world._data.load(ref, []);
    if (value is! T) reject('data asset ${ref.asset} is not a $T');
    return value;
  });

  Object? resolveData(Object? value, String typeId) => guard(() {
    DataAsset? one(Object? item) {
      if (item == null) return null;
      if (item is! Map || item['asset'] is! String) reject('invalid data-asset reference');
      if (item['sub'] != null && item['sub'] != '') reject('data asset cannot name a sub-asset');
      final ref = AssetRef(item['asset'] as String);
      if (ref.isNone) return null;
      final asset = data<DataAsset>(ref);
      if (asset.dataType?.id != typeId) reject('data asset ${ref.asset} is not a $typeId');
      return asset;
    }

    if (value is List) return [for (final item in value) ?one(item)];
    return one(value);
  });

  T? get<T extends WireRecord>(GameObject go) => guard(() {
    if (T == gen.Name) {
      final name = native.name(ref(go));
      return name == null ? null : gen.Name(value: name) as T;
    }
    return native
        .component<T>(
          ref(go),
          decodeReference: (reference) {
            // Ordinary object fields retain the existing SDK's weak-reference rule.
            if (reference == null || !native.contains(reference)) return null;
            return object(reference);
          },
        )
        ?.value;
  });

  WireRecord? lookup(GameObject go, int id) =>
      guard(() => gen.componentCodecs[id]!.withType(<T extends WireRecord>() => get<T>(go)));

  void set(GameObject go, WireRecord value) => guard(() {
    final owner = ref(go);
    if (value is gen.Name) {
      native.setName(owner, value.value);
    } else {
      native.set(owner, value, encodeReference: (target) => target == null ? null : ref(target));
    }
  });

  List<GameObject> objects({List<int> components = const [], bool includeInactive = true}) => guard(() {
    final wantsName = components.contains(gen.Name.componentId);
    final refs = native.entities(components: components.where((id) => id != gen.Name.componentId).toList());
    return [
      for (final r in refs)
        if ((!wantsName || native.name(r) != null) && (includeInactive || native.active(r).$2)) object(r),
    ];
  });

  Affine3 matrix(GameObject go) => guard(() {
    final m = native.worldMatrix(ref(go));
    return Affine3(Vec3(m[0], m[1], m[2]), Vec3(m[4], m[5], m[6]), Vec3(m[8], m[9], m[10]), Vec3(m[12], m[13], m[14]));
  });

  bool behaviourRemoved(Behaviour behaviour) =>
      guard(() => behaviour._retired || _behaviourRemovals.contains(behaviour));

  bool behaviourEnabled(Behaviour behaviour) => guard(() => _enabledChanges[behaviour] ?? behaviour._enabled);

  GameObject _behaviourOwner(Behaviour behaviour) {
    final staged = _attachments[behaviour];
    if (staged != null) return staged;
    if (!identical(behaviour._system, world._behaviours) || behaviour._gameObject == null) {
      reject('behaviour does not belong to this world');
    }
    return behaviour._gameObject!;
  }

  void setBehaviourEnabled(Behaviour behaviour, bool enabled) => guard(() {
    final go = _behaviourOwner(behaviour);
    if (behaviour._destroyed ||
        behaviour._destroyRequested ||
        behaviourRemoved(behaviour) ||
        !go.isAlive ||
        !native.contains(ref(go))) {
      reject('cannot change enabled on a removed behaviour');
    }
    _enabledChanges[behaviour] = enabled;
  });

  void destroyBehaviour(Behaviour behaviour) => guard(() {
    final go = _behaviourOwner(behaviour);
    // Repeated destruction is idempotent, including after a previous commit.
    if (behaviour._destroyed || behaviour._destroyRequested || behaviourRemoved(behaviour)) return;
    if (!go.isAlive || !native.contains(ref(go))) return;
    _behaviourRemovals.add(behaviour);
  });

  void attachBehaviour(GameObject go, Behaviour behaviour) => guard(() {
    if (!native.contains(ref(go))) reject('cannot attach a behaviour to a destroyed object');
    if (behaviour._system != null || _attachments.containsKey(behaviour)) {
      reject('behaviour is already attached');
    }
    _attachments[behaviour] = go;
    // Evaluate user code before publication. The staged binding is available to
    // the getter, but none of the scheduler's committed lists have changed.
    _attachmentOrders[behaviour] = behaviour.order;
  });

  List<Behaviour> behavioursOf(GameObject go) => guard(() {
    if (!native.contains(ref(go))) reject('cannot read behaviours of a destroyed object');
    return List.unmodifiable([
      for (final b in go._behaviours)
        if (!behaviourRemoved(b)) b,
      for (final entry in _attachments.entries)
        if (identical(entry.value, go) && !behaviourRemoved(entry.key)) entry.key,
    ]);
  });

  List<Behaviour> allBehaviours() => guard(() {
    final orders = Map<Behaviour, int>.identity();
    final result = <Behaviour>[];
    for (final b in world._behaviours._all) {
      if (!behaviourRemoved(b) && b._gameObject!._alive && native.contains(ref(b._gameObject!))) {
        result.add(b);
        orders[b] = b.order;
      }
    }
    for (final entry in _attachments.entries) {
      if (!behaviourRemoved(entry.key) && native.contains(ref(entry.value))) {
        result.add(entry.key);
        orders[entry.key] = _attachmentOrders[entry.key]!;
      }
    }
    // Stable tie order is the committed attach order followed by staged attach
    // order. No user getters run in the comparator.
    final positions = Map<Behaviour, int>.identity();
    for (var i = 0; i < result.length; i++) {
      positions[result[i]] = i;
    }
    result.sort((a, b) {
      final order = orders[a]!.compareTo(orders[b]!);
      return order != 0 ? order : positions[a]!.compareTo(positions[b]!);
    });
    return List.unmodifiable(result);
  });

  void prepareCommit() => guard(() {
    // Existing order getters are user code too. Evaluate them before final
    // membership, as they can perform ordinary edits. No getter runs after
    // native publication. Existing dying scripts stay until settle delivers
    // their destruction callbacks.
    final orders = Map<Behaviour, int>.identity();
    if (_attachments.isNotEmpty) {
      for (final b in world._behaviours._all) {
        orders[b] = b.order;
      }
    }
    for (final entry in native.identityChanges().entries) {
      if (entry.value == null) _removed.add(entry.key);
      final go = handles[entry.key] ?? (entry.key.live == null ? null : world._byEntity[entry.key.live]);
      if (go != null) _names[go] = entry.value;
    }
    for (final c in _scheduled) {
      if (_stoppedRoutines.contains(c)) continue;
      final owner = c._owner;
      if (owner != null && (behaviourRemoved(owner) || _removed.contains(ref(_behaviourOwner(owner))))) {
        continue;
      }
      if (owner != null && !c._invoke && !native.active(ref(_behaviourOwner(owner))).$2) continue;
      if (c._target case final target? when _removed.contains(ref(target))) continue;
      _preparedRoutines.add(c);
    }
    _preparedSequence = world._behaviours._nextSequence;
    if (_attachments.isNotEmpty) {
      final sequence = Map<Behaviour, int>.identity();
      _preparedBehaviours = [...world._behaviours._all];
      for (final b in _preparedBehaviours) {
        sequence[b] = b._sequence;
      }
      for (final entry in _attachments.entries) {
        if (behaviourRemoved(entry.key) || _removed.contains(ref(entry.value))) continue;
        final b = entry.key;
        sequence[b] = _preparedSequence++;
        orders[b] = _attachmentOrders[b]!;
        _preparedAttachments.add((b, entry.value, sequence[b]!));
        _preparedBehaviours.add(b);
      }
      _preparedBehaviours.sort((a, b) {
        final order = orders[a]!.compareTo(orders[b]!);
        return order != 0 ? order : sequence[a]!.compareTo(sequence[b]!);
      });
    }
  });

  void publish<R>(NativeGameplayCommit<R> commit) {
    void publishMap<T>(Map<String, T?> staged, Map<String, T> committed) {
      for (final entry in staged.entries) {
        final value = entry.value;
        if (value == null) {
          committed.remove(entry.key);
        } else {
          committed[entry.key] = value;
        }
      }
    }

    publishMap(_materials, world._runtimeMaterials);
    publishMap(_meshes, world._runtimeMeshes);
    publishMap(_shaderMaterials, world._shaderMaterials);
    publishMap(_codeValues, world._codeValues);
    for (final entry in _runtimeClips.entries) {
      entry.value ? world._runtimeClips.add(entry.key) : world._runtimeClips.remove(entry.key);
    }
    if (_bindingsChanged) world.input.invalidate();
    // No callbacks, native calls, or user codecs after publication. The commit
    // report resolves identity; prepared metadata reconciles existing handles.
    for (final entry in pending.entries) {
      final entity = commit.resolve(entry.value);
      entry.key
        .._world = world
        .._spec = null
        .._alive = entity != null;
      if (entity != null) {
        entry.key._entity = entity;
        world._byEntity[entity] = entry.key;
      }
    }
    for (final entry in _names.entries) {
      final go = entry.key;
      final name = entry.value;
      if (name == null) {
        go._alive = false;
        world._byEntity.remove(go._entity);
        world._behaviours.objectDied();
        world._toSubscribe.remove(go._entity);
        world._observed.remove(go);
        go._observed = false;
      } else {
        go._name = name;
      }
    }
    if (_attachments.isNotEmpty) {
      for (final (behaviour, go, sequence) in _preparedAttachments) {
        behaviour
          .._gameObject = go
          .._system = world._behaviours
          .._sequence = sequence;
        go._behaviours.add(behaviour);
        world._observe(go);
      }
      world._behaviours._all
        ..clear()
        ..addAll(_preparedBehaviours);
      world._behaviours._nextSequence = _preparedSequence;
    }
    for (final entry in _enabledChanges.entries) {
      final b = entry.key;
      // A canceled attachment stays unbound and unchanged. Existing scripts
      // retain their final requested state for cleanup, even when also removed.
      if (identical(b._system, world._behaviours) && !b._retired) {
        b._enabled = entry.value;
      }
    }
    for (final b in _behaviourRemovals) {
      if (identical(b._system, world._behaviours)) {
        b._retired = true;
        b._destroyRequested = true;
      }
    }
    for (final entry in _specObjects.entries) {
      ObjectSpec._objects[entry.key] = entry.value;
    }
    if (_lastPrefab case final instance?) {
      final metadata = instance.metadata;
      final resolved = <String, Object?>{
        for (final entry in instance.entities.entries)
          if (commit.resolve(entry.value) case final entity?) entry.key: entity.toJson(),
      };
      final report = InstantiateReport.fromJson({
        'ok': true,
        'diagnostics': metadata['diagnostics'],
        'residency': metadata['residency'],
        'instance': {
          ...metadata,
          'root': (commit.resolve(instance.root) ?? EntityRef.nil).toJson(),
          'entities': resolved.length,
          'behaviours': [
            for (final raw in metadata['behaviours'] as List)
              if (resolved[(raw as Map)['authoringId']] case final entity?)
                {...raw, 'entity': entity, 'references': resolved},
          ],
        },
      });
      world.lastInstantiate = report.withBehaviours(
        attached: _preparedAttachments.where((entry) => _lastPrefabAttachments.contains(entry.$1)).length,
        extra: [],
      );
    }
    for (final c in _stoppedRoutines) {
      c._done = true;
    }
    for (final c in _scheduled) {
      if (_preparedRoutines.contains(c)) {
        c._owner?._routines.add(c);
        world._behaviours._routines.add(c);
      } else {
        c._finish();
      }
    }
    if (timeScale case final scale?) world._timeScale = scale;
    world._data.byId.addAll(_dataAssets);
    // Native identity/activity changes are reconciled at the next entry.
    // These Dart-owned membership changes need their own invalidation.
    if (_attachments.isNotEmpty || _enabledChanges.isNotEmpty || _behaviourRemovals.isNotEmpty) {
      world._behaviours.objectChanged();
    }
  }

  void abort() {
    closed = true;
    (Object, StackTrace)? firstError;
    for (final action in _abortActions) {
      try {
        action();
      } catch (error, stack) {
        firstError ??= (error, stack);
      }
    }
    _abortActions.clear();
    for (final c in _scheduled) {
      c._finish();
    }
    for (final asset in _dataAssets.values) {
      asset._destroyed = true;
    }
    for (final go in pending.keys) {
      go._alive = false;
    }
    if (firstError case final failure?) Error.throwWithStackTrace(failure.$1, failure.$2);
  }
}
