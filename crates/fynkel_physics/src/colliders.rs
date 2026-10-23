//! Colliders that follow an object, as Unity's do (docs/stage-5.md §3.3):
//! their shape is given in the object's own space, unscaled, and the
//! physics scales and places it from the object's world pose.

use rapier3d::prelude::*;

use crate::meshes::Meshes;
use crate::{Physics, PhysicsError, Pose, Rotation, Vector, body_handle, check_vector, collider_id};

/// The axis a capsule's length runs along, in its object's space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// A collider's shape in its object's space, before the object's scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColliderShape {
    /// A box `size` across on each axis.
    Box {
        size: Vector,
    },
    Sphere {
        radius: f32,
    },
    /// A capsule `height` long from end to end (the half-spheres included)
    /// along `direction`. Shorter than `2 * radius`, it's a sphere.
    Capsule {
        radius: f32,
        height: f32,
        direction: Axis,
    },
    /// Mesh `mesh` ([`Physics::add_mesh`]): its triangles, or its convex
    /// hull when `convex`. Triangles collide only with what's convex, so
    /// on a dynamic body they collide as the hull too.
    Mesh {
        mesh: u64,
        convex: bool,
    },
}

/// How two colliders' frictions or bouncinesses make the pair's (Unity's
/// `PhysicsMaterialCombine`). Where two differ, the later one here wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Combine {
    #[default]
    Average,
    Minimum,
    Multiply,
    Maximum,
}

impl Combine {
    fn rule(self) -> CoefficientCombineRule {
        match self {
            Self::Average => CoefficientCombineRule::Average,
            Self::Minimum => CoefficientCombineRule::Min,
            Self::Multiply => CoefficientCombineRule::Multiply,
            Self::Maximum => CoefficientCombineRule::Max,
        }
    }
}

/// What a collider's surface is like (Unity's `PhysicsMaterial`; Rapier
/// has one friction, so it's the dynamic friction).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub friction: f32,
    pub bounciness: f32,
    pub friction_combine: Combine,
    pub bounce_combine: Combine,
}

impl Material {
    /// Unity's default: friction 0.6, no bounce, both averaged.
    pub const DEFAULT: Self =
        Self { friction: 0.6, bounciness: 0.0, friction_combine: Combine::Average, bounce_combine: Combine::Average };
}

impl Default for Material {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What a collider is, as the game sets it: Unity's collider properties.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColliderSpec {
    pub shape: ColliderShape,
    /// Where its middle is, in the object's space.
    pub center: Vector,
    /// A trigger: queries can see it, but nothing collides with it.
    pub is_trigger: bool,
    /// Its object's layer, 0 to 31.
    pub layer: u32,
    pub material: Material,
    /// What its touching others is reported for ([`crate::Physics::contact_events`]):
    /// [`crate::LISTEN_TRIGGERS`], [`crate::LISTEN_COLLISIONS`], both or 0.
    pub listens: u32,
}

/// An object's world pose, as the engine gives it: `GameObject.worldTransform`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObjectPose {
    pub position: Vector,
    pub rotation: Rotation,
    pub scale: Vector,
}

impl ObjectPose {
    pub const IDENTITY: Self = Self { position: Vector::ZERO, rotation: Rotation::IDENTITY, scale: Vector::ONE };
}

/// A followed collider's spec, the last pose of its object, the body it's
/// on, if any, and whether it's a character controller's (moved by
/// [`Physics::move_character`]), which meets triggers as a kinematic body.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Followed {
    pub(crate) spec: ColliderSpec,
    pub(crate) object: ObjectPose,
    pub(crate) body: Option<RigidBodyHandle>,
    pub(crate) character: bool,
}

// The smallest a scaled size gets: a collider scaled to 0 stays a point
// rather than being refused.
const SMALLEST: f32 = 1e-5;

impl ColliderSpec {
    fn check(&self) -> Result<(), PhysicsError> {
        check_vector("a collider's center", self.center)?;
        if self.layer >= 32 {
            return Err(PhysicsError::Value(format!("a layer is 0 to 31, not {}", self.layer)));
        }
        check_not_negative("a material's friction", self.material.friction)?;
        if !(0.0..=1.0).contains(&self.material.bounciness) {
            return Err(PhysicsError::Value(format!(
                "a material's bounciness is 0 to 1, not {}",
                self.material.bounciness
            )));
        }
        match self.shape {
            ColliderShape::Box { size } => {
                check_vector("a box's size", size)?;
                if size.min_element() < 0.0 {
                    return Err(PhysicsError::Value(format!("a box's size must be 0 or more, not {size}")));
                }
            }
            ColliderShape::Sphere { radius } => check_not_negative("a sphere's radius", radius)?,
            ColliderShape::Capsule { radius, height, .. } => {
                check_not_negative("a capsule's radius", radius)?;
                check_not_negative("a capsule's height", height)?;
            }
            ColliderShape::Mesh { .. } => {}
        }
        Ok(())
    }

    /// The Rapier shape, scaled as Unity scales it, and where it sits in
    /// the world on an object at `object`: a mesh's hull if `hull`.
    pub(crate) fn place(&self, object: ObjectPose, meshes: &Meshes, hull: bool) -> (SharedShape, Pose) {
        (self.scaled(object.scale.abs(), meshes, hull), self.pose(object))
    }

    fn scaled(&self, s: Vector, meshes: &Meshes, hull: bool) -> SharedShape {
        match self.shape {
            ColliderShape::Box { size } => {
                let half = (size * s / 2.0).max(Vector::splat(SMALLEST));
                SharedShape::cuboid(half.x, half.y, half.z)
            }
            // A sphere takes the largest of the three scales.
            ColliderShape::Sphere { radius } => SharedShape::ball((radius * s.max_element()).max(SMALLEST)),
            // A capsule's length takes its axis's scale, its radius the
            // larger of the other two.
            ColliderShape::Capsule { radius, height, direction } => {
                let (along, across) = match direction {
                    Axis::X => (s.x, s.y.max(s.z)),
                    Axis::Y => (s.y, s.x.max(s.z)),
                    Axis::Z => (s.z, s.x.max(s.y)),
                };
                let radius = (radius * across).max(SMALLEST);
                let half_height = (height * along / 2.0 - radius).max(0.0);
                SharedShape::capsule_y(half_height, radius)
            }
            // A mesh is scaled on each axis, so it squashes and stretches.
            ColliderShape::Mesh { mesh, convex } => meshes.shape(mesh, convex || hull, s, SMALLEST),
        }
    }

    /// Where the shape sits in the world on an object at `object`.
    pub(crate) fn pose(&self, object: ObjectPose) -> Pose {
        // Rapier's capsules run along Y.
        let turn = match self.shape {
            ColliderShape::Capsule { direction: Axis::X, .. } => {
                Rotation::from_rotation_z(-std::f32::consts::FRAC_PI_2)
            }
            ColliderShape::Capsule { direction: Axis::Z, .. } => Rotation::from_rotation_x(std::f32::consts::FRAC_PI_2),
            _ => Rotation::IDENTITY,
        };
        let position = object.position + object.rotation * (self.center * object.scale);
        Pose::from_parts(position, (object.rotation * turn).normalize())
    }

    /// Whether its shape changes when its body turns dynamic or kinematic.
    pub(crate) fn hull_on_dynamic(&self) -> bool {
        matches!(self.shape, ColliderShape::Mesh { convex: false, .. })
    }
}

fn check_not_negative(what: &str, value: f32) -> Result<(), PhysicsError> {
    if value >= 0.0 && value.is_finite() {
        Ok(())
    } else {
        Err(PhysicsError::Value(format!("{what} must be finite and 0 or more, not {value}")))
    }
}

fn check_object(object: ObjectPose) -> Result<(), PhysicsError> {
    check_vector("an object's position", object.position)?;
    check_vector("an object's scale", object.scale)?;
    let r = object.rotation;
    if !r.is_finite() || (r.length() - 1.0).abs() > 1e-3 {
        return Err(PhysicsError::Value(format!("an object's rotation {r} is not a unit quaternion")));
    }
    Ok(())
}

impl Physics {
    /// Adds a collider with no body, a static one, that follows an object:
    /// `spec` in the object's space, placed by the object's world pose,
    /// with `key` to know it by in query hits.
    pub fn add_static_collider(
        &mut self,
        key: u64,
        spec: ColliderSpec,
        object: ObjectPose,
    ) -> Result<u64, PhysicsError> {
        self.add_object_collider(key, spec, object, None)
    }

    /// Adds a collider that follows an object, as [`Self::add_static_collider`]
    /// does, on `body` when there is one: it then moves with the body, from
    /// where the object is relative to the body now, and its volume
    /// shares out the body's mass.
    pub fn add_object_collider(
        &mut self,
        key: u64,
        spec: ColliderSpec,
        object: ObjectPose,
        body: Option<u64>,
    ) -> Result<u64, PhysicsError> {
        self.check_spec(&spec)?;
        check_object(object)?;
        let body = body.map(|id| body_handle(&self.world.bodies, id)).transpose()?;
        let (shape, pose) = spec.place(object, &self.meshes, self.dynamic(body));
        let builder = ColliderBuilder::new(shape)
            .sensor(spec.is_trigger)
            .active_collision_types(collision_types(spec.is_trigger))
            .collision_groups(self.contacts.groups(spec.layer))
            .active_hooks(self.contacts.hooks(key))
            .user_data(u128::from(key));
        let builder = with_material(builder, spec.material);
        let w = &mut self.world;
        let handle = match body {
            Some(parent) => {
                // Its mass comes from the body (bodies.rs), so its own is 0.
                let relative = w.bodies[parent].position().inverse() * pose;
                let builder = builder.position(relative).density(0.0);
                w.colliders.insert_with_parent(builder, parent, &mut w.bodies)
            }
            None => w.colliders.insert(builder.position(pose)),
        };
        let index = handle.into_raw_parts().0 as usize;
        if self.followed.len() <= index {
            self.followed.resize(index + 1, None);
        }
        self.followed[index] = Some(Followed { spec, object, body, character: false });
        self.listening_changed(0, spec.listens);
        self.dirty.push(handle);
        if let Some(body) = body {
            self.mass_changed(body);
        }
        Ok(collider_id(handle))
    }

    /// Changes what a followed collider is: its shape, center, trigger or
    /// layer.
    pub fn set_collider_spec(&mut self, id: u64, spec: ColliderSpec) -> Result<(), PhysicsError> {
        self.check_spec(&spec)?;
        let handle = self.followed_handle(id)?;
        let followed = self.followed[handle.into_raw_parts().0 as usize].as_mut().expect("followed");
        let before = followed.spec.listens;
        followed.spec = spec;
        let object = followed.object;
        self.listening_changed(before, spec.listens);
        self.replace(handle, spec, object);
        Ok(())
    }

    /// Marks followed collider `id` as a character controller's, or not:
    /// it meets triggers as a kinematic body would, before its first
    /// [`Physics::move_character`] too. Adding the collider again clears it.
    pub fn set_character(&mut self, id: u64, character: bool) -> Result<(), PhysicsError> {
        let handle = self.followed_handle(id)?;
        self.followed[handle.into_raw_parts().0 as usize].as_mut().expect("followed").character = character;
        Ok(())
    }

    /// The followed colliders `ids[i]`'s objects are now at `objects[i]`:
    /// those that moved, turned or were scaled are put where their objects
    /// are. Checks every id and pose before changing anything.
    pub fn follow(&mut self, ids: &[u64], objects: &[ObjectPose]) -> Result<(), PhysicsError> {
        if ids.len() != objects.len() {
            return Err(PhysicsError::Value(format!("{} ids but {} poses", ids.len(), objects.len())));
        }
        for (&id, &object) in ids.iter().zip(objects) {
            self.followed_handle(id)?;
            check_object(object)?;
        }
        for (&id, &object) in ids.iter().zip(objects) {
            let handle = self.followed_handle(id).expect("checked above");
            let followed = self.followed[handle.into_raw_parts().0 as usize].as_mut().expect("followed");
            if followed.object == object {
                continue;
            }
            let rescaled = followed.object.scale != object.scale;
            followed.object = object;
            let spec = followed.spec;
            if rescaled {
                self.replace(handle, spec, object);
            } else {
                self.put(handle, spec.pose(object));
            }
        }
        Ok(())
    }

    /// A collider's shape and where it is, as the physics has them: for
    /// drawing it.
    pub fn collider_shape(&self, id: u64) -> Result<(crate::Shape, Pose), PhysicsError> {
        let handle = self.collider_handle(id)?;
        let collider = &self.world.colliders[handle];
        let shape = match collider.shape().as_typed_shape() {
            TypedShape::Cuboid(c) => crate::Shape::Box { half_extents: c.half_extents },
            TypedShape::Ball(b) => crate::Shape::Sphere { radius: b.radius },
            TypedShape::Capsule(c) => crate::Shape::Capsule { half_height: c.half_height(), radius: c.radius },
            _ => return Err(PhysicsError::Value(format!("collider {id} is of a shape that can't be read"))),
        };
        Ok((shape, *collider.position()))
    }

    /// A mesh collider's edges where they are in the world, as pairs of
    /// points, into `out`: its triangles' or its hull's, each once. For
    /// drawing it.
    pub fn collider_edges(&self, id: u64, out: &mut Vec<[Vector; 2]>) -> Result<(), PhysicsError> {
        out.clear();
        let handle = self.collider_handle(id)?;
        let collider = &self.world.colliders[handle];
        let pose = *collider.position();
        match collider.shape().as_typed_shape() {
            TypedShape::TriMesh(mesh) => {
                let points = mesh.vertices();
                let mut seen = std::collections::HashSet::new();
                for t in mesh.indices() {
                    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                        if seen.insert((a.min(b), a.max(b))) {
                            out.push([pose * points[a as usize], pose * points[b as usize]]);
                        }
                    }
                }
            }
            TypedShape::ConvexPolyhedron(hull) => {
                let points = hull.points();
                for edge in hull.edges() {
                    let [a, b] = edge.vertices;
                    out.push([pose * points[a as usize], pose * points[b as usize]]);
                }
            }
            _ => return Err(PhysicsError::Value(format!("collider {id} isn't a mesh"))),
        }
        Ok(())
    }

    fn check_spec(&self, spec: &ColliderSpec) -> Result<(), PhysicsError> {
        spec.check()?;
        if let ColliderShape::Mesh { mesh, convex } = spec.shape {
            self.meshes.check(mesh, convex)?;
        }
        Ok(())
    }

    /// Whether `body` is there and dynamic: its non-convex meshes are hulls.
    fn dynamic(&self, body: Option<RigidBodyHandle>) -> bool {
        body.is_some_and(|b| self.world.bodies[b].is_dynamic())
    }

    /// Gives `body`'s non-convex mesh colliders their hull or their
    /// triangles again, after it turned dynamic or kinematic.
    pub(crate) fn reshape_meshes(&mut self, body: RigidBodyHandle) {
        let colliders: Vec<_> = self.world.bodies[body].colliders().to_vec();
        for handle in colliders {
            let Some(Some(followed)) = self.followed.get(handle.into_raw_parts().0 as usize) else { continue };
            if followed.spec.hull_on_dynamic() {
                let (spec, object) = (followed.spec, followed.object);
                self.replace(handle, spec, object);
            }
        }
    }

    pub(crate) fn replace(&mut self, handle: ColliderHandle, spec: ColliderSpec, object: ObjectPose) {
        let body = collider_body(&self.followed, handle);
        let (shape, pose) = spec.place(object, &self.meshes, self.dynamic(body));
        let groups = self.contacts.groups(spec.layer);
        let collider = &mut self.world.colliders[handle];
        collider.set_shape(shape);
        collider.set_sensor(spec.is_trigger);
        collider.set_active_collision_types(collision_types(spec.is_trigger));
        collider.set_collision_groups(groups);
        collider.set_friction(spec.material.friction);
        collider.set_restitution(spec.material.bounciness);
        collider.set_friction_combine_rule(spec.material.friction_combine.rule());
        collider.set_restitution_combine_rule(spec.material.bounce_combine.rule());
        self.put(handle, pose);
        // A new shape or trigger changes how the body's mass is shared out.
        if let Some(body) = body {
            self.mass_changed(body);
        }
    }

    /// Puts a collider at `pose` in the world: relative to its body, where
    /// the body is now, when it has one.
    fn put(&mut self, handle: ColliderHandle, pose: Pose) {
        let w = &mut self.world;
        match collider_body(&self.followed, handle) {
            Some(body) => {
                let relative = w.bodies[body].position().inverse() * pose;
                let collider = &mut w.colliders[handle];
                collider.set_position_wrt_parent(relative);
                // The step works the world pose out again; until then the
                // queries need it now.
                collider.set_position(pose);
            }
            None => w.colliders[handle].set_position(pose),
        }
        self.dirty.push(handle);
    }

    pub(crate) fn followed_handle(&self, id: u64) -> Result<ColliderHandle, PhysicsError> {
        let handle = self.collider_handle(id)?;
        match self.followed.get(handle.into_raw_parts().0 as usize) {
            Some(Some(_)) => Ok(handle),
            _ => Err(PhysicsError::Value(format!("collider {id} doesn't follow an object"))),
        }
    }

    /// Puts the colliders added or moved since the last step into the
    /// queries' tree, so the queries see the world as it is now.
    pub(crate) fn sync(&mut self) {
        if self.dirty.is_empty() {
            return;
        }
        let w = &mut self.world;
        for &handle in &self.dirty {
            if let Some(collider) = w.colliders.get(handle) {
                w.broad_phase.set_aabb(&w.integration_parameters, handle, collider.compute_aabb());
            }
        }
        self.dirty.clear();
    }
}

fn collider_body(followed: &[Option<Followed>], handle: ColliderHandle) -> Option<RigidBodyHandle> {
    followed.get(handle.into_raw_parts().0 as usize)?.as_ref()?.body
}

fn with_material(builder: ColliderBuilder, material: Material) -> ColliderBuilder {
    builder
        .friction(material.friction)
        .restitution(material.bounciness)
        .friction_combine_rule(material.friction_combine.rule())
        .restitution_combine_rule(material.bounce_combine.rule())
}

/// What a collider meets: Rapier's default for a solid one (a dynamic body
/// on one side); every pair for a trigger, which Unity's table lets meet
/// kinematic and static colliders (contacts.rs sorts out which report).
fn collision_types(is_trigger: bool) -> ActiveCollisionTypes {
    if is_trigger { ActiveCollisionTypes::all() } else { ActiveCollisionTypes::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl ColliderSpec {
        fn place_plain(&self, object: ObjectPose) -> (SharedShape, Pose) {
            self.place(object, &Meshes::default(), false)
        }
    }

    fn spec(shape: ColliderShape) -> ColliderSpec {
        ColliderSpec {
            shape,
            center: Vector::ZERO,
            is_trigger: false,
            layer: 0,
            material: Material::DEFAULT,
            listens: 0,
        }
    }

    fn on(position: Vector, rotation: Rotation, scale: Vector) -> ObjectPose {
        ObjectPose { position, rotation, scale }
    }

    #[test]
    fn a_box_scales_on_each_axis_and_its_center_scales_and_turns_with_its_object() {
        let mut s = spec(ColliderShape::Box { size: Vector::new(1.0, 2.0, 3.0) });
        s.center = Vector::new(1.0, 0.0, 0.0);
        let turn = Rotation::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let (shape, pose) = s.place_plain(on(Vector::new(5.0, 0.0, 0.0), turn, Vector::new(2.0, -1.0, 1.0)));
        let half = shape.as_cuboid().expect("a box").half_extents;
        assert!(half.abs_diff_eq(Vector::new(1.0, 1.0, 1.5), 1e-6), "{half}");
        // The center, 1 along X, doubled, then turned a quarter left: 2 along -Z.
        assert!(pose.translation.abs_diff_eq(Vector::new(5.0, 0.0, -2.0), 1e-5), "{}", pose.translation);
        assert!(pose.rotation.abs_diff_eq(turn, 1e-6));
    }

    #[test]
    fn a_sphere_takes_the_largest_scale() {
        let (shape, _) = spec(ColliderShape::Sphere { radius: 0.5 }).place_plain(on(
            Vector::ZERO,
            Rotation::IDENTITY,
            Vector::new(1.0, -3.0, 2.0),
        ));
        assert_eq!(shape.as_ball().expect("a ball").radius, 1.5);
    }

    #[test]
    fn a_capsule_lies_along_its_direction_scaled_along_and_across() {
        for (direction, scale, along) in [
            (Axis::X, Vector::new(2.0, 1.0, 3.0), Vector::X),
            (Axis::Y, Vector::new(3.0, 2.0, 1.0), Vector::Y),
            (Axis::Z, Vector::new(1.0, 3.0, 2.0), Vector::Z),
        ] {
            let (shape, pose) = spec(ColliderShape::Capsule { radius: 0.5, height: 4.0, direction }).place_plain(on(
                Vector::ZERO,
                Rotation::IDENTITY,
                scale,
            ));
            let capsule = shape.as_capsule().expect("a capsule");
            // Radius 0.5 × 3 (the larger across); length 4 × 2 = 8, so the
            // centres are 8/2 - 1.5 = 2.5 from the middle.
            assert!((capsule.radius - 1.5).abs() < 1e-6, "{direction:?}");
            let end = pose.rotation * capsule.segment.b;
            assert!(end.abs_diff_eq(along * 2.5, 1e-5) || end.abs_diff_eq(-along * 2.5, 1e-5), "{direction:?}: {end}");
        }
    }

    #[test]
    fn a_capsule_shorter_than_its_width_is_a_sphere_and_a_flat_box_stays_a_box() {
        let (shape, _) = spec(ColliderShape::Capsule { radius: 1.0, height: 0.5, direction: Axis::Y })
            .place_plain(ObjectPose::IDENTITY);
        assert_eq!(shape.as_capsule().unwrap().half_height(), 0.0);
        let (shape, _) = spec(ColliderShape::Box { size: Vector::new(1.0, 1.0, 1.0) }).place_plain(on(
            Vector::ZERO,
            Rotation::IDENTITY,
            Vector::new(1.0, 0.0, 1.0),
        ));
        assert!(shape.as_cuboid().unwrap().half_extents.y > 0.0);
    }

    #[test]
    fn bad_specs_and_poses_are_refused_and_nothing_is_added() {
        let mut physics = Physics::new();
        for bad in [
            spec(ColliderShape::Sphere { radius: -1.0 }),
            spec(ColliderShape::Box { size: Vector::new(1.0, f32::NAN, 1.0) }),
            spec(ColliderShape::Box { size: Vector::new(1.0, -1.0, 1.0) }),
            spec(ColliderShape::Capsule { radius: 1.0, height: f32::INFINITY, direction: Axis::Y }),
            ColliderSpec { layer: 32, ..spec(ColliderShape::Sphere { radius: 1.0 }) },
        ] {
            assert!(
                matches!(physics.add_static_collider(1, bad, ObjectPose::IDENTITY), Err(PhysicsError::Value(_))),
                "{bad:?}"
            );
        }
        let bad_pose = on(Vector::ZERO, Rotation::from_xyzw(0.0, 0.0, 0.0, 2.0), Vector::ONE);
        assert!(physics.add_static_collider(1, spec(ColliderShape::Sphere { radius: 1.0 }), bad_pose).is_err());
        assert_eq!(physics.collider_count(), 0);
    }

    #[test]
    fn following_moves_only_what_moved_and_checks_everything_first() {
        let mut physics = Physics::new();
        let a =
            physics.add_static_collider(1, spec(ColliderShape::Sphere { radius: 1.0 }), ObjectPose::IDENTITY).unwrap();
        let b =
            physics.add_static_collider(2, spec(ColliderShape::Sphere { radius: 1.0 }), ObjectPose::IDENTITY).unwrap();
        let moved = on(Vector::new(3.0, 0.0, 0.0), Rotation::IDENTITY, Vector::splat(2.0));
        physics.follow(&[a, b], &[moved, ObjectPose::IDENTITY]).unwrap();
        let (shape, pose) = physics.collider_shape(a).unwrap();
        assert_eq!(pose.translation, Vector::new(3.0, 0.0, 0.0));
        assert_eq!(shape, crate::Shape::Sphere { radius: 2.0 });
        assert_eq!(physics.collider_shape(b).unwrap().1.translation, Vector::ZERO);

        // One bad id and nothing moves.
        physics.remove_collider(b).unwrap();
        assert_eq!(physics.follow(&[a, b], &[ObjectPose::IDENTITY; 2]), Err(PhysicsError::NoCollider(b)));
        assert_eq!(physics.collider_shape(a).unwrap().1.translation, Vector::new(3.0, 0.0, 0.0));
        // A collider that follows no object can't follow one.
        let body = physics.add_body(crate::BodyKind::Dynamic, 1, Pose::IDENTITY).unwrap();
        let plain = physics.add_collider(Some(body), crate::Shape::Sphere { radius: 1.0 }, Pose::IDENTITY).unwrap();
        assert!(matches!(physics.follow(&[plain], &[ObjectPose::IDENTITY]), Err(PhysicsError::Value(_))));
    }
}
