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
