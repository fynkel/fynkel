//! The physics components (blueprint §9, §19.2d items 8 and 9): pure data,
//! authored in scenes, crossing the bridge with the generated codecs. The
//! Rapier side never leaks — a body is addressed by its entity, shapes are
//! described in metres, masses in kilograms.

use fynkel_contracts::AssetRef;
use fynkel_schema_derive::Schema;
use glam::Vec3;

/// How a body moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Schema)]
#[schema(name = "fynkel.BodyType")]
pub enum BodyType {
    /// Never moves on its own; gameplay may still set its transform (a
    /// simulation-lane write is a teleport).
    Static,
    /// Simulated: gravity, contacts, forces. Physics owns the transform
    /// (`PHYSICS_OWNED` for presentation writes); gameplay moves it with
    /// body commands or a simulation-lane transform write (a teleport).
    #[default]
    Dynamic,
    /// Moved by gameplay (`setKinematicTarget` or a transform write);
    /// pushes dynamic bodies, is pushed by nothing.
    Kinematic,
}

/// A rigid body on the entity. Its colliders are the entity's own
/// `fynkel.Collider` (one per entity in V1). A `Collider` without a
/// `RigidBody` is static geometry.
#[derive(Clone, Copy, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 10, name = "fynkel.RigidBody")]
pub struct RigidBody {
    pub body_type: BodyType,
    /// Kilograms (dynamic only, `> 0`); the inertia follows the collider
    /// shape scaled to this mass.
    pub mass: f32,
    /// Per-second velocity decay, `>= 0`.
    pub linear_damping: f32,
    /// Per-second angular velocity decay, `>= 0`.
    pub angular_damping: f32,
    /// Multiplies the world's gravity for this body (0 floats).
    pub gravity_scale: f32,
    /// Continuous collision detection: fast bodies do not tunnel through
    /// thin geometry (costs a little per body).
    pub ccd: bool,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            body_type: BodyType::Dynamic,
            mass: 1.0,
            linear_damping: 0.0,
            angular_damping: 0.05,
            gravity_scale: 1.0,
            ccd: false,
        }
    }
}

/// Collision shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Schema)]
#[schema(name = "fynkel.ColliderShape")]
pub enum ColliderShape {
    /// `size` is the half extents.
    #[default]
    Box,
    /// `size.x` is the radius.
    Sphere,
    /// Along the entity's local Y: `size.x` the radius, `size.y` the half
    /// height of the cylindrical part (total height `2 * (y + x)`).
    Capsule,
    /// The triangles of `mesh` (a cooked mesh sub-asset), static bodies
    /// only, at most [`crate::MAX_MESH_COLLIDER_TRIANGLES`] triangles.
    Mesh,
}

/// A collision shape on the entity, in the entity's local frame scaled by
/// the entity's world scale. Alone it is static geometry; with a
/// `RigidBody` it is that body's shape.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 11, name = "fynkel.Collider")]
pub struct Collider {
    pub shape: ColliderShape,
    /// Metres; what each component means depends on `shape`.
    pub size: Vec3,
    /// The mesh sub-asset for `shape: mesh` (`mesh/<n>` of an imported
    /// glTF), ignored otherwise.
    pub mesh: Option<AssetRef>,
    /// Local translation of the shape from the entity's origin, metres.
    pub offset: Vec3,
    /// A `.physicsmaterial.json` (ADR-0025): its friction, bounciness and
    /// combine rules replace `friction` and `restitution`. None: those two
    /// apply, averaged with the other collider's.
    pub material: Option<AssetRef>,
    /// Coulomb friction coefficient, `>= 0` (0.5 is rubber on concrete);
    /// ignored when `material` names one.
    pub friction: f32,
    /// Bounciness `0..=1`; ignored when `material` names one.
    pub restitution: f32,
    /// A sensor reports trigger events and applies no forces.
    pub is_sensor: bool,
    /// Which layers this collider collides with (bit `i` = the project's
    /// `layers[i]`; both sides must agree). Its own layer is its entity's
    /// `fynkel.Layer` (ADR-0020).
    pub mask: u32,
}

impl Default for Collider {
    fn default() -> Self {
        Self {
            shape: ColliderShape::Box,
            size: Vec3::splat(0.5),
            mesh: None,
            offset: Vec3::ZERO,
            material: None,
            friction: 0.5,
            restitution: 0.0,
            is_sensor: false,
            mask: u32::MAX,
        }
    }
}

fn finite_non_negative(what: &str, v: f32) -> Result<(), String> {
    if !v.is_finite() || v < 0.0 {
        return Err(format!("{what} must be finite and >= 0, got {v}"));
    }
    Ok(())
}

fn finite_positive(what: &str, v: f32) -> Result<(), String> {
    if !v.is_finite() || v <= 0.0 {
        return Err(format!("{what} must be finite and > 0, got {v}"));
    }
    Ok(())
}

impl RigidBody {
    /// Value rules the schema cannot express (the `INVALID_PAYLOAD` text).
    pub fn validate(&self) -> Result<(), String> {
        if self.body_type == BodyType::Dynamic {
            finite_positive("mass", self.mass)?;
        } else if !self.mass.is_finite() || self.mass < 0.0 {
            return Err(format!("mass must be finite and >= 0, got {}", self.mass));
        }
        finite_non_negative("linear_damping", self.linear_damping)?;
        finite_non_negative("angular_damping", self.angular_damping)?;
        if !self.gravity_scale.is_finite() {
            return Err(format!(
                "gravity_scale must be finite, got {}",
                self.gravity_scale
            ));
        }
        Ok(())
    }
}

impl Collider {
    /// Value rules the schema cannot express (the `INVALID_PAYLOAD` text).
    pub fn validate(&self) -> Result<(), String> {
        if !self.size.is_finite() || !self.offset.is_finite() {
            return Err("size and offset must be finite".into());
        }
        match self.shape {
            ColliderShape::Box => {
                if self.size.min_element() <= 0.0 {
                    return Err(format!(
                        "box half extents must be > 0 on every axis, got {:?}",
                        self.size
                    ));
                }
            }
            ColliderShape::Sphere => finite_positive("sphere radius (size.x)", self.size.x)?,
            ColliderShape::Capsule => {
                finite_positive("capsule radius (size.x)", self.size.x)?;
                finite_non_negative("capsule half height (size.y)", self.size.y)?;
            }
            ColliderShape::Mesh => {
                if self.mesh.is_none() {
                    return Err("a mesh collider needs `mesh`".into());
                }
            }
        }
        finite_non_negative("friction", self.friction)?;
        if !self.restitution.is_finite() || !(0.0..=1.0).contains(&self.restitution) {
            return Err(format!(
                "restitution must be in 0..=1, got {}",
                self.restitution
            ));
        }
        Ok(())
    }
}

/// A kinematic capsule walked by gameplay (blueprint §19.2d item 9, over
/// Rapier's kinematic character controller). The entity's origin is the
/// capsule's centre minus `center` — with the default `center` the origin
/// is at the feet. Moved by `move`/`jump` commands (one `move` per tick;
/// no command is standing still) or, with `move_action`/`jump_action`
/// named, by the runtime itself from those actions (walking relative to
/// the `fynkel.ThirdPersonCamera` that targets the entity, if any). Slopes
/// up to `max_slope` are climbed, steeper ones slid down; steps up to
/// `max_step_height` are climbed; gravity accumulates while airborne and
/// the character snaps to ground within `snap_to_ground`. Ground contact,
/// sliding and the effective velocity are held state
/// (`characterState()`). Its own kinematic capsule replaces any
/// `RigidBody`/`Collider` on the entity (`FYN-H0006`), collides by
/// its object's `fynkel.Layer` and its `mask`, reports contact and trigger events and pushes dynamic
/// bodies as a body of `mass` kilograms would.
#[derive(Clone, Debug, PartialEq, Schema)]
#[schema(kind = "component", id = 12, name = "fynkel.CharacterController")]
pub struct CharacterController {
    /// Capsule radius, metres, `> 0`.
    pub radius: f32,
    /// Total capsule height (both caps included), `>= 2 * radius`.
    pub height: f32,
    /// The capsule's centre in the entity's local frame (scaled by the
    /// entity's scale, like a collider offset).
    pub center: Vec3,
    /// Skin the controller keeps from geometry, metres, `>= 0`.
    pub skin_width: f32,
    /// Steepest climbable slope, radians in `0..pi/2`; steeper ground
    /// slides the character down.
    pub max_slope: f32,
    /// Highest step climbed without a jump, metres (`0` disables).
    pub max_step_height: f32,
    /// How far below the feet ground still counts as touched when walking
    /// off a bump, metres (`0` disables).
    pub snap_to_ground: f32,
    /// Multiplies the world's gravity; `0` is a flying character whose
    /// `move` velocity is used in full (vertical included).
    pub gravity_scale: f32,
    /// Kilograms the character pushes dynamic bodies with (`0` pushes
    /// nothing).
    pub mass: f32,
    /// Walk speed of the runtime driver (`move_action`), m/s.
    pub speed: f32,
    /// Take-off speed of a `jump` without an explicit speed, m/s.
    pub jump_speed: f32,
    /// Radians per second the entity turns towards its horizontal motion
    /// (`0` leaves the rotation to gameplay).
    pub turn_speed: f32,
    /// A 2D action walked by the runtime every tick at `speed` (x right,
    /// y forward, relative to the camera that targets this entity);
    /// empty leaves movement to `move` commands.
    pub move_action: String,
    /// A button action whose press jumps when grounded; empty leaves it
    /// to `jump` commands.
    pub jump_action: String,
    /// Which layers the capsule collides with and walks on (bit `i` = the
    /// project's `layers[i]`); its own layer is its entity's `fynkel.Layer`
    /// (ADR-0020).
    pub mask: u32,
}

impl Default for CharacterController {
    fn default() -> Self {
        Self {
            radius: 0.4,
            height: 1.8,
            center: Vec3::new(0.0, 0.9, 0.0),
            skin_width: 0.02,
            max_slope: core::f32::consts::FRAC_PI_4,
            max_step_height: 0.3,
            snap_to_ground: 0.2,
            gravity_scale: 1.0,
            mass: 70.0,
            speed: 4.0,
            jump_speed: 5.0,
            turn_speed: 10.0,
            move_action: String::new(),
            jump_action: String::new(),
            mask: u32::MAX,
        }
    }
}

impl CharacterController {
    /// Value rules the schema cannot express (the `INVALID_PAYLOAD` text).
    pub fn validate(&self) -> Result<(), String> {
        finite_positive("radius", self.radius)?;
        if !self.height.is_finite() || self.height < 2.0 * self.radius {
            return Err(format!(
                "height must be finite and >= 2 * radius, got {} < {}",
                self.height,
                2.0 * self.radius
            ));
        }
        if !self.center.is_finite() {
            return Err("center must be finite".into());
        }
        finite_non_negative("skin_width", self.skin_width)?;
        if !self.max_slope.is_finite()
            || self.max_slope < 0.0
            || self.max_slope >= core::f32::consts::FRAC_PI_2
        {
            return Err(format!(
                "max_slope must be in 0..pi/2 radians, got {}",
                self.max_slope
            ));
        }
        finite_non_negative("max_step_height", self.max_step_height)?;
        finite_non_negative("snap_to_ground", self.snap_to_ground)?;
        if !self.gravity_scale.is_finite() {
            return Err(format!(
                "gravity_scale must be finite, got {}",
                self.gravity_scale
            ));
        }
        finite_non_negative("mass", self.mass)?;
        finite_non_negative("speed", self.speed)?;
        finite_non_negative("jump_speed", self.jump_speed)?;
        finite_non_negative("turn_speed", self.turn_speed)?;
        Ok(())
    }

    /// Half height of the cylindrical part.
    pub fn half_height(&self) -> f32 {
        (self.height * 0.5 - self.radius).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::schema::{FieldType, HasEnumSchema, HasSchema};

    #[test]
    fn character_layout_and_rules() {
        let s = &CharacterController::SCHEMA;
        assert_eq!(s.id, 12);
        assert_eq!(s.name, "fynkel.CharacterController");
        // radius @0, height @4, center 12 @8, skin @20, slope @24, step @28,
        // snap @32, gravity @36, mass @40, speed @44, jump @48, turn @52,
        // move_action (u32,u32) @56, jump_action @64, mask @72 → 80.
        assert_eq!(
            s.offsets(),
            vec![0, 4, 8, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 64, 72]
        );
        assert_eq!(s.wire_size(), 80);
        assert!(s.has_strings());
        assert!(matches!(s.fields[12].ty, FieldType::Str));
        let c = CharacterController::default();
        assert!(c.validate().is_ok());
        assert!((c.half_height() - 0.5).abs() < 1e-6);
        assert!(CharacterController {
            height: 0.5,
            ..CharacterController::default()
        }
        .validate()
        .is_err());
        assert!(CharacterController {
            max_slope: 2.0,
            ..CharacterController::default()
        }
        .validate()
        .is_err());
        assert!(CharacterController {
            mass: -1.0,
            ..CharacterController::default()
        }
        .validate()
        .is_err());
        assert!(CharacterController {
            radius: f32::NAN,
            ..CharacterController::default()
        }
        .validate()
        .is_err());
    }

    #[test]
    fn layouts_and_ids() {
        assert_eq!(RigidBody::SCHEMA.id, 10);
        assert_eq!(RigidBody::SCHEMA.name, "fynkel.RigidBody");
        // body_type u32 @0, mass @4, linear @8, angular @12, gravity @16, ccd @20 → 24.
        assert_eq!(RigidBody::SCHEMA.offsets(), vec![0, 4, 8, 12, 16, 20]);
        assert_eq!(RigidBody::SCHEMA.wire_size(), 24);
        assert_eq!(Collider::SCHEMA.id, 11);
        // shape @0, size 12 @4, mesh 24 @16, offset 12 @40, material 24
        // @56 (ADR-0025; an asset ref aligns to 8), friction @80,
        // restitution @84, is_sensor @88, mask @92 → 96.
        assert_eq!(
            Collider::SCHEMA.offsets(),
            vec![0, 4, 16, 40, 56, 80, 84, 88, 92]
        );
        assert_eq!(Collider::SCHEMA.wire_size(), 96);
        for i in [2, 4] {
            assert!(matches!(
                Collider::SCHEMA.fields[i].ty,
                FieldType::OptionalAssetRef
            ));
        }
        assert_eq!(BodyType::ENUM.variants, &["static", "dynamic", "kinematic"]);
        assert_eq!(
            ColliderShape::ENUM.variants,
            &["box", "sphere", "capsule", "mesh"]
        );
    }

    #[test]
    fn value_rules() {
        assert!(RigidBody::default().validate().is_ok());
        assert!(RigidBody {
            mass: 0.0,
            ..RigidBody::default()
        }
        .validate()
        .is_err());
        assert!(RigidBody {
            body_type: BodyType::Static,
            mass: 0.0,
            ..RigidBody::default()
        }
        .validate()
        .is_ok());
        assert!(RigidBody {
            linear_damping: -1.0,
            ..RigidBody::default()
        }
        .validate()
        .is_err());
        assert!(Collider::default().validate().is_ok());
        assert!(Collider {
            size: Vec3::new(1.0, 0.0, 1.0),
            ..Collider::default()
        }
        .validate()
        .is_err());
        assert!(Collider {
            shape: ColliderShape::Sphere,
            size: Vec3::new(1.0, 0.0, 0.0),
            ..Collider::default()
        }
        .validate()
        .is_ok());
        assert!(Collider {
            shape: ColliderShape::Mesh,
            ..Collider::default()
        }
        .validate()
        .is_err());
        assert!(Collider {
            restitution: 1.5,
            ..Collider::default()
        }
        .validate()
        .is_err());
    }
}
