//! Owns the asset store, residency tokens and cancellable background work.
use crate::load;
use fynkel_contracts::Entity;
#[derive(Default)]
pub(crate) struct ResourceServices {
    /// The cache the last load opened (a renderer drawing this world adopts
    /// it, so both see one residency store).
    pub(crate) assets: Option<fynkel_assets::Assets>,
    /// One token per loaded scene (keyed by its id): the closure stays
    /// resident until the scene is unloaded, a replace load empties the
    /// world, or the world is destroyed (M3-12).
    pub(crate) residency: Vec<(Option<fynkel_contracts::Uuid>, fynkel_assets::Residency)>,
    /// The prefabs this world instantiated, kept in memory after the first
    /// (Unity's `Instantiate` is an in-memory copy, foundation 3.1): read
    /// once, their meshes, nested prefabs, clips and sounds registered
    /// once, their closure held resident by one token each. Emptied by a
    /// replace load and when the world's store changes.
    pub(crate) prefabs:
        std::collections::HashMap<fynkel_contracts::Uuid, std::sync::Arc<load::LoadedPrefab>>,
    /// The roots the last `load_project` made from the project's
    /// `persistentObjects` (ADR-0039); the next one destroys them first.
    pub(crate) project_persistent: Vec<Entity>,
    /// The background loads not activated yet, by ticket (ADR-0038);
    /// dropping one stops its thread at the next asset.
    pub(crate) loads: std::collections::HashMap<u32, load::PendingLoad>,
    pub(crate) next_ticket: u32,
    /// Action → bindings the user chose over the project's
    /// (`fynkel_world_bind_actions`, M3-12), re-applied after every load.
    pub(crate) rebinds: std::collections::BTreeMap<String, Vec<String>>,
}

impl ResourceServices {
    pub(crate) fn adopt(&mut self, assets: fynkel_assets::Assets) {
        if let Some(old) = &self.assets {
            assets.adopt_runtime_assets(old);
            if !old.same_store(&assets) {
                self.prefabs.clear();
            }
        }
        self.assets = Some(assets);
    }
}
