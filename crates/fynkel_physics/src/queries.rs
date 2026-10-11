//! Queries, as Unity's `Physics` has them (docs/stage-5.md §3.6): rays,
//! shape casts and overlaps, filtered by layer and trigger. They see the
//! world as it is now: what was added or moved since the last step goes
//! into the queries' tree first.
//!
//! As Unity's, a ray finds nothing it starts inside, and a single cast
//! nothing it starts overlapping; the `_all` casts report what they start
//! overlapping at distance 0, with a zero point and the normal against the
//! direction.

use rapier3d::parry::query::{ShapeCastOptions, ShapeCastStatus};
use rapier3d::parry::shape::{Ball, Capsule, Cuboid, Shape};
use rapier3d::prelude::*;

use crate::{Physics, PhysicsError, Pose, Rotation, Vector, check_vector, collider_id};

/// What a query sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryOptions {
    /// One bit per layer it sees.
    pub layers: u32,
    /// Whether it sees triggers.
    pub hit_triggers: bool,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self { layers: u32::MAX, hit_triggers: true }
    }
}

/// A shape a query sweeps or tests, in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QueryShape {
    Sphere {
        center: Vector,
        radius: f32,
    },
    Box {
        center: Vector,
        half_extents: Vector,
        rotation: Rotation,
    },
    /// The centres of its two half-spheres, and their radius.
    Capsule {
        a: Vector,
        b: Vector,
        radius: f32,
    },
}

/// Where a query hit a collider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub collider: u64,
    /// The key the collider was added with.
    pub key: u64,
    pub point: Vector,
    /// The hit surface's outward normal.
    pub normal: Vector,
    pub distance: f32,
}

impl QueryShape {
    fn check(&self) -> Result<(), PhysicsError> {
        let positive = |what: &str, v: f32| {
            if v > 0.0 && v.is_finite() {
                Ok(())
            } else {
                Err(PhysicsError::Value(format!("{what} must be finite and more than 0, not {v}")))
            }
        };
        match *self {
            Self::Sphere { center, radius } => {
                check_vector("a sphere's center", center)?;
                positive("a sphere's radius", radius)
            }
            Self::Box { center, half_extents, rotation } => {
                check_vector("a box's center", center)?;
                check_vector("a box's half extents", half_extents)?;
                positive("a box's half extents", half_extents.min_element())?;
                if !rotation.is_finite() || (rotation.length() - 1.0).abs() > 1e-3 {
                    return Err(PhysicsError::Value(format!("a box's rotation {rotation} is not a unit quaternion")));
                }
                Ok(())
            }
            Self::Capsule { a, b, radius } => {
                check_vector("a capsule's ends", a)?;
                check_vector("a capsule's ends", b)?;
                positive("a capsule's radius", radius)
            }
        }
    }

    /// The parry shape and its pose.
    fn parts(&self) -> (Box<dyn Shape>, Pose) {
        match *self {
            Self::Sphere { center, radius } => (Box::new(Ball::new(radius)), Pose::from_translation(center)),
            Self::Box { center, half_extents, rotation } => {
                (Box::new(Cuboid::new(half_extents)), Pose::from_parts(center, rotation.normalize()))
            }
            Self::Capsule { a, b, radius } => {
                let middle = (a + b) / 2.0;
                (Box::new(Capsule::new(a - middle, b - middle, radius)), Pose::from_translation(middle))
            }
        }
    }
}

/// The direction made unit length, or an error; and the distance as
/// Rapier takes it.
fn sweep(direction: Vector, max_distance: f32) -> Result<Option<(Vector, f32)>, PhysicsError> {
    check_vector("a direction", direction)?;
    if max_distance.is_nan() || max_distance < 0.0 {
        return Err(PhysicsError::Value(format!("a query's max distance must be 0 or more, not {max_distance}")));
    }
    let direction = direction.normalize_or_zero();
    // As Unity: no direction, no hits.
    if direction == Vector::ZERO {
        return Ok(None);
    }
    Ok(Some((direction, max_distance.min(f32::MAX))))
}

fn sees(options: QueryOptions, collider: &Collider) -> bool {
    collider.collision_groups().memberships.bits() & options.layers != 0
        && (options.hit_triggers || !collider.is_sensor())
}

impl Physics {
    fn hit(&self, handle: ColliderHandle, point: Vector, normal: Vector, distance: f32) -> Hit {
        // The key went in as a u64, so it comes back whole.
        #[allow(clippy::cast_possible_truncation)]
        let key = self.world.colliders[handle].user_data as u64;
        Hit { collider: collider_id(handle), key, point, normal, distance }
    }

    /// The first collider a ray from `origin` along `direction` hits within
    /// `max_distance`.
    pub fn raycast(
        &mut self,
        origin: Vector,
        direction: Vector,
        max_distance: f32,
        options: QueryOptions,
    ) -> Result<Option<Hit>, PhysicsError> {
        check_vector("a ray's origin", origin)?;
        let Some((direction, max)) = sweep(direction, max_distance)? else { return Ok(None) };
        self.sync();
        let outside =
            |_: ColliderHandle, c: &Collider| sees(options, c) && !c.shape().contains_point(c.position(), origin);
        let ray = Ray::new(origin, direction);
        let found = self
            .world
            .query_pipeline_with_filter(QueryFilter::new().predicate(&outside))
            .cast_ray_and_get_normal(&ray, max, true);
        Ok(found.map(|(handle, hit)| {
            self.hit(handle, origin + direction * hit.time_of_impact, hit.normal, hit.time_of_impact)
        }))
    }

    /// Every collider the ray hits, nearest first, into `out` (cleared first).
    pub fn raycast_all(
        &mut self,
        origin: Vector,
        direction: Vector,
        max_distance: f32,
        options: QueryOptions,
        out: &mut Vec<Hit>,
    ) -> Result<(), PhysicsError> {
        out.clear();
        check_vector("a ray's origin", origin)?;
        let Some((direction, max)) = sweep(direction, max_distance)? else { return Ok(()) };
        self.sync();
        let outside =
            |_: ColliderHandle, c: &Collider| sees(options, c) && !c.shape().contains_point(c.position(), origin);
        let ray = Ray::new(origin, direction);
        let hits: Vec<_> = self
            .world
            .intersect_ray(ray, max, true, QueryFilter::new().predicate(&outside))
            .map(|(handle, _, hit)| (handle, hit))
            .collect();
        out.extend(hits.into_iter().map(|(handle, hit)| {
            self.hit(handle, origin + direction * hit.time_of_impact, hit.normal, hit.time_of_impact)
        }));
        out.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        Ok(())
    }

    /// The first collider `shape` hits, swept along `direction` up to
    /// `max_distance`; not those it starts overlapping.
    pub fn cast(
        &mut self,
        shape: QueryShape,
        direction: Vector,
        max_distance: f32,
        options: QueryOptions,
    ) -> Result<Option<Hit>, PhysicsError> {
        shape.check()?;
        let Some((direction, max)) = sweep(direction, max_distance)? else { return Ok(None) };
        self.sync();
        let (parry, pose) = shape.parts();
        let dispatcher = self.world.narrow_phase.query_dispatcher();
        let apart = |_: ColliderHandle, c: &Collider| {
            sees(options, c)
                && !dispatcher
                    .intersection_test(&pose.inv_mul(c.position()), parry.as_ref(), c.shape())
                    .is_ok_and(|hit| hit.intersecting)
        };
        let found = self.world.query_pipeline_with_filter(QueryFilter::new().predicate(&apart)).cast_shape(
            &pose,
            direction,
            parry.as_ref(),
            ShapeCastOptions::with_max_time_of_impact(max),
        );
        Ok(found.map(|(handle, hit)| self.hit(handle, hit.witness1, hit.normal1, hit.time_of_impact)))
    }

    /// Every collider `shape` hits, swept, nearest first, into `out`
    /// (cleared first); those it starts overlapping at distance 0.
    pub fn cast_all(
        &mut self,
        shape: QueryShape,
        direction: Vector,
        max_distance: f32,
        options: QueryOptions,
        out: &mut Vec<Hit>,
    ) -> Result<(), PhysicsError> {
        out.clear();
        shape.check()?;
        let Some((direction, max)) = sweep(direction, max_distance)? else { return Ok(()) };
        self.sync();
        let (parry, pose) = shape.parts();
        // Cast again and again, each time without what was found: Rapier
        // gives the nearest only.
        let mut found: Vec<ColliderHandle> = Vec::new();
        loop {
            let unseen = |handle: ColliderHandle, c: &Collider| sees(options, c) && !found.contains(&handle);
            let next = self.world.query_pipeline_with_filter(QueryFilter::new().predicate(&unseen)).cast_shape(
                &pose,
                direction,
                parry.as_ref(),
                ShapeCastOptions::with_max_time_of_impact(max),
            );
            let Some((handle, hit)) = next else { break };
            found.push(handle);
            let hit = if hit.status == ShapeCastStatus::PenetratingOrWithinTargetDist {
                self.hit(handle, Vector::ZERO, -direction, 0.0)
            } else {
                self.hit(handle, hit.witness1, hit.normal1, hit.time_of_impact)
            };
            out.push(hit);
        }
        Ok(())
    }

    /// The keys and ids of the colliders `shape` touches or holds, into
    /// `out` (cleared first).
    pub fn overlap(
        &mut self,
        shape: QueryShape,
        options: QueryOptions,
        out: &mut Vec<(u64, u64)>,
    ) -> Result<(), PhysicsError> {
        out.clear();
        shape.check()?;
        self.sync();
        let (parry, pose) = shape.parts();
        let filter = |_: ColliderHandle, c: &Collider| sees(options, c);
        let found: Vec<_> = self
            .world
            .intersect_shape(pose, parry.as_ref(), QueryFilter::new().predicate(&filter))
            .map(|(handle, collider)| {
                #[allow(clippy::cast_possible_truncation)]
                (collider_id(handle), collider.user_data as u64)
            })
            .collect();
        out.extend(found);
        Ok(())
    }

    /// Whether `shape` touches or holds any collider.
    pub fn check(&mut self, shape: QueryShape, options: QueryOptions) -> Result<bool, PhysicsError> {
        shape.check()?;
        self.sync();
        let (parry, pose) = shape.parts();
        let filter = |_: ColliderHandle, c: &Collider| sees(options, c);
        Ok(self.world.intersect_shape(pose, parry.as_ref(), QueryFilter::new().predicate(&filter)).next().is_some())
    }

    /// The point of collider `id` nearest `point`: `point` itself if it's
    /// inside.
    pub fn closest_point(&self, id: u64, point: Vector) -> Result<Vector, PhysicsError> {
        check_vector("a point", point)?;
        let collider = &self.world.colliders[self.collider_handle(id)?];
        Ok(collider.shape().project_point(collider.position(), point, true).point)
    }

    /// Where a ray hits collider `id`, if it does, and doesn't start inside it.
    pub fn raycast_collider(
        &self,
        id: u64,
        origin: Vector,
        direction: Vector,
        max_distance: f32,
    ) -> Result<Option<Hit>, PhysicsError> {
        let handle = self.collider_handle(id)?;
        check_vector("a ray's origin", origin)?;
        let Some((direction, max)) = sweep(direction, max_distance)? else { return Ok(None) };
        let collider = &self.world.colliders[handle];
        if collider.shape().contains_point(collider.position(), origin) {
            return Ok(None);
        }
        let ray = Ray::new(origin, direction);
        Ok(collider
            .shape()
            .cast_ray_and_get_normal(collider.position(), &ray, max, true)
            .map(|hit| self.hit(handle, origin + direction * hit.time_of_impact, hit.normal, hit.time_of_impact)))
    }
}
