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
/// [TransformRef] or a [Behaviour].
abstract class Component extends FynkelObject {
  /// The object this component is on.
  GameObject get gameObject;

  /// The object's transform.
  TransformRef get transform => gameObject.transform;

  @override
  String get name => gameObject.name;

  /// A component of type [T] on the same object; see
  /// [GameObject.getComponent].
  T? getComponent<T extends Component>() => gameObject.getComponent<T>();
}

/// A project asset as an object (Unity's `Material`, `Mesh`,
/// `ScriptableObject`…): [ref] is the asset it stands for.
abstract class Asset extends FynkelObject {
  /// The asset in the project; null for one that exists only in code.
  AssetRef? get ref;
}
