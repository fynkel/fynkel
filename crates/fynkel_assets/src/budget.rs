//! Admission accounting for decoded cooked payloads owned by the cache.
use super::*;

impl Store {
    pub(crate) fn generation(&self, id: Uuid) -> (u64, u64) {
        (
            self.cache_generation,
            self.asset_generations.get(&id).copied().unwrap_or(0),
        )
    }
    pub(crate) fn invalidate_asset(&mut self, r: &AssetRef) {
        self.compact_changes();
        *self.asset_generations.entry(r.asset).or_default() += 1;
        self.requests.retain(|(_, key), _| key.asset != r.asset);
    }
    pub(crate) fn check_generation(
        &self,
        id: Uuid,
        generation: (u64, u64),
    ) -> Result<(), Diagnostic> {
        if self.generation(id) == generation {
            Ok(())
        } else {
            Err(Diagnostic::error(
                Code::IO,
                "asset preparation invalidated by cache replacement; retry",
            )
            .with_id(id.to_string()))
        }
    }
    pub(crate) fn evict_ids(&mut self, gone: &HashSet<Uuid>) {
        if gone.is_empty() {
            return;
        }
        // In-flight direct decodes and delayed queue refusals must not publish
        // into a new residency lifetime of the same asset.
        for id in gone {
            self.invalidate_asset(&AssetRef::new(*id));
        }
        self.meshes.retain(|r, _| !gone.contains(&r.asset));
        self.textures.retain(|r, _| !gone.contains(&r.asset));
        self.environments.retain(|r, _| !gone.contains(&r.asset));
        self.materials.retain(|r, _| !gone.contains(&r.asset));
        self.custom_materials
            .retain(|r, _| !gone.contains(&r.asset));
        self.shaders.retain(|id, _| !gone.contains(id));
        self.clips.retain(|r, _| !gone.contains(&r.asset));
        self.blends.retain(|r, _| !gone.contains(&r.asset));
        self.controllers.retain(|r, _| !gone.contains(&r.asset));
        self.volumes.retain(|r, _| !gone.contains(&r.asset));
        self.physics_materials
            .retain(|r, _| !gone.contains(&r.asset));
        self.skins.retain(|r, _| !gone.contains(&r.asset));
        self.sounds.retain(|r, _| !gone.contains(&r.asset));
        self.metas.retain(|id, _| !gone.contains(id));
        self.prefab_docs.retain(|id, _| !gone.contains(id));
        self.cache_costs
            .retain(|(r, _), _| !gone.contains(&r.asset));
        self.warm.retain(|w| !gone.contains(&w.asset().asset));
        self.evictions += 1;
    }

    pub(crate) fn cache_bytes(&self) -> u64 {
        self.cache_costs.values().sum()
    }

    pub(crate) fn admit_replacement(
        &mut self,
        r: &AssetRef,
        kind: [u8; 4],
        previous: [u8; 4],
        bytes: u64,
    ) -> Result<(), Diagnostic> {
        let key = (r.clone(), previous);
        let old = self.cache_costs.remove(&key);
        if let Err(error) = self.admit_cache(r, kind, bytes) {
            if let Some(old) = old {
                self.cache_costs.insert(key, old);
            }
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn admit_cache(
        &mut self,
        r: &AssetRef,
        kind: [u8; 4],
        bytes: u64,
    ) -> Result<(), Diagnostic> {
        let key = (r.clone(), kind);
        let before = self.cache_costs.get(&key).copied().unwrap_or(0);
        let available = self
            .cache_budget_bytes
            .saturating_sub(self.cache_bytes().saturating_sub(before));
        if bytes > available {
            let gone = self
                .cache_costs
                .keys()
                .filter_map(|(key, _)| {
                    (key.asset != r.asset && !self.counts.contains_key(&key.asset))
                        .then_some(key.asset)
                })
                .collect();
            self.evict_ids(&gone);
        }
        if bytes
            > self
                .cache_budget_bytes
                .saturating_sub(self.cache_bytes().saturating_sub(before))
        {
            return Err(Diagnostic::error(
                Code::IO,
                format!(
                    "CPU asset cache budget exhausted: {r} needs {bytes} bytes, limit {} bytes",
                    self.cache_budget_bytes
                ),
            )
            .with_id(r.asset.to_string())
            .with_pointer(r.sub.clone()));
        }
        self.cache_costs.insert(key, bytes);
        Ok(())
    }
}
impl Assets {
    /// Limit decoded cooked payload residency. Excludes external Arc owners,
    /// runtime-owned assets, cache-index metadata and allocator overhead.
    /// Pinned data is never silently discarded; admission fails explicitly.
    pub fn set_cache_budget(&self, bytes: u64) -> Result<(), Diagnostic> {
        let mut s = self.lock();
        let gone = s
            .cache_costs
            .keys()
            .filter_map(|(r, _)| (!s.counts.contains_key(&r.asset)).then_some(r.asset))
            .collect();
        s.evict_ids(&gone);
        if s.cache_bytes() > bytes {
            return Err(Diagnostic::error(
                Code::IO,
                "CPU cache budget is below pinned payload residency",
            ));
        }
        s.cache_budget_bytes = bytes;
        // Budget failures can be retried after the host changes its limit.
        s.requests
            .retain(|_, state| !matches!(state, poll::RequestState::Failed(_)));
        Ok(())
    }
    pub fn cache_budget(&self) -> u64 {
        self.lock().cache_budget_bytes
    }
}

pub(crate) fn mesh_bytes(m: &MeshPayload) -> u64 {
    (std::mem::size_of_val(m)
        + m.vertices.capacity() * std::mem::size_of::<fynkel_cooked::payload::Vertex>()
        + m.indices.capacity() * 4
        + m.skin.capacity() * std::mem::size_of::<fynkel_cooked::payload::SkinWeight>()) as u64
}
pub(crate) fn clip_bytes(c: &Clip) -> u64 {
    (std::mem::size_of_val(c)
        + c.name.capacity()
        + c.channels.capacity() * std::mem::size_of::<Channel>()
        + c.channels
            .iter()
            .map(|c| c.target.capacity() + (c.times.capacity() + c.values.capacity()) * 4)
            .sum::<usize>()
        + c.markers.capacity() * std::mem::size_of::<Marker>()
        + c.markers.iter().map(|m| m.name.capacity()).sum::<usize>()) as u64
}

// Payload accounting includes inline values and owned allocation capacities.
// BTree node/allocator overhead and shared Arc headers remain outside the budget.
fn vec_bytes<T>(v: &Vec<T>) -> usize {
    v.capacity() * std::mem::size_of::<T>()
}
fn motion_heap(m: &fynkel_core::animation::blend_tree::MotionDef) -> usize {
    use fynkel_core::animation::blend_tree::MotionDef;
    match m {
        MotionDef::Asset(r) => r.sub.capacity(),
        MotionDef::Tree(t) => {
            std::mem::size_of_val(&**t)
                + vec_bytes(&t.parameters)
                + vec_bytes(&t.children)
                + t.children
                    .iter()
                    .map(|c| motion_heap(&c.motion))
                    .sum::<usize>()
        }
    }
}
pub(crate) fn controller_bytes(c: &ControllerDef) -> u64 {
    use fynkel_core::animation::controller::IkKind;
    let optional = |s: &Option<String>| s.as_ref().map_or(0, String::capacity);
    (std::mem::size_of_val(c)
        + c.name.capacity()
        + vec_bytes(&c.parameters)
        + c.parameters
            .iter()
            .map(|p| p.name.capacity())
            .sum::<usize>()
        + vec_bytes(&c.layers)
        + c.layers
            .iter()
            .map(|l| {
                l.name.capacity()
                    + optional(&l.root)
                    + vec_bytes(&l.states)
                    + l.states
                        .iter()
                        .map(|s| s.name.capacity() + s.motion.as_ref().map_or(0, motion_heap))
                        .sum::<usize>()
                    + vec_bytes(&l.transitions)
                    + l.transitions
                        .iter()
                        .map(|t| vec_bytes(&t.conditions))
                        .sum::<usize>()
                    + vec_bytes(&l.machines)
                    + l.machines.iter().map(|(s, _)| s.capacity()).sum::<usize>()
                    + vec_bytes(&l.mask)
                    + l.mask.iter().map(|m| m.node.capacity()).sum::<usize>()
            })
            .sum::<usize>()
        + vec_bytes(&c.ik)
        + c.ik
            .iter()
            .map(|ik| {
                ik.name.capacity()
                    + optional(&ik.target)
                    + match &ik.kind {
                        IkKind::TwoBone {
                            root,
                            middle,
                            tip,
                            pole,
                        } => optional(root) + optional(middle) + tip.capacity() + optional(pole),
                        IkKind::LookAt { head, spine, .. } => {
                            head.capacity()
                                + vec_bytes(spine)
                                + spine.iter().map(String::capacity).sum::<usize>()
                        }
                        IkKind::FootPlacement {
                            hips, left, right, ..
                        } => hips.capacity() + left.capacity() + right.capacity(),
                    }
            })
            .sum::<usize>()) as u64
}
pub(crate) fn json_bytes(v: &serde_json::Value) -> usize {
    use serde_json::Value;
    std::mem::size_of_val(v)
        + match v {
            Value::String(s) => s.capacity(),
            Value::Array(v) => {
                vec_bytes(v)
                    + v.iter()
                        .map(|x| json_bytes(x) - std::mem::size_of_val(x))
                        .sum::<usize>()
            }
            Value::Object(v) => v
                .iter()
                .map(|(k, v)| std::mem::size_of_val(k) + k.capacity() + json_bytes(v))
                .sum(),
            _ => 0,
        }
}
pub(crate) fn prefab_bytes(p: &CookedPrefab) -> u64 {
    // The generated field view accounts variable component data, including
    // future builtins; charge it in addition to the inline enum storage.
    (std::mem::size_of_val(p)
        + p.name.capacity()
        + p.path.capacity()
        + p.description.name.capacity()
        + vec_bytes(&p.closure)
        + vec_bytes(&p.description.entities)
        + p.description
            .entities
            .iter()
            .map(|e| {
                e.name.capacity()
                    + vec_bytes(&e.children)
                    + vec_bytes(&e.components)
                    + e.components
                        .iter()
                        .map(|c| {
                            json_bytes(
                                &fynkel_document::generated::encode_component(c).1.to_serde(),
                            )
                        })
                        .sum::<usize>()
                    + vec_bytes(&e.registered)
                    + e.registered
                        .iter()
                        .map(|r| {
                            r.id.capacity()
                                + r.fields
                                    .iter()
                                    .map(|(k, v)| {
                                        use fynkel_core::{AuthoredRegisteredValue, ScalarValue};
                                        std::mem::size_of_val(k)
                                            + k.capacity()
                                            + std::mem::size_of_val(v)
                                            + match v {
                                                AuthoredRegisteredValue::Scalar(
                                                    ScalarValue::Text(s),
                                                ) => s.capacity(),
                                                AuthoredRegisteredValue::Scalar(
                                                    ScalarValue::Asset(r),
                                                ) => r.sub.capacity(),
                                                _ => 0,
                                            }
                                    })
                                    .sum::<usize>()
                        })
                        .sum::<usize>()
            })
            .sum::<usize>()
        + vec_bytes(&p.behaviours)
        + p.behaviours
            .iter()
            .map(|b| b.type_id.capacity() + b.pointer.capacity() + json_bytes(&b.data))
            .sum::<usize>()
        + vec_bytes(&p.diagnostics)
        + p.diagnostics
            .iter()
            .map(|d| d.to_string().len())
            .sum::<usize>()) as u64
}

pub(crate) fn material_bytes(m: &PbrMaterial) -> u64 {
    (std::mem::size_of_val(m)
        + m.textures()
            .iter()
            .filter_map(|(_, _, r)| *r)
            .map(|r| r.sub.capacity())
            .sum::<usize>()) as u64
}
pub(crate) fn custom_bytes(m: &CustomMaterialPayload) -> u64 {
    (std::mem::size_of_val(m)
        + m.block.capacity()
        + m.slots.capacity() * std::mem::size_of::<MaterialSlot>()
        + m.slots
            .iter()
            .filter_map(|s| s.texture.as_ref())
            .map(|(_, s)| s.capacity())
            .sum::<usize>()) as u64
}
