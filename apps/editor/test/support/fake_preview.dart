// An in-memory edit-preview world for the Scene view tests: every entity
// with a Transform is a unit cube at its world matrix (what the real
// preview draws for an entity without a mesh), lights and cameras carry an
// icon, hidden subtrees leave picking, `setTransforms` previews a local
// transform until the document changes or `restoreTransforms` — the same
// contract `ScenePreview` has, over a `FakeDocument`, no native world
// (`worldAddress` 0: the viewport draws nothing).
import 'dart:async';
import 'dart:math' as math;

import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/documents/editor_document.dart';
import 'package:fynkel_editor/src/hierarchy/transform_math.dart';

import 'fake_document.dart';

final class FakePreviewBackend implements PreviewBackend {
  FakePreviewBackend(this.document) {
    document.addListener(_onDocument);
  }

  final FakeDocument document;

  /// Previewed local transforms by entity (a gizmo drag in flight).
  final overrides = <String, Trs>{};

  /// What `setComponents` holds in the world: entity id → type → data.
  final componentOverrides = <String, Map<String, Map<String, Object?>>>{};

  /// The ids `setHidden` was last given (subtrees, as the editor passes them).
  Set<String> hiddenIds = const {};

  /// Every call, for the tests: `camera`, `hidden`, `pick`, `pickRect`,
  /// `lines`, `worldMatrix`, `setTransforms`, `restoreTransforms`,
  /// `resync`, `reload`, `recook`, `dump`, `stats`, `close`.
  final calls = <String>[];

  /// The last camera set.
  PreviewCamera? camera;

  final _syncs = StreamController<PreviewSyncReport>.broadcast();
  int _loads = 1;
  int _syncsCount = 0;
  bool _closed = false;

  @override
  int get worldAddress => 0;
  @override
  bool get isClosed => _closed || document.isClosed;
  @override
  PreviewStats get stats => _stats();
  @override
  List<Diagnostic> diagnostics = const [];
  @override
  CookReport? cook;
  @override
  Stream<PreviewSyncReport> get syncs => _syncs.stream;

  void _onDocument() {
    // The session's sync after a transaction/undo/redo/reload: the world
    // follows the document, previews are gone.
    final edit = document.lastEdit;
    if (edit == null || edit.kind == DocumentChangeKind.saved || edit.kind == DocumentChangeKind.moved) return;
    overrides.clear();
    _syncsCount++;
    _syncs.add(
      PreviewSyncReport.fromJson({
        'handle': 1,
        'updated': edit.affectedIds?.length ?? 0,
        'reloaded': edit.wholeDocument,
      }),
    );
  }

  PreviewStats _stats() {
    final es = document.entitiesView;
    var meshes = 0, lights = 0;
    for (final e in es.values) {
      final types = _types(e);
      if (types.contains('fynkel.MeshRenderer')) meshes++;
      if (types.contains('fynkel.Light')) lights++;
    }
    final hidden = {for (final id in hiddenIds) ..._subtree(id)}.length;
    return PreviewStats.fromJson({
      'entities': es.length,
      'live': es.length,
      'meshes': meshes,
      'lights': lights,
      'hidden': hidden,
      'tick': 0,
      'loads': _loads,
      'syncs': _syncsCount,
    });
  }

  static List<String> _types(Map<String, Object?> e) => [
    for (final c in (e['components'] as Map? ?? const {}).values)
      if (c is Map) c['type'] as String? ?? '',
  ];

  List<String> _subtree(String id) {
    final out = <String>[];
    void walk(String i) {
      final e = document.entitiesView[i];
      if (e == null) return;
      out.add(i);
      for (final c in e['children'] as List? ?? const []) {
        walk(c as String);
      }
    }

    walk(id);
    return out;
  }

  Set<String> get _hiddenAll => {for (final id in hiddenIds) ..._subtree(id)};

  // ── Transforms ─────────────────────────────────────────────────────────

  /// The expanded entities of every prefab instance, as the real world
  /// holds them under their derived ids (M4-07): parent and Transform.
  Map<String, (String?, Trs?)> get _derived {
    final out = <String, (String?, Trs?)>{};
    for (final e in document.entitiesView.entries) {
      if (e.value['prefab'] == null) continue;
      final PrefabExpansion x;
      try {
        x = document.instance(e.key);
      } on AuthoringException {
        continue;
      }
      for (final d in x.entities) {
        Trs? t;
        for (final c in d.components.values) {
          if (c.type == 'fynkel.Transform') t ??= Trs.fromData(c.data);
        }
        out[d.id] = (d.parent, t);
      }
    }
    return out;
  }

  bool _known(String id) => document.entitiesView.containsKey(id) || _derived.containsKey(id);

  Trs? _local(String id) {
    final o = overrides[id];
    if (o != null) return o;
    final e = document.entitiesView[id];
    if (e == null) return _derived[id]?.$2;
    for (final c in (e['components'] as Map? ?? const {}).values) {
      if (c is Map && c['type'] == 'fynkel.Transform') {
        final d = c['data'];
        return Trs.fromData(d is Map<String, Object?> ? d : null);
      }
    }
    return null;
  }

  String? _parentOf(String id) => document.entitiesView[id]?['parent'] as String? ?? _derived[id]?.$1;

  /// The world matrix of [id] from the document, the expansions and the
  /// overrides; null for an entity without a Transform (or unknown).
  List<double>? worldOf(String id) {
    final chain = <String>[];
    String? cur = id;
    while (cur != null && chain.length < 100000) {
      chain.insert(0, cur);
      cur = _parentOf(cur);
    }
    if (_local(id) == null) return null;
    var m = List.of(identityMatrix);
    for (final a in chain) {
      final t = _local(a);
      if (t != null) m = matMul(m, t.toMatrix());
    }
    return m;
  }

  /// The ids the preview draws: with a Transform, not hidden.
  Iterable<String> get _visible {
    final hidden = _hiddenAll;
    return document.entitiesView.keys.where((id) => !hidden.contains(id) && _local(id) != null);
  }

  // ── Calls ──────────────────────────────────────────────────────────────

  void _check(String call) {
    if (isClosed) throw StateError('the preview is closed ($call)');
    calls.add(call);
  }

  @override
  void setCamera(PreviewCamera camera) {
    _check('camera');
    this.camera = camera;
  }

  @override
  int setHidden(Iterable<String> ids) {
    _check('hidden');
    hiddenIds = ids.toSet();
    return _hiddenAll.length;
  }

  /// The same ray `fynkel_authoring::View::ray` builds.
  static (List<double>, List<double>) rayOf(PreviewCamera camera, double aspect, List<double> ndc) {
    final m = camera.world;
    List<double> col(int i) => [m[i], m[i + 1], m[i + 2]];
    final eye = col(12);
    List<double> unit(List<double> v) {
      final l = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
      return [v[0] / l, v[1] / l, v[2] / l];
    }

    final right = unit(col(0)), up = unit(col(4)), back = unit(col(8));
    if (camera.orthographic) {
      final h = camera.orthoHeight / 2;
      final o = [for (var i = 0; i < 3; i++) eye[i] + right[i] * ndc[0] * h * aspect + up[i] * ndc[1] * h];
      return (o, [-back[0], -back[1], -back[2]]);
    }
    final t = math.tan(camera.fovY / 2);
    final dx = ndc[0] * t * aspect, dy = ndc[1] * t;
    final d = unit([for (var i = 0; i < 3; i++) right[i] * dx + up[i] * dy - back[i]]);
    return (eye, d);
  }

  /// Ray vs the entity's unit cube (in its own space): the distance along
  /// the ray, or null.
  double? _hitCube(List<double> world, List<double> origin, List<double> dir) {
    final inv = matInverseAffine(world);
    List<double> tp(List<double> v) => [
      inv[0] * v[0] + inv[4] * v[1] + inv[8] * v[2] + inv[12],
      inv[1] * v[0] + inv[5] * v[1] + inv[9] * v[2] + inv[13],
      inv[2] * v[0] + inv[6] * v[1] + inv[10] * v[2] + inv[14],
    ];
    List<double> td(List<double> v) => [
      inv[0] * v[0] + inv[4] * v[1] + inv[8] * v[2],
      inv[1] * v[0] + inv[5] * v[1] + inv[9] * v[2],
      inv[2] * v[0] + inv[6] * v[1] + inv[10] * v[2],
    ];
    final o = tp(origin), d = td(dir);
    var tMin = -double.infinity, tMax = double.infinity;
    for (var i = 0; i < 3; i++) {
      if (d[i].abs() < 1e-12) {
        if (o[i].abs() > 0.5) return null;
        continue;
      }
      var t1 = (-0.5 - o[i]) / d[i], t2 = (0.5 - o[i]) / d[i];
      if (t1 > t2) (t1, t2) = (t2, t1);
      tMin = math.max(tMin, t1);
      tMax = math.min(tMax, t2);
      if (tMin > tMax) return null;
    }
    if (tMax < 0) return null;
    final tLocal = tMin >= 0 ? tMin : tMax;
    // Back to world distance: the local hit point through the world matrix.
    final p = [for (var i = 0; i < 3; i++) o[i] + d[i] * tLocal];
    final wp = [
      world[0] * p[0] + world[4] * p[1] + world[8] * p[2] + world[12],
      world[1] * p[0] + world[5] * p[1] + world[9] * p[2] + world[13],
      world[2] * p[0] + world[6] * p[1] + world[10] * p[2] + world[14],
    ];
    final dx = wp[0] - origin[0], dy = wp[1] - origin[1], dz = wp[2] - origin[2];
    return math.sqrt(dx * dx + dy * dy + dz * dz);
  }

  @override
  PreviewPick pick(PreviewCamera camera, double aspect, List<double> ndc) {
    _check('pick');
    final (origin, dir) = rayOf(camera, aspect, ndc);
    return pickRay(origin, dir);
  }

  PreviewPick pickRay(List<double> origin, List<double> dir) {
    final hits = <Map<String, Object?>>[];
    for (final id in _visible) {
      final w = worldOf(id)!;
      final t = _hitCube(w, origin, dir);
      if (t == null) continue;
      final types = _types(document.entitiesView[id]!);
      hits.add({
        'id': id,
        'distance': t,
        'point': [for (var i = 0; i < 3; i++) origin[i] + dir[i] * t],
        'mesh': types.contains('fynkel.MeshRenderer'),
      });
    }
    hits.sort((a, b) => (a['distance'] as double).compareTo(b['distance'] as double));
    return PreviewPick.fromJson({
      'ray': {'origin': origin, 'dir': dir},
      'hits': hits,
    });
  }

  @override
  List<String> pickRect(PreviewCamera camera, double aspect, List<double> min, List<double> max) {
    _check('pickRect');
    final view = matInverseAffine(camera.world);
    final out = <String>[];
    for (final id in _visible) {
      final w = worldOf(id)!;
      final p = [w[12], w[13], w[14]];
      final v = [
        view[0] * p[0] + view[4] * p[1] + view[8] * p[2] + view[12],
        view[1] * p[0] + view[5] * p[1] + view[9] * p[2] + view[13],
        view[2] * p[0] + view[6] * p[1] + view[10] * p[2] + view[14],
      ];
      double nx, ny;
      if (camera.orthographic) {
        final h = camera.orthoHeight / 2;
        nx = v[0] / (h * aspect);
        ny = v[1] / h;
      } else {
        if (v[2] >= -camera.near) continue;
        final t = math.tan(camera.fovY / 2);
        nx = v[0] / (-v[2] * t * aspect);
        ny = v[1] / (-v[2] * t);
      }
      if (nx >= min[0] && nx <= max[0] && ny >= min[1] && ny <= max[1]) out.add(id);
    }
    return out;
  }

  /// A handle per drawn id: its place in the document plus one.
  @override
  List<int> entities(Iterable<String> ids) {
    _check('entities');
    final order = document.entitiesView.keys.toList();
    final drawn = _visible.toSet();
    return {
      for (final id in ids)
        if (drawn.contains(id)) order.indexOf(id) + 1,
    }.toList();
  }

  static const _cubeEdges = [
    [0, 1], [1, 3], [3, 2], [2, 0], //
    [4, 5], [5, 7], [7, 6], [6, 4], //
    [0, 4], [1, 5], [2, 6], [3, 7],
  ];

  @override
  PreviewLines lines({
    Iterable<String> outline = const [],
    List<double>? outlineColor,
    bool outlineBoxes = true,
    double? iconsAspect,
  }) {
    _check('lines');
    final color = outlineColor ?? const [1.0, 0.6, 0.1, 1.0];
    final lines = <List<double>>[];
    for (final id in outline) {
      final w = worldOf(id);
      if (w == null) continue;
      // Without boxes a mesh is the renderer's outline's to ring.
      if (!outlineBoxes && _types(document.entitiesView[id]!).contains('fynkel.MeshRenderer')) continue;
      final corners = [
        for (final x in const [-0.5, 0.5])
          for (final y in const [-0.5, 0.5])
            for (final z in const [-0.5, 0.5])
              [
                w[0] * x + w[4] * y + w[8] * z + w[12],
                w[1] * x + w[5] * y + w[9] * z + w[13],
                w[2] * x + w[6] * y + w[10] * z + w[14],
              ],
      ];
      for (final e in _cubeEdges) {
        lines.add([...corners[e[0]], ...corners[e[1]], ...color]);
      }
    }
    if (iconsAspect != null) {
      for (final id in _visible) {
        final types = _types(document.entitiesView[id]!);
        if (!types.contains('fynkel.Light') && !types.contains('fynkel.Camera')) continue;
        final w = worldOf(id)!;
        final c = [w[12], w[13], w[14]];
        for (var i = 0; i < 3; i++) {
          final a = List.of(c), b = List.of(c);
          a[i] -= 0.25;
          b[i] += 0.25;
          lines.add([...a, ...b, 1, 1, 0.3, 1]);
        }
      }
    }
    return PreviewLines.fromJson({'count': lines.length, 'lines': lines});
  }

  @override
  Map<String, List<double>> worldMatrices(Iterable<String> ids) {
    _check('worldMatrix');
    return {for (final id in ids) id: ?worldOf(id)};
  }

  @override
  void setTransforms(Iterable<PreviewTransform> transforms) {
    _check('setTransforms');
    for (final t in transforms) {
      if (!_known(t.id)) continue;
      overrides[t.id] = Trs(position: t.position, rotation: t.rotation, scale: t.scale);
    }
  }

  @override
  void restoreTransforms(Iterable<String> ids) {
    _check('restoreTransforms');
    for (final id in ids) {
      overrides.remove(id);
    }
  }

  @override
  int setComponents(Iterable<({String id, String type, Map<String, Object?> data})> components) {
    _check('setComponents');
    var count = 0;
    for (final c in components) {
      if (!_known(c.id) || !c.type.startsWith('fynkel.')) continue;
      (componentOverrides[c.id] ??= {})[c.type] = c.data;
      count++;
    }
    return count;
  }

  @override
  void restoreComponents(Iterable<String> ids) {
    _check('restoreComponents');
    for (final id in ids) {
      componentOverrides.remove(id);
    }
  }

  /// What `setMaterial` holds: material id → its fields.
  final materialOverrides = <String, Map<String, Object?>>{};

  @override
  void setMaterial(String material, Map<String, Object?> data) {
    _check('setMaterial');
    materialOverrides[material] = data;
  }

  @override
  void restoreMaterial(String material) {
    _check('restoreMaterial');
    materialOverrides.remove(material);
  }

  @override
  Map<String, Object?> material(String material) {
    _check('material');
    return materialOverrides[material] ?? const {};
  }

  /// What `setVolume` holds: profile id → its fields.
  final volumeOverrides = <String, Map<String, Object?>>{};

  @override
  void setVolume(String volume, Map<String, Object?> data) {
    _check('setVolume');
    volumeOverrides[volume] = data;
  }

  @override
  void restoreVolume(String volume) {
    _check('restoreVolume');
    volumeOverrides.remove(volume);
  }

  @override
  Map<String, Object?> volume(String volume) {
    _check('volume');
    return volumeOverrides[volume] ?? const {};
  }

  @override
  PreviewSyncReport resync() {
    _check('resync');
    overrides.clear();
    componentOverrides.clear();
    _syncsCount++;
    return PreviewSyncReport.fromJson({'handle': 1});
  }

  @override
  PreviewStats reload() {
    _check('reload');
    overrides.clear();
    _loads++;
    return _stats();
  }

  /// Whether the last recook was Play's (the unsaved documents as held).
  bool? lastRecookLive;

  @override
  CookReport recook({bool live = false}) {
    _check('recook');
    lastRecookLive = live;
    return cook = CookReport.fromJson({'ok': true, 'profile': 'desktop', 'cached': 0, 'cooked': 0, 'failed': 0});
  }

  /// `{ entities: { id: { name, world } } }` — the world matrices as the
  /// world holds them; equal after a drag's commit and after a fresh load.
  @override
  Map<String, Object?> dump() {
    _check('dump');
    final ids = document.entitiesView.keys.toList()..sort();
    return {
      'entities': {
        for (final id in ids)
          id: {
            'name': document.entitiesView[id]!['name'],
            'world': [for (final v in worldOf(id) ?? identityMatrix) Trs.clean(v)],
          },
      },
    };
  }

  @override
  PreviewStats refreshStats() {
    _check('stats');
    return _stats();
  }

  @override
  void close() {
    if (_closed) return;
    calls.add('close');
    _closed = true;
    document.removeListener(_onDocument);
    _syncs.close();
  }
}
