part of 'world.dart';

/// A shape to draw. Each built-in shape is one [Mesh], so `Mesh.cube()`
/// always returns the same object and a world uploads it once.
final class Mesh {
  Mesh._(this._shape, this._name);

  /// The engine's `FY_MESH_*` number.
  final int _shape;
  final String _name;

  /// A 1×1×1 cube around the origin.
  factory Mesh.cube() => _cube;

  /// A sphere of radius 0.5.
  factory Mesh.sphere() => _sphere;

  /// A 1×1 plane facing +Y.
  factory Mesh.plane() => _plane;

  static final _cube = Mesh._(FY_MESH_CUBE, 'cube');
  static final _sphere = Mesh._(FY_MESH_SPHERE, 'sphere');
  static final _plane = Mesh._(FY_MESH_PLANE, 'plane');

  @override
  String toString() => 'Mesh.$_name()';
}

/// How a surface looks under light. A material can be used in any number
/// of worlds, and changing it changes every object that uses it, in every
/// world.
final class Material {
  Material({this._color = Color.white, double metallic = 0, double roughness = 0.5})
    : _metallic = _unit(metallic, 'metallic'),
      _roughness = _unit(roughness, 'roughness');

  Color _color;
  double _metallic;
  double _roughness;

  /// The worlds that have this material. Weak, so a material kept in a
  /// global doesn't keep every world it was used in alive.
  final List<WeakReference<World>> _worlds = [];

  /// The base colour (albedo).
  Color get color => _color;
  set color(Color value) {
    _color = value;
    _changed();
  }

  /// 0 for paint, plastic or wood; 1 for bare metal.
  double get metallic => _metallic;
  set metallic(double value) {
    _metallic = _unit(value, 'metallic');
    _changed();
  }

  /// 0 for a mirror finish, 1 for fully matte.
  double get roughness => _roughness;
  set roughness(double value) {
    _roughness = _unit(value, 'roughness');
    _changed();
  }

  void _changed() {
    _worlds.removeWhere((ref) => ref.target?._updateMaterial(this) != true);
  }

  // The engine checks again; checking here too reports the mistake where
  // it was made, even before the material is used in a world.
  static double _unit(double value, String name) {
    if (!(value >= 0 && value <= 1)) throw RangeError.value(value, name, 'must be between 0 and 1');
    return value;
  }

  @override
  String toString() => 'Material(color: $color, metallic: $metallic, roughness: $roughness)';
}
