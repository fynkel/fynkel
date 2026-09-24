//! Physics materials (ADR-0025, `docs/physics.md` § Physics materials):
//! the friction and bounciness a `<name>.physicsmaterial.json` document
//! gives every `fynkel.Collider` that names it, with the rules that
//! combine two touching colliders' values. Unity's Physics Material, on
//! Rapier's coefficients: one Coulomb friction (Rapier has no separate
//! static coefficient) and a restitution.

/// How two touching colliders' coefficients become the contact's. When
/// the two colliders ask for different rules, the one later in this list
/// wins (`average < minimum < multiply < maximum`) — Unity's priority and
/// Rapier's own (`CoefficientCombineRule`, whose `Ord` is the same order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum CombineRule {
    /// `(a + b) / 2`.
    #[default]
    Average,
    /// `min(a, b)`: the slippery (or dead) side wins.
    Minimum,
    /// `a × b`.
    Multiply,
    /// `max(a, b)`: the grippy (or bouncy) side wins.
    Maximum,
}

impl CombineRule {
    pub const ALL: [CombineRule; 4] = [
        CombineRule::Average,
        CombineRule::Minimum,
        CombineRule::Multiply,
        CombineRule::Maximum,
    ];

    /// The document's spelling.
    pub const fn name(self) -> &'static str {
        match self {
            CombineRule::Average => "average",
            CombineRule::Minimum => "minimum",
            CombineRule::Multiply => "multiply",
            CombineRule::Maximum => "maximum",
        }
    }

    pub fn parse(name: &str) -> Option<CombineRule> {
        CombineRule::ALL.into_iter().find(|r| r.name() == name)
    }

    /// The contact's coefficient from two colliders' values and rules
    /// (what Rapier computes; kept here so tests and tools can say what a
    /// pair will do without a world).
    pub fn combine(a: f32, rule_a: CombineRule, b: f32, rule_b: CombineRule) -> f32 {
        match rule_a.max(rule_b) {
            CombineRule::Average => (a + b) / 2.0,
            CombineRule::Minimum => a.min(b),
            CombineRule::Multiply => a * b,
            CombineRule::Maximum => a.max(b),
        }
    }

    pub(crate) fn rapier(self) -> rapier3d::prelude::CoefficientCombineRule {
        use rapier3d::prelude::CoefficientCombineRule as R;
        match self {
            CombineRule::Average => R::Average,
            CombineRule::Minimum => R::Min,
            CombineRule::Multiply => R::Multiply,
            CombineRule::Maximum => R::Max,
        }
    }
}

/// A physics material's values; the defaults are Unity's default
/// material (dynamic friction 0.6, bounciness 0, both averaged).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsMaterial {
    /// Coulomb friction coefficient, `>= 0`.
    pub friction: f32,
    /// Restitution, `0..=1`: 0 stops dead, 1 bounces back as fast.
    pub bounciness: f32,
    pub friction_combine: CombineRule,
    pub bounce_combine: CombineRule,
}

impl Default for PhysicsMaterial {
    fn default() -> Self {
        Self {
            friction: DEFAULT_FRICTION,
            bounciness: DEFAULT_BOUNCINESS,
            friction_combine: CombineRule::Average,
            bounce_combine: CombineRule::Average,
        }
    }
}

/// Unity's default dynamic friction.
pub const DEFAULT_FRICTION: f32 = 0.6;
pub const DEFAULT_BOUNCINESS: f32 = 0.0;

impl PhysicsMaterial {
    /// Value rules (the schema holds the same ranges).
    pub fn validate(&self) -> Result<(), String> {
        if !self.friction.is_finite() || self.friction < 0.0 {
            return Err(format!(
                "friction must be finite and >= 0, got {}",
                self.friction
            ));
        }
        if !self.bounciness.is_finite() || !(0.0..=1.0).contains(&self.bounciness) {
            return Err(format!(
                "bounciness must be in 0..=1, got {}",
                self.bounciness
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_higher_rule_wins_in_unitys_and_rapiers_order() {
        use rapier3d::prelude::CoefficientCombineRule as R;
        // The order is Rapier's too, so its `max` picks the same rule.
        for a in CombineRule::ALL {
            for b in CombineRule::ALL {
                assert_eq!(a.max(b).rapier(), a.rapier().max(b.rapier()));
            }
        }
        assert_eq!(CombineRule::Average.rapier(), R::default());
        let c = |ra, rb| CombineRule::combine(0.2, ra, 0.8, rb);
        assert_eq!(c(CombineRule::Average, CombineRule::Average), 0.5);
        assert_eq!(c(CombineRule::Average, CombineRule::Minimum), 0.2);
        assert_eq!(c(CombineRule::Multiply, CombineRule::Minimum), 0.2 * 0.8);
        assert_eq!(c(CombineRule::Minimum, CombineRule::Maximum), 0.8);
        assert_eq!(CombineRule::parse("maximum"), Some(CombineRule::Maximum));
        assert_eq!(CombineRule::parse("max"), None);
    }

    #[test]
    fn defaults_are_unitys_and_ranges_hold() {
        let m = PhysicsMaterial::default();
        assert_eq!((m.friction, m.bounciness), (0.6, 0.0));
        assert!(m.validate().is_ok());
        let bad = |f, b| {
            PhysicsMaterial {
                friction: f,
                bounciness: b,
                ..m
            }
            .validate()
            .is_err()
        };
        assert!(bad(-0.1, 0.0));
        assert!(bad(f32::NAN, 0.0));
        assert!(bad(0.5, 1.5));
        assert!(!bad(2.0, 1.0));
    }
}
