//! An optional, statically linked package. No engine catalogue branch.
use fynkel_core::*;
pub struct Combat;
#[derive(Debug)]
pub struct Health {
    pub hp: f64,
}
impl RegisteredComponent for Health {
    fn encode(&self) -> Vec<ScalarValue> {
        vec![ScalarValue::Float(self.hp)]
    }
    fn decode(fields: &[ScalarValue]) -> Result<Self, String> {
        match fields {
            [ScalarValue::Float(hp)] if hp.is_finite() && *hp >= 0. => Ok(Self { hp: *hp }),
            _ => Err("invalid health".into()),
        }
    }
}
pub fn health_schema() -> ComponentSchema {
    ComponentSchema {
        id: "example.combat.Health".into(),
        version: 1,
        fields: vec![RegisteredField {
            name: "hp".into(),
            default: ScalarValue::Float(100.),
        }],
    }
}
impl NativeExtension for Combat {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: "example.combat".into(),
            version: 1,
            api_version: EXTENSION_API_VERSION,
            runtime_abi: fynkel_contracts::ABI_VERSION,
            requires: vec![],
        }
    }
    fn register(&self, r: &mut ExtensionRegistrar) -> Result<(), ExtensionError> {
        r.component::<Health>(health_schema())?;
        let mut spec = SystemSpec::new("example.combat.Damage", SystemPhase::Simulation);
        spec.writes.push("example.combat.Health".into());
        r.system(RegisteredSystem { spec, run: damage })
    }
}
fn damage(edit: &mut GameplayEdit<'_>, time: SystemContext) -> EditResult<u32> {
    let batch = edit.query_registered("example.combat.Health")?;
    let values = batch
        .rows()
        .iter()
        .map(|(_, v)| match v.as_slice() {
            [ScalarValue::Float(hp)] => vec![ScalarValue::Float(
                (hp - f64::from(time.delta_seconds) * 10.).max(0.),
            )],
            _ => unreachable!(),
        })
        .collect();
    edit.apply_registered_batch(&batch, values)?;
    Ok(batch.rows().len() as u32)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incompatible_or_failed_package_installation_leaves_no_partial_registry() {
        struct Broken {
            incompatible: bool,
        }
        impl NativeExtension for Broken {
            fn manifest(&self) -> ExtensionManifest {
                let mut m = Combat.manifest();
                if self.incompatible {
                    m.api_version += 1;
                }
                m
            }
            fn register(&self, r: &mut ExtensionRegistrar) -> Result<(), ExtensionError> {
                assert!(!self.incompatible, "incompatible package code was executed");
                r.component::<Health>(health_schema())?;
                let mut spec = SystemSpec::new("example.combat.Broken", SystemPhase::Simulation);
                spec.after.push("example.combat.Missing".into());
                r.system(RegisteredSystem { spec, run: damage })
            }
        }
        let mut w = World::new(Default::default());
        let systems = w.registered_systems().count();
        for incompatible in [true, false] {
            assert!(w.install_extension(&Broken { incompatible }).is_err());
            assert_eq!(w.registered_schemas().count(), 0);
            assert_eq!(w.registered_systems().count(), systems);
        }
        w.install_extension(&Combat).unwrap();
    }
    #[test]
    fn linked_package_uses_same_storage_as_host_and_survives_stripping() {
        let mut w = World::new(Default::default());
        w.install_extension(&Combat).unwrap();
        // Host metadata registration adopts the native codec/storage, never replaces it.
        w.register_dynamic_component(health_schema()).unwrap();
        let c = w
            .edit_gameplay(|e| {
                let p = e.spawn("p", Transform::IDENTITY, None)?;
                e.add_registered_defaults(p, "example.combat.Health")?;
                Ok(p)
            })
            .unwrap();
        let p = c.resolve(c.value).unwrap();
        assert_eq!(w.step().rejected, 0);
        assert!(w.ecs().get::<Health>(p).unwrap().hp < 100.);
        assert!(w.install_extension(&Combat).is_err());
    }
    #[test]
    fn editor_never_executes_package_registration() {
        struct NoCode;
        impl NativeExtension for NoCode {
            fn manifest(&self) -> ExtensionManifest {
                panic!("editor invoked plugin")
            }
            fn register(&self, _: &mut ExtensionRegistrar) -> Result<(), ExtensionError> {
                panic!("editor invoked plugin")
            }
        }
        let mut w = World::new(WorldConfig {
            edit_mode: true,
            ..Default::default()
        });
        assert!(matches!(
            w.install_extension(&NoCode),
            Err(ExtensionError::EditorIsolation)
        ));
    }
}
