// Unity's GameObject → 3D Object / Light / Camera (ADR-0023): an object
// with the components and values Unity gives it. The 3D Objects use the
// built-in shapes the `installPrimitives` operation puts in the project
// (`Fynkel/Primitives.glb`, a fixed id), so their references are known
// before the file exists. The presets live here, not in any format: the
// result is an ordinary object anyone could have built by hand.

/// The fixed asset id of `Primitives.glb` (`PRIMITIVES_ID` in
/// fynkel_authoring).
const primitivesId = 'f7a1e5d0-0000-4000-8000-000000000023';

/// Where a preset is offered: its submenu under GameObject.
enum PresetGroup { object3d, light, camera }

enum GameObjectPreset {
  cube('Cube', PresetGroup.object3d),
  sphere('Sphere', PresetGroup.object3d),
  capsule('Capsule', PresetGroup.object3d),
  cylinder('Cylinder', PresetGroup.object3d),
  plane('Plane', PresetGroup.object3d),
  quad('Quad', PresetGroup.object3d),
  directionalLight('Directional Light', PresetGroup.light),
  pointLight('Point Light', PresetGroup.light),
  spotLight('Spot Light', PresetGroup.light),
  camera('Camera', PresetGroup.camera);

  const GameObjectPreset(this.label, this.group);

  /// The menu label and the new object's name, as Unity's.
  final String label;
  final PresetGroup group;

  /// Needs `Primitives.glb` in the project.
  bool get usesPrimitives => group == PresetGroup.object3d;

  /// The command id: `gameobject.cube`, `gameobject.directionalLight`.
  String get commandId => 'gameobject.$name';

  /// The components besides the Transform, as `(type, overrides)`: each
  /// override replaces that field of the component's defaults.
  List<(String, Map<String, Object?>)> get components => switch (this) {
    cube => _shape('Cube', {'shape': 'box', 'size': _half}),
    sphere => _shape('Sphere', {
      'shape': 'sphere',
      'size': [0.5, 0.5, 0.5],
    }),
    // A capsule collider for both, as Unity: radius 0.5, 2 m tall.
    capsule => _shape('Capsule', _capsule),
    cylinder => _shape('Cylinder', _capsule),
    plane => _shape('Plane', {'shape': 'mesh', 'mesh': _ref('mesh/Plane')}),
    quad => _shape('Quad', {'shape': 'mesh', 'mesh': _ref('mesh/Quad')}),
    directionalLight => [
      (
        'fynkel.DirectionalLight',
        {
          // Unity's default sun is warm white.
          'color': [1.0, 0.97, 0.9],
        },
      ),
    ],
    pointLight => [('fynkel.PointLight', const {})],
    spotLight => [('fynkel.SpotLight', const {})],
    camera => [('fynkel.Camera', const {})],
  };

  /// The Transform's fields that differ from identity.
  Map<String, Object?> get transform => switch (this) {
    // 50° down, turned 30° (Unity's (50, -30, 0) in Fynkel's −Z-forward
    // frame), 3 m up.
    directionalLight => {
      'position': [0.0, 3.0, 0.0],
      'rotation': [-0.408218, 0.23457, 0.109382, 0.875426],
    },
    // Pointing down (its −Z to −Y).
    spotLight => {
      'position': [0.0, 3.0, 0.0],
      'rotation': [-0.707107, 0.0, 0.0, 0.707107],
    },
    // Unity's (0, 1, −10) looking along +Z, mirrored for −Z forward.
    camera => {
      'position': [0.0, 1.0, 10.0],
    },
    _ => const {},
  };

  static const _half = [0.5, 0.5, 0.5];
  static const _capsule = {
    'shape': 'capsule',
    'size': [0.5, 0.5, 0.5],
  };

  static Map<String, Object?> _ref(String sub) => {'asset': primitivesId, 'sub': sub};

  static List<(String, Map<String, Object?>)> _shape(String mesh, Map<String, Object?> collider) => [
    ('fynkel.MeshRenderer', {'mesh': _ref('mesh/$mesh'), 'material': _ref('material/Default')}),
    ('fynkel.Collider', collider),
  ];

  static List<GameObjectPreset> of(PresetGroup group) => [
    for (final p in values)
      if (p.group == group) p,
  ];
}
