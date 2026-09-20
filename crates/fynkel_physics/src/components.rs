//! The physics components (blueprint §9, §19.2d item 8): pure data,
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
    /// Coulomb friction coefficient, `>= 0` (0.5 is rubber on concrete).
    pub friction: f32,
    /// Bounciness `0..=1`.
    pub restitution: f32,
    /// A sensor reports trigger events and applies no forces.
    pub is_sensor: bool,
    /// Membership bits (bit `i` = layer `i` of the project's
    /// `physics.layers`); default layer 0.
    pub layer: u32,
    /// Which layers this collider collides with (both sides must agree).
    pub mask: u32,
}

impl Default for Collider {
    fn default() -> Self {
        Self {
            shape: ColliderShape::Box,
            size: Vec3::splat(0.5),
            mesh: None,
            offset: Vec3::ZERO,
            friction: 0.5,
            restitution: 0.0,
            is_sensor: false,
            layer: 1,
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

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_contracts::schema::{FieldType, HasEnumSchema, HasSchema};

    #[test]
    fn layouts_and_ids() {
        assert_eq!(RigidBody::SCHEMA.id, 10);
        assert_eq!(RigidBody::SCHEMA.name, "fynkel.RigidBody");
        // body_type u32 @0, mass @4, linear @8, angular @12, gravity @16, ccd @20 → 24.
        assert_eq!(RigidBody::SCHEMA.offsets(), vec![0, 4, 8, 12, 16, 20]);
        assert_eq!(RigidBody::SCHEMA.wire_size(), 24);
        assert_eq!(Collider::SCHEMA.id, 11);
        // shape @0, size 12 @4, mesh 24 @16, offset 12 @40, friction @52,
        // restitution @56, is_sensor @60, layer @64, mask @68 → 72.
        assert_eq!(
            Collider::SCHEMA.offsets(),
            vec![0, 4, 16, 40, 52, 56, 60, 64, 68]
        );
        assert_eq!(Collider::SCHEMA.wire_size(), 72);
        assert!(matches!(
            Collider::SCHEMA.fields[2].ty,
            FieldType::OptionalAssetRef
        ));
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
