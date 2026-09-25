// Objects described in code (ADR-0026 §2): an `ObjectSpec` is a tree —
// name, transform, active, tags, layer, native components, behaviours,
// children, or an instance of a prefab asset — and `World.build` spawns it.
// A function returning a spec is a code prefab: call it once per copy.
// The same description is what `package:fynkel/authoring.dart` writes as
// a scene or prefab document (ADR-0026 §2, step 6).

part of 'world.dart';

/// One object and everything under it, to [World.build].
final class ObjectSpec {
  /// A new object named [name] (Unity's `new GameObject(name)`):
  /// [components] are native component values (`const RigidBody()`,
  /// `MeshRenderer(mesh: Assets.models.crate.meshes.crate)`; a `Transform`
  /// among them replaces [transform]), [behaviours] script instances with
  /// their fields set (`Spinner()..speed = 2`), [children] the objects
  /// under it in order. [transform] is local to the parent. [tags] and
  /// [layer] name the project's (`fynkel.project.json`); an inactive
  /// object ([active] false) builds with its subtree switched off.
  const ObjectSpec(
    String this.name, {
    this.transform = identityTransform,
    this.active = true,
    this.tags = const [],
    this.layer,
    this.components = const [],
    this.behaviours = const [],
    this.children = const [],
  }) : prefab = null;

  /// An instance of the prefab asset [prefab] (`Assets.prefabs.coin`, or a
  /// model's `prefab` sub-asset), as `World.instantiate` makes it: named
  /// [name] (the prefab's name when null), at [transform]; [components],
  /// [behaviours], [tags], [layer] and [active] apply to its root on top of
  /// the prefab's own (a component the prefab has is replaced), and
  /// [children] are added after the prefab's.
  const ObjectSpec.prefab(
    AssetRef this.prefab, {
    this.name,
    this.transform = identityTransform,
    this.active = true,
    this.tags = const [],
    this.layer,
    this.components = const [],
    this.behaviours = const [],
    this.children = const [],
  });

  /// The object's name; null only for a prefab instance keeping the
  /// prefab's.
  final String? name;

  /// The prefab this is an instance of, or null for a new object.
  final AssetRef? prefab;

  final Transform transform;
  final bool active;
  final Iterable<String> tags;
  final String? layer;
  final List<WireRecord> components;

  /// Attached as they are: a behaviour instance belongs to one object, so a
  /// spec with behaviours builds once — make it in a function to build it
  /// again.
  final List<Behaviour> behaviours;
  final List<ObjectSpec> children;

  /// This spec and every spec under it, depth first.
  Iterable<ObjectSpec> get _all sync* {
    yield this;
    for (final c in children) {
      yield* c._all;
    }
  }

  @override
  String toString() => prefab == null ? 'ObjectSpec($name)' : 'ObjectSpec.prefab(${name ?? prefab!.asset})';
}

extension on World {
  /// Checks the whole tree before anything spawns, so a mistake leaves no
  /// half-built object behind.
  void _checkSpec(ObjectSpec spec) {
    final seen = Set<Behaviour>.identity();
    for (final s in spec._all) {
      for (final b in s.behaviours) {
        if (b._system != null) {
          throw StateError('$s: $b is already attached to ${b._gameObject} — build a spec with behaviours once, '
              'or make it in a function that creates them');
        }
        if (!seen.add(b)) throw StateError('$s: $b appears twice in the spec');
      }
      for (final t in s.tags) {
        _tagBit(t);
      }
      if (s.layer != null) _layerIndex(s.layer!);
      if (s.prefab != null) {
        final r = s.prefab!;
        if ((r.sub ?? '').isNotEmpty && r.sub != World.importedPrefabSub) {
          throw ArgumentError.value(r, '$s', 'sub "${r.sub}" is not a prefab');
        }
      }
    }
  }

  GameObject _buildSpec(ObjectSpec spec, GameObject? parent, Transform transform) {
    final GameObject go;
    if (spec.prefab case final prefab?) {
      // The native instantiate needs a spawned parent; under one this build
      // spawned, the instance is made at the root and moved under it (its
      // transform stays local).
      if (parent == null || !parent.isPending) {
        go = instantiate(prefab, transform: transform, parent: parent, name: spec.name);
      } else {
        go = instantiate(prefab, transform: transform, name: spec.name)..setParent(parent);
      }
    } else {
      go = spawn(name: spec.name ?? '', transform: transform, parent: parent);
    }
    for (final c in spec.components) {
      go.set(c);
    }
    if (spec.layer != null) go.layer = spec.layer!;
    for (final t in spec.tags) {
      go.addTag(t);
    }
    for (final b in spec.behaviours) {
      go.addBehaviour(b);
    }
    for (final c in spec.children) {
      _buildSpec(c, go, c.transform);
    }
    if (!spec.active) go.setActive(false);
    return go;
  }
}
