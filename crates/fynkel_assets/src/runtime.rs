//! Reserved runtime-payload publication. Admission precedes gameplay commit.
use super::*;

#[derive(Clone)]
pub enum RuntimeAssetUpdate {
    Material(AssetRef, Option<PbrMaterial>),
    CustomMaterial(AssetRef, Option<CustomMaterialPayload>),
    Mesh(AssetRef, Option<MeshPayload>),
    Clip(AssetRef, Option<Clip>),
}
impl RuntimeAssetUpdate {
    fn key(&self) -> (&AssetRef, u8) {
        match self {
            Self::Material(r, _) | Self::CustomMaterial(r, _) => (r, 0),
            Self::Mesh(r, _) => (r, 1),
            Self::Clip(r, _) => (r, 2),
        }
    }
    fn bytes(&self) -> u64 {
        match self {
            Self::Material(_, v) => v.as_ref().map_or(0, budget::material_bytes),
            Self::CustomMaterial(_, v) => v.as_ref().map_or(0, budget::custom_bytes),
            Self::Mesh(_, v) => v.as_ref().map_or(0, budget::mesh_bytes),
            Self::Clip(_, v) => v.as_ref().map_or(0, budget::clip_bytes),
        }
    }
}
/// Exclusive reservation for a complete runtime edit. Drop aborts admission;
/// publication performs no I/O, validation or fallible capacity checks.
pub struct PreparedRuntimeAssets {
    assets: Assets,
    updates: Vec<RuntimeAssetUpdate>,
}
impl PreparedRuntimeAssets {
    pub fn publish(mut self) {
        let mut store = self.assets.lock();
        for update in std::mem::take(&mut self.updates) {
            match update {
                RuntimeAssetUpdate::Material(r, v) => store.store_runtime_material(&r, v),
                RuntimeAssetUpdate::CustomMaterial(r, v) => {
                    store.store_runtime_custom_material(&r, v)
                }
                RuntimeAssetUpdate::Mesh(r, v) => store.store_runtime_mesh(&r, v),
                RuntimeAssetUpdate::Clip(r, v) => store.store_runtime_clip(&r, v),
            }
        }
    }
}
impl Drop for PreparedRuntimeAssets {
    fn drop(&mut self) {
        self.assets.lock().runtime_reserved = false;
    }
}
impl Store {
    pub(crate) fn runtime_costs(&self) -> HashMap<(AssetRef, u8), u64> {
        let mut costs = HashMap::new();
        costs.extend(
            self.runtime_materials
                .iter()
                .map(|(r, m)| ((r.clone(), 0), budget::material_bytes(m))),
        );
        costs.extend(
            self.runtime_custom_materials
                .iter()
                .map(|(r, m)| ((r.clone(), 0), budget::custom_bytes(m))),
        );
        costs.extend(
            self.runtime_meshes
                .iter()
                .map(|(r, m)| ((r.clone(), 1), budget::mesh_bytes(m))),
        );
        costs.extend(
            self.runtime_clips
                .iter()
                .map(|(r, m)| ((r.clone(), 2), budget::clip_bytes(m))),
        );
        costs
    }
}
impl Assets {
    pub fn runtime_bytes(&self) -> u64 {
        self.lock().runtime_costs().values().sum()
    }
    pub fn runtime_budget(&self) -> u64 {
        self.lock().runtime_budget_bytes
    }
    pub fn set_runtime_budget(&self, bytes: u64) -> Result<(), Diagnostic> {
        let mut s = self.lock();
        if s.runtime_reserved || s.runtime_costs().values().sum::<u64>() > bytes {
            return Err(Diagnostic::error(
                Code::IO,
                "runtime budget is below owned or reserved payload residency",
            ));
        }
        s.runtime_budget_bytes = bytes;
        Ok(())
    }
    pub fn prepare_runtime(
        &self,
        updates: Vec<RuntimeAssetUpdate>,
    ) -> Result<PreparedRuntimeAssets, Diagnostic> {
        let mut s = self.lock();
        if s.runtime_reserved {
            return Err(Diagnostic::error(
                Code::IO,
                "runtime asset publication is already reserved",
            ));
        }
        let mut costs = s.runtime_costs();
        for update in &updates {
            let (r, kind) = update.key();
            if let RuntimeAssetUpdate::Clip(_, Some(clip)) = update {
                clip.validate().map_err(|e| {
                    Diagnostic::error(Code::RUNTIME_PAYLOAD_MALFORMED, e)
                        .with_id(r.asset.to_string())
                })?;
            }
            costs.insert((r.clone(), kind), update.bytes());
        }
        let bytes = costs.values().try_fold(0u64, |sum, n| sum.checked_add(*n));
        if bytes.is_none_or(|n| n > s.runtime_budget_bytes) {
            return Err(Diagnostic::error(
                Code::IO,
                format!(
                    "runtime asset budget exhausted: proposed {} bytes, limit {}",
                    bytes.unwrap_or(u64::MAX),
                    s.runtime_budget_bytes
                ),
            ));
        }
        s.runtime_reserved = true;
        Ok(PreparedRuntimeAssets {
            assets: self.clone(),
            updates,
        })
    }
    pub fn set_runtime_material(
        &self,
        r: &AssetRef,
        m: Option<PbrMaterial>,
    ) -> Result<(), Diagnostic> {
        self.prepare_runtime(vec![RuntimeAssetUpdate::Material(r.clone(), m)])?
            .publish();
        Ok(())
    }
    pub fn set_runtime_custom_material(
        &self,
        r: &AssetRef,
        m: Option<CustomMaterialPayload>,
    ) -> Result<(), Diagnostic> {
        self.prepare_runtime(vec![RuntimeAssetUpdate::CustomMaterial(r.clone(), m)])?
            .publish();
        Ok(())
    }
    pub fn set_runtime_mesh(&self, r: &AssetRef, m: Option<MeshPayload>) -> Result<(), Diagnostic> {
        self.prepare_runtime(vec![RuntimeAssetUpdate::Mesh(r.clone(), m)])?
            .publish();
        Ok(())
    }
    pub fn set_runtime_clip(&self, r: &AssetRef, m: Option<Clip>) -> Result<(), Diagnostic> {
        self.prepare_runtime(vec![RuntimeAssetUpdate::Clip(r.clone(), m)])?
            .publish();
        Ok(())
    }
}

impl Store {
    fn store_runtime_material(&mut self, r: &AssetRef, m: Option<PbrMaterial>) {
        let s = self;
        s.invalidate_asset(r);
        match m {
            Some(m) => s.runtime_materials.insert(r.clone(), Arc::new(m)),
            None => s.runtime_materials.remove(r),
        };
        if s.runtime_custom_materials.remove(r).is_some() {
            s.note_material_change(r);
        }
        s.live_edits += 1;
    }
    fn store_runtime_custom_material(&mut self, r: &AssetRef, m: Option<CustomMaterialPayload>) {
        let s = self;
        s.invalidate_asset(r);
        match m {
            Some(m) => s.runtime_custom_materials.insert(r.clone(), Arc::new(m)),
            None => s.runtime_custom_materials.remove(r),
        };
        if s.runtime_materials.remove(r).is_some() {
            s.live_edits += 1;
        }
        s.note_material_change(r);
    }
    fn store_runtime_mesh(&mut self, r: &AssetRef, m: Option<MeshPayload>) {
        let s = self;
        s.invalidate_asset(r);
        match m {
            Some(mut m) => {
                let (min, max) = bounds_of(&m.vertices);
                m.bounds_min = min;
                m.bounds_max = max;
                s.runtime_meshes.insert(r.clone(), Arc::new(m))
            }
            None => s.runtime_meshes.remove(r),
        };
        s.meshes.remove(r);
        s.cache_costs.remove(&(r.clone(), KIND_MESH));
        s.mesh_edits += 1;
        let at = s.mesh_edits;
        s.mesh_changes.insert(r.clone(), at);
    }
    fn store_runtime_clip(&mut self, r: &AssetRef, c: Option<Clip>) {
        let s = self;
        s.invalidate_asset(r);
        match c {
            Some(c) => s.runtime_clips.insert(r.clone(), Arc::new(c)),
            None => s.runtime_clips.remove(r),
        };
    }
}
