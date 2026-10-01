//! A game-owned type: no Health branch or ID in the engine catalogue.
use fynkel_core::{ComponentSchema, RegisteredComponent, RegisteredField, ScalarValue};

#[derive(Debug, PartialEq)]
pub struct Health {
    pub hp: f64,
    pub invulnerable: bool,
}
impl RegisteredComponent for Health {
    fn encode(&self) -> Vec<ScalarValue> {
        vec![
            ScalarValue::Float(self.hp),
            ScalarValue::Bool(self.invulnerable),
        ]
    }
    fn decode(fields: &[ScalarValue]) -> Result<Self, String> {
        match fields {
            [ScalarValue::Float(hp), ScalarValue::Bool(invulnerable)]
                if hp.is_finite() && *hp >= 0.0 =>
            {
                Ok(Self {
                    hp: *hp,
                    invulnerable: *invulnerable,
                })
            }
            _ => {
                Err("Health requires finite nonnegative hp and a boolean invulnerable flag".into())
            }
        }
    }
}
pub fn schema() -> ComponentSchema {
    ComponentSchema {
        id: "example.Health".into(),
        version: 1,
        fields: vec![
            RegisteredField {
                name: "hp".into(),
                default: ScalarValue::Float(100.0),
            },
            RegisteredField {
                name: "invulnerable".into(),
                default: ScalarValue::Bool(false),
            },
        ],
    }
}
