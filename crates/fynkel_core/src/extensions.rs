//! Build-time linked native packages. This is a Rust source API, not a binary
//! plugin ABI: packages share the host's locked engine revision/toolchain.
use crate::{
    ComponentSchema, Module, RegisteredComponent, RegisteredSystem, RegistryError, ScheduleError,
    World,
};
pub const EXTENSION_API_VERSION: u32 = 1;
#[derive(Clone, Debug)]
pub struct ExtensionManifest {
    pub id: String,
    pub version: u32,
    pub api_version: u32,
    pub runtime_abi: u16,
    pub requires: Vec<Module>,
}
#[derive(Clone, Debug)]
pub enum ExtensionError {
    InvalidManifest,
    Incompatible,
    MissingModule(Module),
    EditorIsolation,
    AlreadyInstalled,
    Registry(RegistryError),
    Schedule(ScheduleError),
    Namespace(String),
    Poisoned,
}
impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "extension: {self:?}")
    }
}
impl std::error::Error for ExtensionError {}
/// Registration can declare metadata/codecs and systems, but has no mutable
/// world handle. A failed declaration batch cannot mutate live entity data.
pub struct ExtensionRegistrar {
    namespace: String,
    pub(crate) registry: crate::registry::ComponentRegistry,
    pub(crate) systems: Vec<RegisteredSystem>,
    failed: Option<ExtensionError>,
}
impl ExtensionRegistrar {
    fn check(&mut self, id: &str) -> Result<(), ExtensionError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if !id.starts_with(&(self.namespace.clone() + ".")) {
            let e = ExtensionError::Namespace(id.into());
            self.failed = Some(e.clone());
            return Err(e);
        }
        Ok(())
    }
    pub fn component<T: RegisteredComponent>(
        &mut self,
        schema: ComponentSchema,
    ) -> Result<(), ExtensionError> {
        self.check(&schema.id)?;
        let result = self
            .registry
            .register::<T>(schema)
            .map_err(ExtensionError::Registry);
        if let Err(e) = &result {
            self.failed = Some(e.clone());
        }
        result
    }
    pub fn system(&mut self, system: RegisteredSystem) -> Result<(), ExtensionError> {
        self.check(&system.spec.id)?;
        self.systems.push(system);
        Ok(())
    }
}
pub trait NativeExtension {
    fn manifest(&self) -> ExtensionManifest;
    fn register(&self, registration: &mut ExtensionRegistrar) -> Result<(), ExtensionError>;
}
impl World {
    pub fn install_extension(
        &mut self,
        extension: &impl NativeExtension,
    ) -> Result<(), ExtensionError> {
        if self.is_poisoned() {
            return Err(ExtensionError::Poisoned);
        }
        if self.config.edit_mode {
            return Err(ExtensionError::EditorIsolation);
        }
        let manifest = extension.manifest();
        if manifest.id.is_empty()
            || manifest.id == "fynkel"
            || manifest.id.starts_with("fynkel.")
            || manifest.version == 0
            || !manifest.id.split('.').all(crate::registry::identifier_word)
        {
            return Err(ExtensionError::InvalidManifest);
        }
        if manifest.api_version != EXTENSION_API_VERSION
            || manifest.runtime_abi != fynkel_contracts::ABI_VERSION
        {
            return Err(ExtensionError::Incompatible);
        }
        if self.extensions.contains_key(&manifest.id) {
            return Err(ExtensionError::AlreadyInstalled);
        }
        for &module in &manifest.requires {
            if !self.modules().has(module) {
                return Err(ExtensionError::MissingModule(module));
            }
        }
        let mut registrar = ExtensionRegistrar {
            namespace: manifest.id.clone(),
            registry: self.registered.clone(),
            systems: vec![],
            failed: None,
        };
        extension.register(&mut registrar)?;
        if let Some(e) = registrar.failed {
            return Err(e);
        }
        for system in &mut registrar.systems {
            for &module in &manifest.requires {
                if !system.spec.requires.contains(&module) {
                    system.spec.requires.push(module);
                }
            }
        }
        let before = std::mem::replace(&mut self.registered, registrar.registry);
        if let Err(e) = self.register_systems(registrar.systems) {
            self.registered = before;
            return Err(ExtensionError::Schedule(e));
        }
        self.extensions.insert(manifest.id, manifest.version);
        Ok(())
    }
}
