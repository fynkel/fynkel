part of 'world.dart';

/// The base of everything a game works with (Unity's `UnityEngine.Object`,
/// ADR-0036 §1): a [GameObject], a [Component] on one (its transform, a
/// script) and an [Asset]. Two variables hold the same object only when
/// they are the same instance; nothing compares by value.
abstract class FynkelObject {
  /// The object's name; a component answers its object's.
  String get name;

  /// False once destroyed (an object, a component removed from its
  /// object) or never attached (a script built but not added yet). Using a
  /// destroyed object throws a `StateError`.
  bool get isAlive;

  /// Destroys the object (Unity's `Destroy`): a [GameObject] with its
  /// subtree, a component off its object, an asset code made at runtime.
  /// Some objects refuse (a transform, an imported asset).
  void destroy();
}

/// Something on one [GameObject] (Unity's `Component`): its
/// [Transform], a [Behaviour], or a built-in [NativeComponent].
abstract class Component extends FynkelObject {
  /// The object this component is on.
  GameObject get gameObject;

  /// The object's transform.
  Transform get transform => gameObject.transform;

  @override
  String get name => gameObject.name;

  /// A component of type [T] on the same object; see
  /// [GameObject.getComponent].
  T? getComponent<T extends Component>() => gameObject.getComponent<T>();
}

/// A built-in component on one object as a live handle (Unity's
/// `Rigidbody`, `Light`…, ADR-0036 §2): `go.component<RigidBody>()`, or a
/// script field `NativeComponent<RigidBody>? body` set in the Inspector.
/// [value] reads and writes the component's record the way
/// [GameObject.get] and [GameObject.set] do (read-your-writes). One
/// handle per component: asking again returns the same one.
final class NativeComponent<T extends WireRecord> extends Component {
  NativeComponent._(this._go);
  final GameObject _go;

  @override
  GameObject get gameObject => _go;

  /// The component's value now; throws once it is gone (removed, or its
  /// object destroyed).
  T get value {
    final v = _go.isAlive ? _go.get<T>() : null;
    if (v == null) throw StateError('the $T of "${_go.name}" was removed or destroyed');
    return v;
  }

  /// Writes the component for the next tick (readable now).
  set value(T v) => _go.set<T>(v);

  /// Its object is alive and still has the component.
  @override
  bool get isAlive => _go.isAlive && _go.has<T>();

  /// Removes the component from its object (Unity's `Destroy(component)`).
  @override
  void destroy() => _go.remove<T>();

  @override
  String toString() => '$T on ${_go.name}';
}

/// A project asset as an object (Unity's `Material`, `Mesh`,
/// `ScriptableObject`…): [ref] is the asset it stands for.
abstract class Asset extends FynkelObject {
  /// The asset in the project; null for one that exists only in code.
  AssetRef? get ref;
}

/// A project asset of one kind (ADR-0036 §3): what `Assets.materials.gold`
/// and a field `MaterialAsset? highlight` hold. It is also the [AssetRef]
/// it names, so every API that takes an `AssetRef` takes it, and it equals
/// a plain `AssetRef` of the same asset. [kind] is the word a field's
/// `asset(<kind>)` hint and the Inspector's picker use, so a typed field
/// only takes that kind of asset. The names carry `Asset` because Flutter
/// and `dart:ui` already own `Material`, `Texture`, `Shader` and `Scene`.
abstract class TypedAsset extends AssetRef implements Asset {
  // A private field cannot be a named initializing formal.
  // ignore: prefer_initializing_formals
  const TypedAsset(super.asset, {super.sub, String? name}) : _name = name;

  final String? _name;

  /// `material`, `mesh`, `texture`, …
  String get kind;

  /// The file's name without its extension, or the sub-asset's
  /// (`mesh/cube` → `cube`), as `fynkel codegen` wrote it; else the id.
  @override
  String get name => _name ?? (sub == null ? asset : sub!.substring(sub!.indexOf('/') + 1));

  @override
  AssetRef get ref => this;

  @override
  bool get isAlive => !isNone;

  /// Refused for a project asset, as Unity refuses; one made in code
  /// (`world.createMaterial`, `createMesh`, `createClip`) is destroyed.
  @override
  void destroy() => throw UnsupportedError('$name is a project asset; only one made in code can be destroyed');

  @override
  String toString() => '$runtimeType($name, ${sub == null ? asset : '$asset#$sub'})';
}

/// A mesh: a model's `mesh/<name>` or one `world.createMesh` made.
final class MeshAsset extends TypedAsset {
  const MeshAsset(super.asset, {super.sub, super.name});
  MeshAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'mesh';
}

/// A material: a `.material.json`, a model's `material/<name>`, or one
/// `world.createMaterial` made.
final class MaterialAsset extends TypedAsset {
  const MaterialAsset(super.asset, {super.sub, super.name});
  MaterialAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'material';
}

/// A texture: an image, or a model's `image/<name>`.
final class TextureAsset extends TypedAsset {
  const TextureAsset(super.asset, {super.sub, super.name});
  TextureAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'texture';
}

/// A prefab: a `.prefab.json`, or a model's hierarchy (`prefab`).
final class PrefabAsset extends TypedAsset {
  const PrefabAsset(super.asset, {super.sub, super.name});
  PrefabAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'prefab';
}

/// A scene (`.scene.json`).
final class SceneAsset extends TypedAsset {
  const SceneAsset(super.asset, {super.sub, super.name});
  SceneAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'scene';
}

/// A sound (Unity's `AudioClip`): an audio file.
final class AudioClipAsset extends TypedAsset {
  const AudioClipAsset(super.asset, {super.sub, super.name});
  AudioClipAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'audio';
}

/// An animation clip: a model's `anim/<name>`, or one `world.createClip`
/// made.
final class AnimationClipAsset extends TypedAsset {
  const AnimationClipAsset(super.asset, {super.sub, super.name});
  AnimationClipAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'clip';
}

/// A skin: a model's `skin/<name>`.
final class SkinAsset extends TypedAsset {
  const SkinAsset(super.asset, {super.sub, super.name});
  SkinAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'skin';
}

/// A shader (`.shader`, ADR-0031).
final class ShaderAsset extends TypedAsset {
  const ShaderAsset(super.asset, {super.sub, super.name});
  ShaderAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'shader';
}

/// An Animator Controller (`.controller.json`, ADR-0032).
final class AnimatorControllerAsset extends TypedAsset {
  const AnimatorControllerAsset(super.asset, {super.sub, super.name});
  AnimatorControllerAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'controller';
}

/// A post-processing profile (`.volume.json`).
final class VolumeProfileAsset extends TypedAsset {
  const VolumeProfileAsset(super.asset, {super.sub, super.name});
  VolumeProfileAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'volume';
}

/// A physics material (`.physicsmaterial.json`, ADR-0025).
final class PhysicsMaterialAsset extends TypedAsset {
  const PhysicsMaterialAsset(super.asset, {super.sub, super.name});
  PhysicsMaterialAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'physics_material';
}

/// A rig (`.rig.json`): a character assembled from models.
final class RigAsset extends TypedAsset {
  const RigAsset(super.asset, {super.sub, super.name});
  RigAsset.fromRef(AssetRef r) : this(r.asset, sub: r.sub);
  @override
  String get kind => 'rig';
}

/// A material [World.createMaterial] or [World.createShaderMaterial] made:
/// alive until destroyed.
final class _MadeMaterial extends MaterialAsset {
  _MadeMaterial(this._world, AssetRef id) : super(id.asset, name: 'Made in code');
  final World _world;

  @override
  bool get isAlive =>
      !_world.isDisposed && (_world.runtimeMaterial(this) != null || _world._shaderMaterial(asset) != null);

  @override
  void destroy() => _world.destroyMaterial(this);
}

/// A mesh [World.createMesh] made.
final class _MadeMesh extends MeshAsset {
  _MadeMesh(this._world, AssetRef id) : super(id.asset, name: 'Made in code');
  final World _world;

  @override
  bool get isAlive => !_world.isDisposed && _world.runtimeMesh(this) != null;

  @override
  void destroy() => _world.destroyMesh(this);
}

/// A clip [World.createClip] made.
final class _MadeClip extends AnimationClipAsset {
  _MadeClip(this._world, AssetRef id) : super(id.asset, name: 'Made in code');
  final World _world;

  @override
  bool get isAlive => !_world.isDisposed && _world._hasRuntimeClip(asset);

  @override
  void destroy() => _world.destroyClip(this);
}
