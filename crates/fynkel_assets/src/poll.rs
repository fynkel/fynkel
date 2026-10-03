//! Non-blocking draw-side access. Decoding is admitted to the shared worker;
//! returned values own their payloads, and pending is never cached as failure.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ResourceKind {
    Mesh,
    Texture,
    Material,
    CustomMaterial,
    Environment,
    Volume,
    Shader,
}
#[derive(Clone)]
pub(crate) enum RequestState {
    Queued,
    Ready,
    Failed(Diagnostic),
}

impl Assets {
    fn poll<T>(
        &self,
        kind: ResourceKind,
        r: &AssetRef,
        cached: impl FnOnce(&Store) -> Option<T>,
    ) -> Option<Result<T, Diagnostic>> {
        let key = (kind, r.clone());
        let generation = {
            let mut s = self.lock();
            if let Some(value) = cached(&s) {
                return Some(Ok(value));
            }
            match s.requests.get(&key) {
                Some(RequestState::Queued) => return None,
                Some(RequestState::Failed(error)) => return Some(Err(error.clone())),
                _ => {
                    s.requests.insert(key.clone(), RequestState::Queued);
                }
            }
            s.generation(r.asset)
        };
        // The lease covers preparation and is released even on rejection/panic.
        // If its consumer went away, releasing it evicts the result immediately.
        let lease = self.retain(&self.closure(r.asset));
        let source = self.clone();
        let request = key.clone();
        let submitted = self.schedule_preparation(PreparationPriority::Visible, move || {
            if source.lock().generation(request.1.asset) != generation {
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match kind {
                ResourceKind::Mesh => source.mesh(&request.1).map(|_| ()),
                ResourceKind::Texture => source.texture(&request.1).map(|_| ()),
                ResourceKind::Material => source.material(&request.1).map(|_| ()),
                ResourceKind::CustomMaterial => source.custom_material(&request.1).map(|_| ()),
                ResourceKind::Environment => source.environment(&request.1).map(|_| ()),
                ResourceKind::Volume => source.volume_profile(&request.1).map(|_| ()),
                ResourceKind::Shader => source.shader_source(request.1.asset).map(|_| ()),
            }))
            .unwrap_or_else(|_| {
                Err(Diagnostic::error(Code::IO, "asset preparation panicked")
                    .with_id(request.1.asset.to_string()))
            });
            let mut s = source.lock();
            if s.generation(request.1.asset) != generation {
                return;
            }
            s.requests.insert(
                request,
                match result {
                    Ok(()) => RequestState::Ready,
                    Err(e) => RequestState::Failed(e),
                },
            );
            drop(s);
            drop(lease);
        });
        match submitted {
            Ok(()) => None,
            Err(error) => self.reject_request(key, generation, error).map(Err),
        }
    }

    fn reject_request(
        &self,
        key: (ResourceKind, AssetRef),
        generation: (u64, u64),
        refusal: PreparationError,
    ) -> Option<Diagnostic> {
        let mut s = self.lock();
        // Submission runs outside the store lock. Invalidation and a newer
        // request may have overtaken it before the refusal is handled.
        if s.generation(key.1.asset) != generation {
            return None;
        }
        if refusal == PreparationError::Busy {
            s.requests.remove(&key);
            return None;
        }
        let error =
            Diagnostic::error(Code::IO, refusal.to_string()).with_id(key.1.asset.to_string());
        s.requests.insert(key, RequestState::Failed(error.clone()));
        Some(error)
    }

    pub fn poll_mesh(&self, r: &AssetRef) -> Option<Result<Arc<MeshPayload>, Diagnostic>> {
        self.poll(ResourceKind::Mesh, r, |s| {
            s.runtime_meshes.get(r).or_else(|| s.meshes.get(r)).cloned()
        })
    }
    pub fn poll_texture(&self, r: &AssetRef) -> Option<Result<Arc<TexturePayload>, Diagnostic>> {
        self.poll(ResourceKind::Texture, r, |s| s.textures.get(r).cloned())
    }
    pub fn poll_material(&self, r: &AssetRef) -> Option<Result<PbrMaterial, Diagnostic>> {
        self.poll(ResourceKind::Material, r, |s| {
            s.runtime_materials
                .get(r)
                .or_else(|| s.materials.get(r))
                .map(|m| (**m).clone())
        })
    }
    pub fn poll_custom_material(
        &self,
        r: &AssetRef,
    ) -> Option<Result<Option<Arc<CustomMaterialPayload>>, Diagnostic>> {
        self.poll(ResourceKind::CustomMaterial, r, |s| {
            if let Some(m) = s.runtime_custom_materials.get(r) {
                Some(Some(m.clone()))
            } else if s.runtime_materials.contains_key(r) {
                Some(None)
            } else {
                s.custom_materials.get(r).cloned()
            }
        })
    }
    pub fn poll_environment(
        &self,
        r: &AssetRef,
    ) -> Option<Result<Option<Arc<EnvironmentPayload>>, Diagnostic>> {
        self.poll(ResourceKind::Environment, r, |s| {
            s.environments.get(r).cloned()
        })
    }
    pub fn poll_volume_profile(&self, r: &AssetRef) -> Option<Result<VolumeProfile, Diagnostic>> {
        self.poll(ResourceKind::Volume, r, |s| s.volumes.get(r).copied())
    }
    pub fn poll_shader_source(&self, id: Uuid) -> Option<Result<Arc<str>, Diagnostic>> {
        self.poll(ResourceKind::Shader, &AssetRef::new(id), |s| {
            s.shaders.get(&id).cloned()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_submission_refusal_cannot_remove_or_poison_a_new_request() {
        let assets = Assets::from_index(CacheIndex {
            root: PathBuf::new(),
            profile: String::new(),
            schema_fingerprint: String::new(),
            project: None,
            entries: HashMap::new(),
        });
        let key = (
            ResourceKind::Texture,
            AssetRef::new(Uuid(*b"request-race-001")),
        );
        let old_generation = assets.lock().generation(key.1.asset);
        let lease = assets.retain(&[key.1.asset]);
        drop(lease);
        assert_ne!(
            assets.lock().generation(key.1.asset),
            old_generation,
            "release must invalidate work from the previous residency lifetime"
        );
        assets
            .lock()
            .requests
            .insert(key.clone(), RequestState::Queued);
        for refusal in [PreparationError::Busy, PreparationError::Unavailable] {
            assert!(assets
                .reject_request(key.clone(), old_generation, refusal)
                .is_none());
            assert!(matches!(
                assets.lock().requests.get(&key),
                Some(RequestState::Queued)
            ));
        }
        let current = assets.lock().generation(key.1.asset);
        assert!(assets
            .reject_request(key.clone(), current, PreparationError::Busy)
            .is_none());
        assert!(!assets.lock().requests.contains_key(&key));
        assert!(assets
            .reject_request(key.clone(), current, PreparationError::Unavailable)
            .is_some());
        assert!(matches!(
            assets.lock().requests.get(&key),
            Some(RequestState::Failed(_))
        ));
    }
}
