//! Character controllers, as Unity's (docs/stage-5.md §3.7): a followed
//! capsule the game moves with [`Physics::move_character`], which slides
//! it along what it meets, up steps and slopes it can climb, and pushes
//! the dynamic bodies in its way. On Rapier's kinematic character
//! controller.

use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude::*;

use crate::{Physics, PhysicsError, Vector, check_vector, collider_id};

/// It hit something on its sides ([`CharacterMove::flags`]).
pub const COLLIDED_SIDES: u32 = 1;
/// It hit something above it.
pub const COLLIDED_ABOVE: u32 = 2;
/// It hit something below it, or stands on the ground.
pub const COLLIDED_BELOW: u32 = 4;

/// How a character moves: Unity's `CharacterController` settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterSettings {
    /// The steepest slope it walks up, in degrees from flat.
    pub slope_limit: f32,
    /// The highest step it walks up, in metres.
    pub step_offset: f32,
    /// The gap it keeps from what it meets, in metres.
    pub skin_width: f32,
    /// How heavy it is when it pushes dynamic bodies: 0 pushes none.
    pub mass: f32,
}

impl Default for CharacterSettings {
    /// Unity's: 45°, 0.3 m, 0.08 m; and 70 kg.
    fn default() -> Self {
        Self { slope_limit: 45.0, step_offset: 0.3, skin_width: 0.08, mass: 70.0 }
    }
}

/// What a move did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterMove {
    /// How far it went: the object is to be moved by this.
    pub translation: Vector,
    /// What it hit: [`COLLIDED_SIDES`], [`COLLIDED_ABOVE`] and [`COLLIDED_BELOW`].
    pub flags: u32,
    /// The collider it stands on after the move, if it's grounded: what it
    /// hit below it, or else what's just under it.
    pub ground: Option<u64>,
    /// The key that collider was added with (0 with none: keys given
    /// out are never 0).
    pub ground_key: u64,
}

/// A collider a move hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterHit {
    pub collider: u64,
    /// The key the collider was added with.
    pub key: u64,
    /// Where it was hit, in the world.
    pub point: Vector,
    /// Its surface's normal there, out of it.
    pub normal: Vector,
}

// How far a hit's normal leans up (or down) before it's below (or above)
// rather than on the sides.
const LEAN: f32 = 0.05;

impl CharacterSettings {
    fn check(&self) -> Result<(), PhysicsError> {
        if !(0.0..=180.0).contains(&self.slope_limit) {
            return Err(PhysicsError::Value(format!("a slope limit is 0 to 180 degrees, not {}", self.slope_limit)));
        }
        for (what, value) in [("a step offset", self.step_offset), ("a mass", self.mass)] {
            if !(value >= 0.0 && value.is_finite()) {
                return Err(PhysicsError::Value(format!("{what} must be finite and 0 or more, not {value}")));
            }
        }
        if !(self.skin_width > 0.0 && self.skin_width.is_finite()) {
            return Err(PhysicsError::Value(format!(
                "a skin width must be finite and more than 0, not {}",
                self.skin_width
            )));
        }
        Ok(())
    }

    fn controller(&self) -> KinematicCharacterController {
        let slope = self.slope_limit.to_radians();
        let steps = self.step_offset > 0.0;
        KinematicCharacterController {
            up: Vector::Y,
            offset: CharacterLength::Absolute(self.skin_width),
            slide: true,
            autostep: steps.then_some(CharacterAutostep {
                max_height: CharacterLength::Absolute(self.step_offset),
                min_width: CharacterLength::Relative(0.5),
                include_dynamic_bodies: false,
            }),
            max_slope_climb_angle: slope,
            min_slope_slide_angle: slope,
            // Down a step as it walks off one, as it goes up one.
            snap_to_ground: steps.then_some(CharacterLength::Absolute(self.step_offset)),
            normal_nudge_factor: 1.0e-4,
        }
    }
}

impl Physics {
    /// Moves character controller `collider` (a followed collider, as its
    /// capsule) by `motion`, over `dt` seconds, as far as what's in its way
    /// lets it, and pushes the dynamic bodies it walks into. Doesn't move
    /// the collider: its object is to be moved by the move's translation,
    /// and the collider follows. The colliders it hit go into `hits`
    /// (cleared first), each once.
    pub fn move_character(
        &mut self,
        collider: u64,
        motion: Vector,
        dt: f32,
        settings: CharacterSettings,
        hits: &mut Vec<CharacterHit>,
    ) -> Result<CharacterMove, PhysicsError> {
        hits.clear();
        let handle = self.followed_handle(collider)?;
        check_vector("a move", motion)?;
        if !(dt > 0.0 && dt.is_finite()) {
            return Err(PhysicsError::Value(format!("a move's time must be finite and more than 0, not {dt}")));
        }
        settings.check()?;
        if let Some(Some(followed)) = self.followed.get_mut(handle.into_raw_parts().0 as usize) {
            followed.character = true;
        }
        self.sync();
        let controller = settings.controller();
        let own = &self.world.colliders[handle];
        let (shape, pose, groups) = (own.shared_shape().clone(), *own.position(), own.collision_groups());
        // The key went in as a u64, so it comes back whole.
        #[allow(clippy::cast_possible_truncation)]
        let key = own.user_data as u64;
        let contacts = &self.contacts;
        let meets = |_: ColliderHandle, c: &Collider| !contacts.ignores_keys(key, c.user_data as u64);
        let filter = QueryFilter::new().exclude_collider(handle).exclude_sensors().groups(groups).predicate(&meets);
        let mut collisions = std::mem::take(&mut self.character_collisions);
        collisions.clear();
        let w = &mut self.world;
        let moved = controller
            .move_shape(dt, &w.query_pipeline_with_filter(filter), &*shape, &pose, motion, |c| collisions.push(c));
        if settings.mass > 0.0 && !collisions.is_empty() {
            let mut queries = w.broad_phase.as_query_pipeline_mut(
                w.narrow_phase.query_dispatcher(),
                &mut w.bodies,
                &mut w.colliders,
                filter,
            );
            controller.solve_character_collision_impulses(dt, &mut queries, &*shape, settings.mass, &collisions);
        }
        let mut flags = if moved.grounded { COLLIDED_BELOW } else { 0 };
        for c in &collisions {
            let normal = c.hit.normal1;
            flags |= match normal.y {
                y if y > LEAN => COLLIDED_BELOW,
                y if y < -LEAN => COLLIDED_ABOVE,
                _ => COLLIDED_SIDES,
            };
            let id = collider_id(c.handle);
            if hits.iter().all(|h| h.collider != id) {
                #[allow(clippy::cast_possible_truncation)]
                let key = self.world.colliders[c.handle].user_data as u64;
                hits.push(CharacterHit { collider: id, key, point: c.hit.witness1, normal });
            }
        }
        // What it stands on: the hit below that faces up the most, or what
        // a short cast down from where it ended finds (it may have snapped
        // to the ground without hitting it).
        let below = collisions
            .iter()
            .filter(|c| c.hit.normal1.y > LEAN)
            .max_by(|a, b| a.hit.normal1.y.total_cmp(&b.hit.normal1.y));
        let mut ground = below.map(|c| c.handle);
        if ground.is_none() && flags & COLLIDED_BELOW != 0 {
            let mut at = pose;
            at.translation += moved.translation;
            ground = self
                .world
                .query_pipeline_with_filter(filter)
                .cast_shape(
                    &at,
                    -Vector::Y,
                    &*shape,
                    ShapeCastOptions::with_max_time_of_impact(settings.skin_width * 2.0 + 0.02),
                )
                .map(|(handle, _)| handle);
        }
        self.character_collisions = collisions;
        // The key went in as a u64, so it comes back whole.
        #[allow(clippy::cast_possible_truncation)]
        let ground_key = ground.map_or(0, |handle| self.world.colliders[handle].user_data as u64);
        Ok(CharacterMove { translation: moved.translation, flags, ground: ground.map(collider_id), ground_key })
    }
}
