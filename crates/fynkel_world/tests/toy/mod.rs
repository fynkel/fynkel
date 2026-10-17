//! A toy model for the animation tests: a body and its arm, and three
//! clips whose poses are easy to work out by hand.

#![allow(dead_code, unreachable_pub)]

use std::f32::consts::FRAC_PI_2;

use fynkel_world::*;

/// A `.glb` with two nodes, `body` (rest at z 5) and its child `arm`, and
/// three clips:
///
/// - `slide` (2 s, linear): the body from x 0 to 4, the arm from no turn
///   to 90° about z;
/// - `hop` (1 s, step): the body at y 1, then from 1 s at y 3;
/// - `grow` (2 s, cubic, flat tangents): the arm's scale from 1 to 3;
/// - `sway` (2 s, linear), there and back, so it loops smoothly: the
///   body from x 0 to 2 and back, the arm to 90° about z and back, and
///   its scale to 2 and back;
/// - `swing` (1 s, linear), the same way: the body to x -1, the arm to
///   -45°.
pub fn glb() -> Vec<u8> {
    let (s, c) = (FRAC_PI_2 / 2.0).sin_cos();
    let (s2, c2) = (-FRAC_PI_2 / 4.0).sin_cos();
    let accessors: [(&str, Vec<f32>); 14] = [
        ("SCALAR", vec![0.0, 2.0]),
        ("VEC3", vec![0.0, 0.0, 0.0, 4.0, 0.0, 0.0]),
        ("VEC4", vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, s, c]),
        ("SCALAR", vec![0.0, 1.0]),
        ("VEC3", vec![0.0, 1.0, 0.0, 0.0, 3.0, 0.0]),
        ("SCALAR", vec![0.0, 2.0]),
        ("VEC3", [[0.0; 3], [1.0; 3], [0.0; 3], [0.0; 3], [3.0; 3], [0.0; 3]].concat()),
        ("SCALAR", vec![0.0, 1.0, 2.0]),
        ("VEC3", vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ("VEC4", vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, s, c, 0.0, 0.0, 0.0, 1.0]),
        ("VEC3", vec![1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0]),
        ("SCALAR", vec![0.0, 0.5, 1.0]),
        ("VEC3", vec![0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ("VEC4", vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, s2, c2, 0.0, 0.0, 0.0, 1.0]),
    ];
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut access = Vec::new();
    for (i, (kind, values)) in accessors.iter().enumerate() {
        let size = match *kind {
            "SCALAR" => 1,
            "VEC3" => 3,
            _ => 4,
        };
        views.push(format!(r#"{{"buffer":0,"byteOffset":{},"byteLength":{}}}"#, bin.len(), values.len() * 4));
        let bounds = if *kind == "SCALAR" {
            let (min, max) = values.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
            format!(r#","min":[{min}],"max":[{max}]"#)
        } else {
            String::new()
        };
        access.push(format!(
            r#"{{"bufferView":{i},"componentType":5126,"count":{},"type":"{kind}"{bounds}}}"#,
            values.len() / size
        ));
        bin.extend(values.iter().flat_map(|v| v.to_le_bytes()));
    }
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"name":"toy","nodes":[0]}}],
        "nodes":[{{"name":"body","translation":[0,0,5],"children":[1]}},{{"name":"arm"}}],
        "buffers":[{{"byteLength":{}}}],"bufferViews":[{}],"accessors":[{}],
        "animations":[
          {{"name":"slide","samplers":[{{"input":0,"output":1}},{{"input":0,"output":2}}],
            "channels":[{{"sampler":0,"target":{{"node":0,"path":"translation"}}}},{{"sampler":1,"target":{{"node":1,"path":"rotation"}}}}]}},
          {{"name":"hop","samplers":[{{"input":3,"output":4,"interpolation":"STEP"}}],
            "channels":[{{"sampler":0,"target":{{"node":0,"path":"translation"}}}}]}},
          {{"name":"grow","samplers":[{{"input":5,"output":6,"interpolation":"CUBICSPLINE"}}],
            "channels":[{{"sampler":0,"target":{{"node":1,"path":"scale"}}}}]}},
          {{"name":"sway","samplers":[{{"input":7,"output":8}},{{"input":7,"output":9}},{{"input":7,"output":10}}],
            "channels":[{{"sampler":0,"target":{{"node":0,"path":"translation"}}}},{{"sampler":1,"target":{{"node":1,"path":"rotation"}}}},{{"sampler":2,"target":{{"node":1,"path":"scale"}}}}]}},
          {{"name":"swing","samplers":[{{"input":11,"output":12}},{{"input":11,"output":13}}],
            "channels":[{{"sampler":0,"target":{{"node":0,"path":"translation"}}}},{{"sampler":1,"target":{{"node":1,"path":"rotation"}}}}]}}
        ]}}"#,
        bin.len(),
        views.join(","),
        access.join(",")
    );
    let mut text = json.into_bytes();
    text.resize(text.len().next_multiple_of(4), b' ');
    bin.resize(bin.len().next_multiple_of(4), 0);
    let mut out = Vec::new();
    let total = 12 + 8 + text.len() + 8 + bin.len();
    for word in [0x4654_6C67, 2, total as u32, text.len() as u32, 0x4E4F_534A] {
        out.extend(u32::to_le_bytes(word));
    }
    out.extend(&text);
    out.extend(u32::to_le_bytes(bin.len() as u32));
    out.extend(u32::to_le_bytes(0x004E_4942));
    out.extend(&bin);
    out
}

pub const SLIDE: u32 = 0;
pub const HOP: u32 = 1;
pub const GROW: u32 = 2;
pub const SWAY: u32 = 3;
pub const SWING: u32 = 4;

pub struct Toy {
    pub world: World,
    pub root: ObjectId,
    pub body: ObjectId,
    pub arm: ObjectId,
    pub events: Vec<AnimationEvent>,
}

impl Toy {
    pub fn new() -> Self {
        let model = ModelData::from_glb(&glb()).unwrap();
        assert!(model.warnings().is_empty(), "{:?}", model.warnings());
        let mut world = World::new();
        let ids = world.add_model(&model).unwrap();
        let root = world.spawn_model(&model, &ids.meshes, &ids.materials, Transform::default()).unwrap();
        let body = world.find(root, "body").unwrap().unwrap();
        let arm = world.find(root, "arm").unwrap().unwrap();
        Self { world, root, body, arm, events: Vec::new() }
    }

    pub fn step(&mut self, dt: f32) {
        self.events.clear();
        self.world.animate(dt, &mut self.events);
    }

    pub fn body(&self) -> Vec3 {
        self.world.get::<Transform>(self.body).unwrap().position
    }

    pub fn arm(&self) -> Transform {
        self.world.get::<Transform>(self.arm).unwrap()
    }
}

pub fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

pub fn turn_z(degrees: f32) -> Quat {
    Quat::from_rotation_z(degrees.to_radians())
}

pub fn same_turn(a: Quat, b: Quat) -> bool {
    a.dot(b).abs() > 1.0 - 1e-6
}

