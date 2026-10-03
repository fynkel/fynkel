// Objects described in code (ADR-0026 §2): an `ObjectSpec` is a tree —
// name, transform, active, tags, layer, native components, behaviours,
// children, or an instance of a prefab asset — and `World.build` spawns it.
// A function returning a spec is a code prefab: call it once per copy.
// The same description is what `package:fynkel/authoring.dart` writes as
// a scene or prefab document (ADR-0026 §2, step 6).

part of 'world.dart';

/// A reusable factory for fresh object descriptions and behaviour instances.
///
/// Call [create] for each use, then pass its result to `world.build` or the
/// authoring writer. Internal references must be assembled inside the factory:
/// each invocation gets its own placeholders. Captured live objects deliberately
/// remain external references. Reusing a spec node or script is rejected before
/// a description can be built or written.
final class ObjectRecipe {
  ObjectRecipe(this._factory);

  final ObjectSpec Function() _factory;
  static final Expando<bool> _issued = Expando<bool>('ObjectRecipe issued objects');

  ObjectSpec create() {
    final root = _factory();
    final nodes = Set<Object>.identity();
    void visit(ObjectSpec spec) {
      final previous = ObjectSpec._objects[spec];
      if (!nodes.add(spec) || _issued[spec] == true || (previous != null && previous._spec == null)) {
        throw StateError('ObjectRecipe must create fresh, non-cyclic ObjectSpec nodes on every call.');
      }
      for (final behaviour in spec.behaviours) {
        if (!nodes.add(behaviour) || _issued[behaviour] == true || behaviour._system != null) {
          throw StateError('ObjectRecipe must create a fresh Behaviour for each object and call.');
        }
      }
      for (final child in spec.children) {
        visit(child);
      }
    }

    visit(root);
    for (final node in nodes) {
      _issued[node] = true;
    }
    return root;
  }
}

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
    this.data = const [],
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
    this.data = const [],
    this.behaviours = const [],
    this.children = const [],
  });

  /// A built-in shape (Unity's `GameObject.CreatePrimitive`): [shape]'s
  /// mesh with the default material and Unity's collider for it, named
  /// after the shape unless [name] says otherwise. [components] come after
  /// those two, so a `MeshRenderer` or `Collider` among them replaces the
  /// shape's (`MeshRenderer(mesh: Primitive.cube.mesh, material: mine)`).
  factory ObjectSpec.primitive(
    Primitive shape, {
    String? name,
    Pose transform = identityTransform,
    bool active = true,
    Iterable<String> tags = const [],
    String? layer,
    List<WireRecord> components = const [],
    List<ComponentData<dynamic>> data = const [],
    List<Behaviour> behaviours = const [],
    List<ObjectSpec> children = const [],
  }) => ObjectSpec(
    name ?? shape.label,
    transform: transform,
    active: active,
    tags: tags,
    layer: layer,
    components: [
      gen.MeshRenderer(mesh: shape.mesh, material: Primitive.defaultMaterial),
      shape.collider,
      ...components,
    ],
    data: data,
    behaviours: behaviours,
    children: children,
  );

  /// The object's name; null only for a prefab instance keeping the
  /// prefab's.
  final String? name;

  /// The prefab this is an instance of, or null for a new object.
  final AssetRef? prefab;

  final Pose transform;
  final bool active;
  final Iterable<String> tags;
  final String? layer;
  final List<WireRecord> components;
  final List<ComponentData<dynamic>> data;

  /// Attached as they are: a behaviour instance belongs to one object, so a
  /// spec with behaviours builds once — make it in a function to build it
  /// again.
  final List<Behaviour> behaviours;
  final List<ObjectSpec> children;

  /// The object this spec builds, to reference before it exists (ADR-0036
  /// §4): `Switch()..door = gate.object` in another spec. Until the spec
  /// is built it is a stand-in only a script field or a component value
  /// (`Follow(target: hero.object)`) can hold, and [World.build] makes
  /// those the built object; once built, this is the object the latest
  /// build made. Captured handles keep their identity across later builds; use
  /// a fresh spec tree per copy when its internal references must follow the copy.
  GameObject get object {
    final edit = Zone.current[_gameplayZoneKey] as _GameplayAdapter?;
    if (edit != null) {
      edit.checkOpen();
      final staged = edit._specObjects[this];
      if (staged != null) return staged;
    }
    return _objects[this] ??= GameObject._placeholder(this);
  }

  // Only the first build consumes the placeholder. Existing references stay
  // pinned to their object when this description is built again.
  GameObject? get _unboundObject {
    final handle = _objects[this] ??= GameObject._placeholder(this);
    return handle._spec != null ? handle : null;
  }

  static final _objects = Expando<GameObject>('ObjectSpec.object');

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

/// The built-in shapes (ADR-0023), the editor's GameObject → 3D Object
/// menu. Every cook ships them (`Fynkel/Primitives.glb`, fixed ids),
/// whether or not the project installed its own copy.
enum Primitive {
  cube('Cube', gen.Collider(shape: gen.ColliderShape.box, size: Vec3(0.5, 0.5, 0.5))),
  sphere('Sphere', gen.Collider(shape: gen.ColliderShape.sphere, size: Vec3(0.5, 0.5, 0.5))),
  // A capsule collider for both, as Unity: radius 0.5, 2 m tall.
  capsule('Capsule', gen.Collider(shape: gen.ColliderShape.capsule, size: Vec3(0.5, 0.5, 0.5))),
  cylinder('Cylinder', gen.Collider(shape: gen.ColliderShape.capsule, size: Vec3(0.5, 0.5, 0.5))),
  plane(
    'Plane',
    gen.Collider(
      shape: gen.ColliderShape.mesh,
      mesh: AssetRef(Primitive.id, sub: 'mesh/Plane'),
    ),
  ),
  quad(
    'Quad',
    gen.Collider(
      shape: gen.ColliderShape.mesh,
      mesh: AssetRef(Primitive.id, sub: 'mesh/Quad'),
    ),
  );

  const Primitive(this.label, this.collider);

  /// The shapes' asset (`Fynkel/Primitives.glb`).
  static const id = 'f7a1e5d0-0000-4000-8000-000000000023';

  /// The material every shape starts with.
  static const defaultMaterial = AssetRef(id, sub: 'material/Default');

  /// Unity's name for it, the new object's name.
  final String label;

  /// Unity's collider for the shape: a 1 m box, a 0.5 m sphere, a 2 m
  /// capsule for the capsule and the cylinder, the mesh for the plane
  /// (10 m) and the quad (1 m).
  final gen.Collider collider;

  AssetRef get mesh => AssetRef(id, sub: 'mesh/$label');
}

extension on World {
  /// Checks the whole tree before anything spawns, so a mistake leaves no
  /// half-built object behind.
  void _checkSpecs(Iterable<ObjectSpec> roots) {
    final seen = Set<Behaviour>.identity();
    final specs = Set<ObjectSpec>.identity();
    final twice = Set<ObjectSpec>.identity();
    for (final s in roots.expand((root) => root._all)) {
      if (!specs.add(s)) twice.add(s);
      for (final b in s.behaviours) {
        if (b._system != null) {
          throw StateError(
            '$s: $b is already attached to ${b._gameObject} — build a spec with behaviours once, '
            'or make it in a function that creates them',
          );
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
    // What the scripts reference: a spec's object in this build (once) or
    // built before, a script in this build or attached already.
    for (final s in specs) {
      for (final data in s.data) {
        for (final target in data.type.encode(data.value).whereType<GameObject>()) {
          if (target._spec != null && twice.contains(target._spec)) {
            throw StateError('ambiguous component reference: spec occurs twice');
          }
        }
      }
      for (final b in s.behaviours) {
        final type = b._type ?? _registeredTypeOf(b);
        for (final f in type?.fields ?? const <BehaviourField>[]) {
          if (!f.isReference) continue;
          switch (f.target?.call(b)) {
            case final GameObject g when g._spec != null:
              final target = g._spec;
              if (twice.contains(target)) {
                throw StateError('$s: ${type!.id}.${f.name} references $target, which is twice in this build');
              }
              if (!specs.contains(target) && !(_gameplay?._specObjects.containsKey(target) ?? false)) {
                throw StateError(
                  '$s: ${type!.id}.${f.name} references $target, which is not in this build '
                  'and was never built',
                );
              }
            case final Behaviour r
                when r._system == null && !seen.contains(r) && !(_gameplay?._attachments.containsKey(r) ?? false):
              throw StateError('$s: ${type!.id}.${f.name} holds $r, which is not in this build and not attached');
          }
        }
      }
    }
  }

  /// Fills a tree after all of its journal references have been allocated.
  void _fillSpec(ObjectSpec spec, Iterator<GameObject> made) {
    final go = made.current;
    for (final c in spec.components) {
      go.set(c);
    }
    for (final data in spec.data) {
      data.apply(go);
    }
    if (spec.layer != null) go.layer = spec.layer!;
    for (final t in spec.tags) {
      go.addTag(t);
    }
    for (final b in spec.behaviours) {
      go.addBehaviour(b);
    }
    for (final c in spec.children) {
      made.moveNext();
      _fillSpec(c, made);
    }
    if (!spec.active) go.setActive(false);
  }
}
