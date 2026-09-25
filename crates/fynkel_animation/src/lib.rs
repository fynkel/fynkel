//! The clip core (blueprint §19.2d item 19, `docs/animation.md`): one
//! representation of keyed motion that both a rigged character's walk and
//! a door's swing use. A [`Clip`] is a set of [`Channel`]s, each keying
//! one property (translation, rotation or scale) of one **named** node
//! under the entity that plays it; sampling a clip at a time gives a
//! [`Pose`] — per target entity, the properties the clip sets — and poses
//! blend, which is all a crossfade (and, later, a state machine or a
//! timeline) needs. A [`Skin`] is the extra a rigged mesh carries: which
//! named joints deform it and their inverse bind matrices; the joints
//! themselves are ordinary entities with ordinary transforms, so the same
//! clip core drives them and the renderer reads their interpolated world
//! matrices like any other.
//!
//! Targets are resolved by **name** (within the animator's subtree; for a
//! skin, within the skeleton root's) because prefab instances get fresh
//! entity ids: what the importer wrote into a clip stays valid for every
//! instance of the prefab. The cooked forms (`fynkel_cooked::payload`)
//! are converted into these types by the asset store (`fynkel_assets`).

use fynkel_contracts::Entity;
use fynkel_scene::Transform;
use glam::{Mat4, Quat, Vec3};

/// The property a channel keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    Translation,
    Rotation,
    Scale,
}

impl Property {
    /// Floats per value: 3 for a vector, 4 for a quaternion.
    pub const fn width(self) -> usize {
        match self {
            Property::Translation | Property::Scale => 3,
            Property::Rotation => 4,
        }
    }
}

/// How a channel is read between keys (glTF's three).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// The previous key's value until the next key.
    Step,
    /// Linear for vectors, spherical for rotations.
    Linear,
    /// Cubic Hermite with the authored tangents: each key stores
    /// in-tangent, value, out-tangent (rotations are normalized after).
    CubicSpline,
}

/// One keyed property of one named node.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    /// The node's name; resolved to an entity under the animator.
    pub target: String,
    pub property: Property,
    pub interpolation: Interpolation,
    /// Key times in seconds, strictly increasing, non-negative.
    pub times: Vec<f32>,
    /// `times.len() * key_floats()` floats.
    pub values: Vec<f32>,
}

impl Channel {
    /// Floats per key: the value, or three of them for a cubic spline.
    pub fn key_floats(&self) -> usize {
        match self.interpolation {
            Interpolation::CubicSpline => 3 * self.property.width(),
            _ => self.property.width(),
        }
    }

    fn validate(&self, what: &str) -> Result<(), String> {
        if self.times.is_empty() {
            return Err(format!("{what}: no keys"));
        }
        if self.values.len() != self.times.len() * self.key_floats() {
            return Err(format!(
                "{what}: {} values for {} keys",
                self.values.len(),
                self.times.len()
            ));
        }
        if !self.times[0].is_finite() || self.times[0] < 0.0 {
            return Err(format!("{what}: key times must be finite and >= 0"));
        }
        if self
            .times
            .windows(2)
            .any(|w| w[1] <= w[0] || !w[1].is_finite())
        {
            return Err(format!("{what}: key times must be strictly increasing"));
        }
        if self.values.iter().any(|v| !v.is_finite()) {
            return Err(format!("{what}: non-finite value"));
        }
        Ok(())
    }

    /// The key at or before `time` and the fraction towards the next:
    /// `(i, u)` with `u == 0` at or before the first key and at or after
    /// the last.
    fn locate(&self, time: f32) -> (usize, f32) {
        let n = self.times.len();
        if n == 1 || time <= self.times[0] {
            return (0, 0.0);
        }
        if time >= self.times[n - 1] {
            return (n - 1, 0.0);
        }
        // The last key at or before `time` (partition point is the first
        // key after it).
        let i = self.times.partition_point(|t| *t <= time) - 1;
        let span = self.times[i + 1] - self.times[i];
        ((i), ((time - self.times[i]) / span).clamp(0.0, 1.0))
    }

    /// The raw floats of key `i`'s value (the middle third of a cubic key).
    fn value_at(&self, i: usize) -> &[f32] {
        let w = self.property.width();
        let k = self.key_floats();
        let start = i * k
            + if self.interpolation == Interpolation::CubicSpline {
                w
            } else {
                0
            };
        &self.values[start..start + w]
    }

    fn in_tangent_at(&self, i: usize) -> &[f32] {
        let w = self.property.width();
        let start = i * self.key_floats();
        &self.values[start..start + w]
    }

    fn out_tangent_at(&self, i: usize) -> &[f32] {
        let w = self.property.width();
        let start = i * self.key_floats() + 2 * w;
        &self.values[start..start + w]
    }

    /// The channel's value at `time`, as up to four floats (`width()` of
    /// them are meaningful).
    pub fn sample(&self, time: f32) -> [f32; 4] {
        let (i, u) = self.locate(time);
        let w = self.property.width();
        let mut out = [0.0f32; 4];
        let last = self.times.len() - 1;
        match self.interpolation {
            Interpolation::Step => out[..w].copy_from_slice(self.value_at(i)),
            _ if u == 0.0 || i == last => out[..w].copy_from_slice(self.value_at(i)),
            Interpolation::Linear => {
                let a = self.value_at(i);
                let b = self.value_at(i + 1);
                if self.property == Property::Rotation {
                    let q = quat(a).slerp(quat(b), u);
                    out.copy_from_slice(&q.to_array());
                } else {
                    for k in 0..w {
                        out[k] = a[k] + (b[k] - a[k]) * u;
                    }
                }
            }
            Interpolation::CubicSpline => {
                let dt = self.times[i + 1] - self.times[i];
                let (v0, b0) = (self.value_at(i), self.out_tangent_at(i));
                let (v1, a1) = (self.value_at(i + 1), self.in_tangent_at(i + 1));
                let (u2, u3) = (u * u, u * u * u);
                let (h00, h10, h01, h11) = (
                    2.0 * u3 - 3.0 * u2 + 1.0,
                    u3 - 2.0 * u2 + u,
                    -2.0 * u3 + 3.0 * u2,
                    u3 - u2,
                );
                for k in 0..w {
                    out[k] = h00 * v0[k] + h10 * dt * b0[k] + h01 * v1[k] + h11 * dt * a1[k];
                }
                if self.property == Property::Rotation {
                    let q = quat(&out).normalize();
                    out.copy_from_slice(&q.to_array());
                }
            }
        }
        out
    }
}

fn quat(v: &[f32]) -> Quat {
    Quat::from_xyzw(v[0], v[1], v[2], v[3])
}

/// A named point in a clip's time (M3-19c): the world raises
/// `ANIMATION_MARKER` when an animator's clock crosses it.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    /// Seconds into the clip, within `[0, duration]`.
    pub time: f32,
    pub name: String,
}

/// A clip: keyed channels over named nodes and the clip's length.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub name: String,
    /// Seconds; the last key of the longest channel.
    pub duration: f32,
    pub channels: Vec<Channel>,
    /// In time order (ties keep their order).
    pub markers: Vec<Marker>,
}

impl Clip {
    /// Every channel well-formed and the duration covering them.
    pub fn validate(&self) -> Result<(), String> {
        if !self.duration.is_finite() || self.duration < 0.0 {
            return Err(format!("{}: duration must be finite and >= 0", self.name));
        }
        for (c, channel) in self.channels.iter().enumerate() {
            channel.validate(&format!("{} channel {c} ({})", self.name, channel.target))?;
            if *channel.times.last().unwrap() > self.duration {
                return Err(format!(
                    "{} channel {c} ({}): keys run past the duration",
                    self.name, channel.target
                ));
            }
        }
        for (m, marker) in self.markers.iter().enumerate() {
            if !marker.time.is_finite() || marker.time < 0.0 || marker.time > self.duration {
                return Err(format!(
                    "{} marker {m} ({}): time must be within the clip",
                    self.name, marker.name
                ));
            }
            if marker.name.is_empty() {
                return Err(format!("{} marker {m}: empty name", self.name));
            }
            if m > 0 && self.markers[m - 1].time > marker.time {
                return Err(format!(
                    "{} marker {m}: markers must be in time order",
                    self.name
                ));
            }
        }
        Ok(())
    }

    /// The markers a clock crossed going from `prev` to `next` seconds
    /// (unwrapped: a looping clock hands in `next` past the end or below
    /// zero and the crossings wrap), in the order they were met. Forward,
    /// a marker at `m` fires when `prev < m <= next`; backwards when
    /// `next <= m < prev` — so a marker on the first frame fires on every
    /// wrap of a forward loop and one on the last frame on every wrap of
    /// a backward one, never on the tick that set the clock.
    pub fn markers_crossed(&self, prev: f32, next: f32, looping: bool, out: &mut Vec<usize>) {
        let duration = self.duration.max(0.0);
        if self.markers.is_empty() || prev == next {
            return;
        }
        if !looping || duration <= 0.0 {
            if next > prev {
                out.extend(
                    (0..self.markers.len())
                        .filter(|&i| self.markers[i].time > prev && self.markers[i].time <= next),
                );
            } else {
                out.extend(
                    (0..self.markers.len())
                        .rev()
                        .filter(|&i| self.markers[i].time >= next && self.markers[i].time < prev),
                );
            }
            return;
        }
        // A looping clock: the crossings of `m + k * duration` for every
        // wrap `k` the interval covers, wrap by wrap so the order holds.
        if next > prev {
            let mut base = 0.0;
            while base < next {
                out.extend((0..self.markers.len()).filter(|&i| {
                    let t = self.markers[i].time + base;
                    t > prev && t <= next
                }));
                base += duration;
            }
        } else {
            let mut base = 0.0;
            while base + duration > next {
                out.extend((0..self.markers.len()).rev().filter(|&i| {
                    let t = self.markers[i].time + base;
                    t >= next && t < prev
                }));
                base -= duration;
            }
        }
    }

    /// Samples every channel whose target resolved (`targets[c]`, one per
    /// channel) into `pose`, merged by entity.
    pub fn sample(&self, time: f32, targets: &[Option<Entity>], pose: &mut Pose) {
        for (channel, target) in self.channels.iter().zip(targets) {
            let Some(entity) = target else {
                continue;
            };
            let v = channel.sample(time);
            let entry = pose.entry(*entity);
            match channel.property {
                Property::Translation => entry.translation = Some(Vec3::new(v[0], v[1], v[2])),
                Property::Rotation => entry.rotation = Some(quat(&v)),
                Property::Scale => entry.scale = Some(Vec3::new(v[0], v[1], v[2])),
            }
        }
    }
}

/// How the clips of a blend space share a clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BlendSync {
    /// The clips advance by normalized phase: the blend's clock runs over
    /// a duration interpolated between the two active clips', and each
    /// clip is sampled at the same fraction of its own length — a walk
    /// and a run keep their feet together.
    #[default]
    Phase,
}

/// One point of a blend space: the clip that plays alone when the
/// parameter is exactly `at`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendPoint {
    pub at: f32,
    pub clip: fynkel_contracts::AssetRef,
}

/// A 1D blend space (M3-19c, `<name>.blend.json`): clips placed along
/// one named parameter. An `Animator` plays it like a clip; the world
/// blends the two points around the parameter's value (clamped to the
/// first and last point) at the fraction between them, phase-synced.
#[derive(Clone, Debug, PartialEq)]
pub struct BlendSpace {
    pub name: String,
    /// The parameter the space reads (`SetParameter { name, .. }`).
    pub parameter: String,
    /// Sorted by `at`, strictly increasing.
    pub points: Vec<BlendPoint>,
    pub sync: BlendSync,
}

impl BlendSpace {
    /// The most points one blend space may have.
    pub const MAX_POINTS: usize = 16;

    pub fn validate(&self) -> Result<(), String> {
        if self.parameter.is_empty() {
            return Err(format!("{}: the parameter needs a name", self.name));
        }
        if self.points.is_empty() {
            return Err(format!(
                "{}: a blend space needs at least one point",
                self.name
            ));
        }
        if self.points.len() > Self::MAX_POINTS {
            return Err(format!(
                "{}: {} points; at most {} are supported",
                self.name,
                self.points.len(),
                Self::MAX_POINTS
            ));
        }
        for (i, p) in self.points.iter().enumerate() {
            if !p.at.is_finite() {
                return Err(format!("{} point {i}: `at` must be finite", self.name));
            }
            if p.clip.is_nil() {
                return Err(format!("{} point {i}: the clip is not set", self.name));
            }
            if i > 0 && self.points[i - 1].at >= p.at {
                return Err(format!(
                    "{} point {i}: points must be strictly increasing in `at`",
                    self.name
                ));
            }
        }
        Ok(())
    }

    /// The parameter's value the space plays for `value`: clamped to the
    /// first and last point.
    pub fn clamp(&self, value: f32) -> f32 {
        match (self.points.first(), self.points.last()) {
            (Some(a), Some(b)) => value.clamp(a.at, b.at),
            _ => value,
        }
    }

    /// The two points around `value` and the fraction from the first to
    /// the second: `(i, j, t)` with `i == j` and `t == 0` on a point (or
    /// past the ends, or with one point).
    pub fn segment(&self, value: f32) -> (usize, usize, f32) {
        let n = self.points.len();
        if n == 0 {
            return (0, 0, 0.0);
        }
        let value = self.clamp(value);
        let j = self.points.partition_point(|p| p.at < value);
        if j == 0 {
            return (0, 0, 0.0);
        }
        if j >= n {
            return (n - 1, n - 1, 0.0);
        }
        let (a, b) = (self.points[j - 1].at, self.points[j].at);
        if value == b {
            return (j, j, 0.0);
        }
        let t = ((value - a) / (b - a)).clamp(0.0, 1.0);
        (j - 1, j, t)
    }
}

/// What a sampled clip sets on one entity: each property only when a
/// channel keyed it (the entity's other properties are left alone).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseEntry {
    pub entity: Entity,
    pub translation: Option<Vec3>,
    pub rotation: Option<Quat>,
    pub scale: Option<Vec3>,
}

impl PoseEntry {
    /// The local transform after this entry is applied over `current`.
    pub fn apply_to(&self, current: Transform) -> Transform {
        Transform {
            position: self.translation.unwrap_or(current.position),
            rotation: self.rotation.unwrap_or(current.rotation),
            scale: self.scale.unwrap_or(current.scale),
        }
    }
}

/// The sampled state of a set of entities. Small (a rig's joints), kept
/// in entity order of first appearance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    pub entries: Vec<PoseEntry>,
}

impl Pose {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn entry(&mut self, entity: Entity) -> &mut PoseEntry {
        match self.entries.iter().position(|e| e.entity == entity) {
            Some(i) => &mut self.entries[i],
            None => {
                self.entries.push(PoseEntry {
                    entity,
                    translation: None,
                    rotation: None,
                    scale: None,
                });
                self.entries.last_mut().unwrap()
            }
        }
    }

    /// `self` blended towards `other` by `weight` (0 = `self`, 1 =
    /// `other`), written into `self`: a property both set is interpolated
    /// (rotations spherically), one only one side sets is taken as is.
    pub fn blend(&mut self, other: &Pose, weight: f32) {
        let w = weight.clamp(0.0, 1.0);
        for o in &other.entries {
            let e = self.entry(o.entity);
            e.translation = mix(e.translation, o.translation, w, |a, b| a.lerp(b, w));
            e.rotation = mix(e.rotation, o.rotation, w, |a, b| a.slerp(b, w));
            e.scale = mix(e.scale, o.scale, w, |a, b| a.lerp(b, w));
        }
    }
}

impl Pose {
    /// The entry for `entity`, if the pose sets anything on it.
    pub fn get(&self, entity: Entity) -> Option<&PoseEntry> {
        self.entries.iter().find(|e| e.entity == entity)
    }

    /// Every entry of `other` that has a counterpart in `reference` added
    /// to `self` as a **delta** from the reference (M3-19c, additive
    /// layers): translation `+ (o - r) * w`, rotation `slerp(1, o * r⁻¹,
    /// w) *`, scale `* lerp(1, o / r, w)`. An entity `self` does not set
    /// gets the delta over `base(entity)` — its current transform.
    pub fn add_delta(
        &mut self,
        other: &Pose,
        reference: &Pose,
        weight: f32,
        base: &mut dyn FnMut(Entity) -> Option<Transform>,
    ) {
        let w = weight.clamp(0.0, 1.0);
        if w == 0.0 {
            return;
        }
        for o in &other.entries {
            let Some(r) = reference.get(o.entity) else {
                continue;
            };
            let current = match self.get(o.entity) {
                Some(e) => Some(*e),
                None => base(o.entity).map(|t| PoseEntry {
                    entity: o.entity,
                    translation: Some(t.position),
                    rotation: Some(t.rotation),
                    scale: Some(t.scale),
                }),
            };
            let Some(current) = current else {
                continue;
            };
            let e = self.entry(o.entity);
            *e = current;
            if let (Some(ot), Some(rt), Some(t)) = (o.translation, r.translation, e.translation) {
                e.translation = Some(t + (ot - rt) * w);
            }
            if let (Some(oq), Some(rq), Some(q)) = (o.rotation, r.rotation, e.rotation) {
                let delta = (oq * rq.inverse()).normalize();
                e.rotation = Some((Quat::IDENTITY.slerp(delta, w) * q).normalize());
            }
            if let (Some(os), Some(rs), Some(sc)) = (o.scale, r.scale, e.scale) {
                let ratio = Vec3::new(
                    if rs.x != 0.0 { os.x / rs.x } else { 1.0 },
                    if rs.y != 0.0 { os.y / rs.y } else { 1.0 },
                    if rs.z != 0.0 { os.z / rs.z } else { 1.0 },
                );
                e.scale = Some(sc * Vec3::ONE.lerp(ratio, w));
            }
        }
    }
}

fn mix<T: Copy>(a: Option<T>, b: Option<T>, _w: f32, f: impl Fn(T, T) -> T) -> Option<T> {
    match (a, b) {
        (Some(a), Some(b)) => Some(f(a, b)),
        (Some(a), None) => Some(a),
        (None, b) => b,
    }
}

/// One joint of a skin.
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    /// The node's name; resolved to an entity under the skeleton root.
    pub name: String,
    /// Index of the parent joint within the skin, when it is one.
    pub parent: Option<u32>,
    /// Mesh space → this joint's bind space.
    pub inverse_bind: Mat4,
    /// The joint node's local transform in the bind pose.
    pub rest: Transform,
}

/// The joints a skinned mesh deforms with, in the order its vertices'
/// joint indices use; `skeleton` names the root node the joints are
/// searched under (empty: the mesh entity's tree root).
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    pub skeleton: String,
    pub joints: Vec<Joint>,
}

impl Skin {
    /// The most joints one skin may have: the renderer's palette
    /// (`docs/animation.md`; the importer's `max_joints`).
    pub const MAX_JOINTS: usize = 256;

    pub fn validate(&self) -> Result<(), String> {
        if self.joints.is_empty() {
            return Err("a skin needs at least one joint".into());
        }
        if self.joints.len() > Self::MAX_JOINTS {
            return Err(format!(
                "{} joints; at most {} are supported",
                self.joints.len(),
                Self::MAX_JOINTS
            ));
        }
        for (j, joint) in self.joints.iter().enumerate() {
            if let Some(p) = joint.parent {
                if p as usize >= self.joints.len() {
                    return Err(format!(
                        "joint {j} ({}): parent {p} out of range",
                        joint.name
                    ));
                }
            }
            if !joint.inverse_bind.is_finite() || !joint.rest.is_finite() {
                return Err(format!("joint {j} ({}): non-finite matrix", joint.name));
            }
            if self.joints[..j].iter().any(|o| o.name == joint.name) {
                return Err(format!(
                    "joint {j}: the name {:?} is used twice",
                    joint.name
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(
        interpolation: Interpolation,
        property: Property,
        times: &[f32],
        values: &[f32],
    ) -> Channel {
        Channel {
            target: "n".into(),
            property,
            interpolation,
            times: times.to_vec(),
            values: values.to_vec(),
        }
    }

    #[test]
    fn step_and_linear_sampling_clamp_at_the_ends() {
        let c = channel(
            Interpolation::Linear,
            Property::Translation,
            &[0.0, 1.0, 3.0],
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 2.0, 0.0],
        );
        assert_eq!(c.sample(-1.0)[..3], [0.0, 0.0, 0.0]);
        assert_eq!(c.sample(0.5)[..3], [0.5, 0.0, 0.0]);
        assert_eq!(c.sample(1.0)[..3], [1.0, 0.0, 0.0]);
        assert_eq!(c.sample(2.0)[..3], [1.0, 1.0, 0.0]);
        assert_eq!(c.sample(9.0)[..3], [1.0, 2.0, 0.0]);
        let s = channel(
            Interpolation::Step,
            Property::Scale,
            &[0.0, 1.0],
            &[1.0, 1.0, 1.0, 2.0, 2.0, 2.0],
        );
        assert_eq!(s.sample(0.99)[..3], [1.0, 1.0, 1.0]);
        assert_eq!(s.sample(1.0)[..3], [2.0, 2.0, 2.0]);
    }

    #[test]
    fn rotations_interpolate_spherically_and_splines_use_tangents() {
        let a = Quat::IDENTITY;
        let b = Quat::from_rotation_z(core::f32::consts::FRAC_PI_2);
        let mut values = a.to_array().to_vec();
        values.extend(b.to_array());
        let c = channel(
            Interpolation::Linear,
            Property::Rotation,
            &[0.0, 1.0],
            &values,
        );
        let mid = quat(&c.sample(0.5));
        let expected = Quat::from_rotation_z(core::f32::consts::FRAC_PI_4);
        assert!(mid.abs_diff_eq(expected, 1e-5), "{mid:?}");

        // A cubic key with zero tangents eases in and out: the midpoint is
        // the midpoint, the quarter point is below a quarter.
        let z = [0.0, 0.0, 0.0];
        let mut v = Vec::new();
        v.extend(z);
        v.extend([0.0, 0.0, 0.0]);
        v.extend(z);
        v.extend(z);
        v.extend([0.0, 1.0, 0.0]);
        v.extend(z);
        let s = channel(
            Interpolation::CubicSpline,
            Property::Translation,
            &[0.0, 2.0],
            &v,
        );
        assert!((s.sample(1.0)[1] - 0.5).abs() < 1e-6);
        assert!(s.sample(0.5)[1] < 0.25);
        assert_eq!(s.sample(2.0)[1], 1.0);
        assert_eq!(s.sample(0.0)[1], 0.0);
    }

    #[test]
    fn a_clip_samples_into_a_pose_and_poses_blend() {
        let e = Entity::from_bits(7);
        let clip = Clip {
            name: "c".into(),
            duration: 1.0,
            markers: Vec::new(),
            channels: vec![
                channel(
                    Interpolation::Linear,
                    Property::Translation,
                    &[0.0, 1.0],
                    &[0.0, 0.0, 0.0, 2.0, 0.0, 0.0],
                ),
                channel(
                    Interpolation::Step,
                    Property::Scale,
                    &[0.0],
                    &[3.0, 3.0, 3.0],
                ),
            ],
        };
        assert!(clip.validate().is_ok());
        let mut pose = Pose::default();
        clip.sample(0.5, &[Some(e), Some(e)], &mut pose);
        assert_eq!(pose.entries.len(), 1);
        assert_eq!(pose.entries[0].translation, Some(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(pose.entries[0].rotation, None);
        assert_eq!(pose.entries[0].scale, Some(Vec3::splat(3.0)));
        let t = pose.entries[0].apply_to(Transform {
            position: Vec3::ZERO,
            rotation: Quat::from_rotation_y(1.0),
            scale: Vec3::ONE,
        });
        assert_eq!(t.rotation, Quat::from_rotation_y(1.0));
        assert_eq!(t.scale, Vec3::splat(3.0));

        let mut other = Pose::default();
        other.entry(e).translation = Some(Vec3::new(4.0, 0.0, 0.0));
        other.entry(e).rotation = Some(Quat::from_rotation_z(1.0));
        pose.blend(&other, 0.25);
        let b = pose.entries[0];
        assert_eq!(b.translation, Some(Vec3::new(1.75, 0.0, 0.0)));
        assert_eq!(b.rotation, Some(Quat::from_rotation_z(1.0)));
        assert_eq!(b.scale, Some(Vec3::splat(3.0)));

        // An unresolved target is skipped; a bad clip is refused.
        let mut empty = Pose::default();
        clip.sample(0.5, &[None, None], &mut empty);
        assert!(empty.is_empty());
        let mut bad = clip.clone();
        bad.channels[0].times[1] = 0.0;
        assert!(bad.validate().is_err());
        bad.channels[0].times[1] = 5.0;
        assert!(bad.validate().unwrap_err().contains("past the duration"));
    }

    #[test]
    fn a_blend_space_finds_its_segment_and_validates() {
        let r = |n: u128| {
            fynkel_contracts::AssetRef::with_sub(
                fynkel_contracts::Uuid::from_bytes(n.to_be_bytes()),
                "anim/x",
            )
        };
        let space = BlendSpace {
            name: "locomotion".into(),
            parameter: "speed".into(),
            points: vec![
                BlendPoint {
                    at: 0.0,
                    clip: r(1),
                },
                BlendPoint {
                    at: 1.0,
                    clip: r(2),
                },
                BlendPoint {
                    at: 3.0,
                    clip: r(3),
                },
            ],
            sync: BlendSync::Phase,
        };
        assert!(space.validate().is_ok());
        assert_eq!(space.segment(-5.0), (0, 0, 0.0));
        assert_eq!(space.segment(0.0), (0, 0, 0.0));
        assert_eq!(space.segment(0.5), (0, 1, 0.5));
        assert_eq!(space.segment(1.0), (1, 1, 0.0));
        assert_eq!(space.segment(2.0), (1, 2, 0.5));
        assert_eq!(space.segment(3.0), (2, 2, 0.0));
        assert_eq!(space.segment(9.0), (2, 2, 0.0));
        let one = BlendSpace {
            points: vec![BlendPoint {
                at: 2.0,
                clip: r(1),
            }],
            ..space.clone()
        };
        assert_eq!(one.segment(7.0), (0, 0, 0.0));
        let mut bad = space.clone();
        bad.points[1].at = 0.0;
        assert!(bad.validate().unwrap_err().contains("strictly increasing"));
        bad = space.clone();
        bad.points.clear();
        assert!(bad.validate().is_err());
        bad = space.clone();
        bad.parameter.clear();
        assert!(bad.validate().is_err());
        bad = space;
        bad.points[0].clip = fynkel_contracts::AssetRef::default();
        assert!(bad.validate().unwrap_err().contains("not set"));
    }

    #[test]
    fn skins_validate_parents_and_names() {
        let joint = |name: &str, parent: Option<u32>| Joint {
            name: name.into(),
            parent,
            inverse_bind: Mat4::IDENTITY,
            rest: Transform::default(),
        };
        let skin = Skin {
            skeleton: String::new(),
            joints: vec![joint("a", None), joint("b", Some(0))],
        };
        assert!(skin.validate().is_ok());
        let dupe = Skin {
            skeleton: String::new(),
            joints: vec![joint("a", None), joint("a", Some(0))],
        };
        assert!(dupe.validate().unwrap_err().contains("twice"));
        let bad_parent = Skin {
            skeleton: String::new(),
            joints: vec![joint("a", Some(3))],
        };
        assert!(bad_parent.validate().is_err());
    }
}
