// Display hints for the built-in components (ADR-0018), keyed by type and
// field, in the `x-fynkel` vocabulary the behaviour generator writes —
// `label`, `tooltip`, `group`, `order`, `header`, `unit`, `kind`,
// `readOnly`, `hidden`, `showIf`, `min`, `max`, `step`, and ADR-0019's
// `gizmo` (a Scene-view gizmo for the field, item 9h). The generated
// component schemas carry types, bounds and descriptions but no display
// hints yet (the Rust `#[fynkel(...)]` field attributes are the M3 stream's
// shared-crate work); until they land, this table stands in underneath
// the schema: whatever a schema's `x-fynkel` says wins over an entry here,
// so the table only ever shrinks. Every kind and unit below is the one the
// component's documentation states (docs/components.md, physics.md,
// rendering.md).
//
// Keep this in step with `schemas/components/*.json`: `inspector_test.dart`
// fails when an entry names a type or a field that no schema has.

const Map<String, Map<String, Map<String, Object?>>> nativeFieldHints = {
  'fynkel.Transform': {
    'position': {'unit': 'm', 'order': 0},
    'rotation': {
      'kind': 'quaternion',
      'order': 1,
      'tooltip': 'Edited as Euler degrees (X, Y, Z); stored as a unit quaternion.',
    },
    'scale': {'order': 2},
  },
  'fynkel.Camera': {
    'projection': {'group': 'Lens', 'order': 0},
    'fov_y': {
      'label': 'Field of view',
      'kind': 'angle',
      'unit': 'deg',
      'min': 0.01,
      'max': 3.1,
      'group': 'Lens',
      'order': 1,
    },
    'ortho_height': {'label': 'Orthographic height', 'unit': 'm', 'min': 0, 'group': 'Lens', 'order': 2},
    'near': {'unit': 'm', 'min': 0, 'group': 'Clipping', 'order': 3},
    'far': {'unit': 'm', 'min': 0, 'group': 'Clipping', 'order': 4},
    'priority': {'group': 'Output', 'order': 5},
    'exposure_mode': {'group': 'Exposure', 'order': 6},
    'exposure': {'label': 'Exposure (EV100)', 'group': 'Exposure', 'order': 7, 'step': 0.1},
  },
  'fynkel.DirectionalLight': {
    'color': {'kind': 'color', 'order': 0},
    'intensity': {'unit': 'lux', 'min': 0, 'order': 1},
    'cast_shadows': {'order': 2},
  },
  'fynkel.PointLight': {
    'color': {'kind': 'color', 'order': 0},
    'intensity': {'unit': 'cd', 'min': 0, 'order': 1},
    'range': {'unit': 'm', 'min': 0, 'order': 2},
  },
  'fynkel.SpotLight': {
    'color': {'kind': 'color', 'order': 0},
    'intensity': {'unit': 'cd', 'min': 0, 'order': 1},
    'range': {'unit': 'm', 'min': 0, 'order': 2},
    'inner_angle': {'kind': 'angle', 'unit': 'deg', 'min': 0, 'max': 1.5707963, 'group': 'Cone', 'order': 3},
    'outer_angle': {'kind': 'angle', 'unit': 'deg', 'min': 0, 'max': 1.5707963, 'group': 'Cone', 'order': 4},
  },
  // Audio (M3-11): the clip is the whole source (a `.wav`/`.ogg`, no sub-asset).
  'fynkel.AudioSource': {
    'clip': {'kind': 'asset(sound)', 'order': 0},
    'bus': {'order': 1, 'tooltip': 'The bus this source plays through: master, music or sfx.'},
    'volume': {'kind': 'slider', 'min': 0, 'max': 1, 'step': 0.01, 'order': 2},
    'pitch': {'min': 0.05, 'max': 8, 'step': 0.01, 'order': 3, 'tooltip': 'Playback rate; 1 is the clip\'s own rate.'},
    'looping': {'group': 'Playback', 'order': 4},
    'playing': {'group': 'Playback', 'order': 5},
    'priority': {
      'group': 'Playback',
      'order': 6,
      'tooltip': 'Past the voice limit a new sound steals the lowest-priority voice at or below its own.',
    },
  },
  'fynkel.AudioListener': {
    'volume': {'kind': 'slider', 'min': 0, 'max': 1, 'step': 0.01},
  },
  'fynkel.Environment': {
    'ambient_color': {'kind': 'color', 'group': 'Ambient', 'order': 0},
    'ambient_intensity': {'unit': 'lux', 'min': 0, 'group': 'Ambient', 'order': 1},
    'sky': {'kind': 'asset(texture)', 'group': 'Sky', 'order': 2},
    'sky_intensity': {'unit': 'nit', 'min': 0, 'group': 'Sky', 'order': 3},
    'sky_rotation': {'kind': 'angle', 'unit': 'deg', 'group': 'Sky', 'order': 4},
  },
  // Volumes (R3a, docs/renderer.md § Volumes): a profile applied by where
  // the camera stands.
  'fynkel.Volume': {
    'profile': {'kind': 'asset(volume)', 'order': 0},
    'shape': {'group': 'Shape', 'order': 1, 'tooltip': 'global: everywhere; box and sphere fade out over the blend distance.'},
    'size': {'label': 'Box size', 'group': 'Shape', 'order': 2},
    'radius': {'label': 'Sphere radius', 'min': 0, 'group': 'Shape', 'order': 3},
    'blend_distance': {'unit': 'm', 'min': 0, 'group': 'Shape', 'order': 4},
    'weight': {'kind': 'slider', 'min': 0, 'max': 1, 'step': 0.01, 'order': 5},
    'priority': {'order': 6, 'tooltip': 'Higher priorities blend over lower ones; ties go by scene order.'},
    'enabled': {'order': 7},
  },
  'fynkel.MeshRenderer': {
    'mesh': {'kind': 'asset(mesh)', 'order': 0},
    'material': {'kind': 'asset(material)', 'order': 1},
    'cast_shadows': {'group': 'Shadows', 'order': 2},
    'receive_shadows': {'group': 'Shadows', 'order': 3},
    'visible': {'order': 4},
  },
  'fynkel.Skin': {
    'skin': {'kind': 'asset(skin)', 'order': 0},
    'skeleton': {'kind': 'entity', 'order': 1},
  },
  'fynkel.Animator': {
    'clip': {'kind': 'asset(clip)', 'order': 0},
    'speed': {'step': 0.05, 'order': 1},
    'playing': {'order': 2},
    'looping': {'order': 3},
    'weight': {'kind': 'slider', 'min': 0, 'max': 1, 'step': 0.01, 'group': 'Blending', 'order': 4},
    'additive': {'group': 'Blending', 'order': 5},
    'root_motion': {'label': 'Root motion', 'order': 6},
  },
  'fynkel.RigidBody': {
    'body_type': {'label': 'Type', 'order': 0},
    'mass': {'unit': 'kg', 'min': 0, 'order': 1},
    'linear_damping': {'unit': '/s', 'min': 0, 'group': 'Damping', 'order': 2},
    'angular_damping': {'unit': '/s', 'min': 0, 'group': 'Damping', 'order': 3},
    'gravity_scale': {'step': 0.1, 'order': 4},
    'ccd': {'label': 'Continuous collision detection', 'order': 5},
  },
  'fynkel.Collider': {
    'shape': {'order': 0},
    'size': {'unit': 'm', 'order': 1},
    'mesh': {'kind': 'asset(mesh)', 'order': 2},
    'offset': {'unit': 'm', 'order': 3},
    'friction': {'min': 0, 'step': 0.05, 'group': 'Material', 'order': 4},
    'restitution': {'kind': 'slider', 'min': 0, 'max': 1, 'step': 0.01, 'group': 'Material', 'order': 5},
    'is_sensor': {'label': 'Sensor', 'order': 6},
    'layer': {'kind': 'layerMask', 'group': 'Layers', 'order': 7},
    'mask': {'kind': 'layerMask', 'group': 'Layers', 'order': 8},
  },
  'fynkel.CharacterController': {
    'radius': {'unit': 'm', 'min': 0, 'group': 'Capsule', 'order': 0},
    'height': {'unit': 'm', 'min': 0, 'group': 'Capsule', 'order': 1},
    'center': {'unit': 'm', 'group': 'Capsule', 'order': 2},
    'skin_width': {'unit': 'm', 'min': 0, 'group': 'Capsule', 'order': 3},
    'max_slope': {'kind': 'angle', 'unit': 'deg', 'min': 0, 'max': 1.5707963, 'group': 'Ground', 'order': 4},
    'max_step_height': {'unit': 'm', 'min': 0, 'group': 'Ground', 'order': 5},
    'snap_to_ground': {'unit': 'm', 'min': 0, 'group': 'Ground', 'order': 6},
    'gravity_scale': {'step': 0.1, 'group': 'Ground', 'order': 7},
    'mass': {'unit': 'kg', 'min': 0, 'group': 'Ground', 'order': 8},
    'speed': {'unit': 'm/s', 'min': 0, 'group': 'Driver', 'order': 9},
    'jump_speed': {'unit': 'm/s', 'min': 0, 'group': 'Driver', 'order': 10},
    'turn_speed': {'unit': 'rad/s', 'min': 0, 'group': 'Driver', 'order': 11},
    'move_action': {'group': 'Driver', 'order': 12},
    'jump_action': {'group': 'Driver', 'order': 13},
    'layer': {'kind': 'layerMask', 'group': 'Layers', 'order': 14},
    'mask': {'kind': 'layerMask', 'group': 'Layers', 'order': 15},
  },
  'fynkel.ThirdPersonCamera': {
    'target': {
      'kind': 'entity',
      'order': 0,
      'gizmo': {'shape': 'lineTo'},
    },
    'pivot': {'unit': 'm', 'order': 1},
    'distance': {'unit': 'm', 'min': 0, 'group': 'Orbit', 'order': 2},
    'min_distance': {'unit': 'm', 'min': 0, 'group': 'Orbit', 'order': 3},
    'yaw': {'kind': 'angle', 'unit': 'deg', 'group': 'Orbit', 'order': 4},
    'pitch': {'kind': 'angle', 'unit': 'deg', 'group': 'Orbit', 'order': 5},
    'min_pitch': {'kind': 'angle', 'unit': 'deg', 'group': 'Orbit', 'order': 6},
    'max_pitch': {'kind': 'angle', 'unit': 'deg', 'group': 'Orbit', 'order': 7},
    'look_action': {'group': 'Input', 'order': 8},
    'sensitivity': {'unit': 'rad/s', 'min': 0, 'group': 'Input', 'order': 9},
    'collision_radius': {'unit': 'm', 'min': 0, 'group': 'Collision', 'order': 10},
    'mask': {'kind': 'layerMask', 'group': 'Collision', 'order': 11},
  },
  'fynkel.Follow': {
    'target': {
      'kind': 'entity',
      'order': 0,
      'gizmo': {'shape': 'lineTo'},
    },
    'offset': {'unit': 'm', 'order': 1},
    'smoothing': {'unit': '/s', 'min': 0, 'order': 2},
    'look_at': {'order': 3},
  },
  'fynkel.LookAtCamera': {
    'lock_y': {'label': 'Lock Y axis'},
  },
  'fynkel.Rotator': {
    'axis': {'order': 0},
    'speed': {'unit': 'rad/s', 'step': 0.1, 'order': 1},
    'space': {'order': 2},
  },
  'fynkel.Oscillate': {
    'axis': {'order': 0},
    'amplitude': {'unit': 'm', 'min': 0, 'order': 1},
    'frequency': {'unit': 'Hz', 'min': 0, 'order': 2},
    'phase': {'kind': 'angle', 'unit': 'deg', 'order': 3},
    'space': {'order': 4},
  },
  'fynkel.Emitter': {
    'prefab': {'kind': 'asset(prefab)', 'order': 0},
    'rate': {'unit': '/s', 'min': 0, 'order': 1},
    'lifetime': {'unit': 's', 'min': 0, 'order': 2},
    'max_alive': {'label': 'Max alive', 'order': 3},
    // Instances launch at it in world space (docs/catalogue.md).
    'velocity': {
      'unit': 'm/s',
      'order': 4,
      'gizmo': {'shape': 'direction', 'space': 'world'},
    },
    'enabled': {'order': 5},
  },
  'fynkel.LifetimeDespawn': {
    'seconds': {'unit': 's', 'min': 0},
  },
  'fynkel.TriggerZone': {
    'event': {'order': 0},
    'once': {'order': 1},
  },
  'fynkel.Name': {
    'value': {'readOnly': true},
  },
};
