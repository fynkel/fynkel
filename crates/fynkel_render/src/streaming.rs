//! Counted upload admission shared by visible draws and background warming.

/// Limits asset work admitted during one `prepare`. Time is a soft bound
/// between indivisible operations; bytes and operation counts are hard bounds.
#[derive(Clone, Copy, Debug)]
pub struct UploadBudget {
    pub bytes_per_frame: u64,
    pub operations_per_frame: u32,
}
impl Default for UploadBudget {
    fn default() -> Self {
        Self {
            bytes_per_frame: 32 * 1024 * 1024,
            operations_per_frame: 16,
        }
    }
}
#[derive(Default)]
pub(crate) struct UploadUsage {
    pub bytes: u64,
    pub operations: u32,
    pub deferred: u32,
}
impl UploadUsage {
    pub fn admit(&mut self, budget: UploadBudget, bytes: u64) -> bool {
        if self.operations >= budget.operations_per_frame
            || bytes > budget.bytes_per_frame.saturating_sub(self.bytes)
        {
            self.deferred += 1;
            return false;
        }
        self.bytes += bytes;
        self.operations += 1;
        true
    }
}

use super::*;
impl ExtractRenderer {
    /// Whether the last prepare still needs asset work (including extra worlds).
    pub fn assets_pending(&self) -> bool {
        self.assets_pending
    }

    /// Nonblocking readiness observation after normal frame preparation.
    /// Missing resources and explicit fallbacks are not GPU-ready.
    pub fn assets_ready(&self, extract: &Extract) -> bool {
        !self.frame_assets_deferred
            && self.required_asset_errors(extract, &[]).is_empty()
            && self.assets_settled(extract, &[])
    }
    fn required_asset_errors(&self, extract: &Extract, extras: &[ExtraWorld]) -> Vec<String> {
        if self.asset_failures.is_empty() {
            return Vec::new();
        }
        let mask = extract
            .camera
            .as_ref()
            .map_or(u32::MAX, |c| c.camera.culling_mask);
        let eye = extract
            .camera
            .as_ref()
            .map_or(self.camera.eye, |c| c.world.w_axis.truncate());
        let mut roots = HashSet::new();
        for view in std::iter::once(extract).chain(extras.iter().map(|e| &e.extract)) {
            for mesh in &view.meshes {
                if view
                    .records
                    .get(mesh.record as usize)
                    .is_some_and(|r| r.layer_bit & mask != 0)
                {
                    roots.extend([mesh.mesh.asset, mesh.material.asset]);
                }
            }
        }
        roots.extend(
            extract
                .environment
                .iter()
                .filter_map(|e| e.environment.sky.as_ref())
                .map(|r| r.asset),
        );
        roots.extend(
            extract
                .volumes
                .iter()
                .filter(|v| {
                    v.volume.weight * fynkel_core::volume::factor(&v.volume, &v.world, eye) > 0.0
                })
                .map(|v| v.volume.profile.asset),
        );
        roots.retain(|id| !id.is_nil());
        let required: HashSet<_> = roots
            .into_iter()
            .flat_map(|id| {
                self.assets
                    .as_ref()
                    .map_or_else(|| vec![id], |a| a.closure(id))
            })
            .collect();
        let mut errors: Vec<_> = self
            .asset_failures
            .iter()
            .filter(|(r, _)| required.contains(&r.asset))
            .flat_map(|(_, errors)| errors.iter().cloned())
            .collect();
        errors.sort();
        errors
    }

    pub(super) fn assets_settled(&self, extract: &Extract, extras: &[ExtraWorld]) -> bool {
        let camera_mask = extract
            .camera
            .as_ref()
            .map_or(u32::MAX, |c| c.camera.culling_mask);
        let eye = extract
            .camera
            .as_ref()
            .map_or(self.camera.eye, |c| c.world.w_axis.truncate());
        std::iter::once(extract)
            .chain(extras.iter().map(|e| &e.extract))
            .all(|view| {
                view.meshes.iter().all(|m| {
                    if view
                        .records
                        .get(m.record as usize)
                        .is_none_or(|r| r.layer_bit & camera_mask == 0)
                    {
                        return true;
                    }
                    (m.mesh.is_nil() || self.meshes.contains_key(&m.mesh))
                        && (m.material.is_nil()
                            || self.materials.contains_key(&m.material)
                            || self.broken_materials.contains(&m.material))
                })
            })
            && extract.volumes.iter().all(|v| {
                v.volume.profile.is_nil()
                    || v.volume.weight * fynkel_core::volume::factor(&v.volume, &v.world, eye)
                        <= 0.0
                    || self.volume_profiles.contains_key(&v.volume.profile)
            })
            && extract
                .environment
                .as_ref()
                .and_then(|e| e.environment.sky.as_ref())
                .is_none_or(|r| {
                    self.textures.get(r).is_some_and(|texture| {
                        texture.is_none()
                            || self.environments.get(r).is_some_and(|env| {
                                env.is_none() || self.ibl.as_ref().is_some_and(|(key, _)| key == r)
                            })
                    })
                })
    }

    /// Explicit loading-screen/export boundary. Normal `prepare` never waits.
    /// Requires successful GPU readiness: terminal fallbacks return an error,
    /// as do requests still pending at the deadline. Normal gameplay can keep
    /// drawing with explicit fallbacks through `prepare` and `asset_errors`.
    pub fn prepare_ready(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        extract: &Extract,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let started = std::time::Instant::now();
        let extras = self.extra_worlds.clone();
        let overlay = self.overlay.clone();
        let debug = self.debug.clone();
        loop {
            self.extra_worlds.clone_from(&extras);
            self.overlay.clone_from(&overlay);
            self.debug.clone_from(&debug);
            self.prepare(device, queue, extract);
            let settled = self.assets_settled(extract, &extras);
            let errors = self.required_asset_errors(extract, &extras);
            if !errors.is_empty() {
                return Err(format!(
                    "required GPU resources failed: {}",
                    errors.join("; ")
                ));
            }
            if settled && !self.frame_assets_deferred {
                return Ok(());
            }
            if started.elapsed() >= timeout {
                return Err("asset preparation did not settle before the deadline".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_work_shares_byte_and_operation_limits_without_overflow() {
        let budget = UploadBudget {
            bytes_per_frame: 10,
            operations_per_frame: 2,
        };
        let mut used = UploadUsage::default();
        assert!(!used.admit(budget, u64::MAX));
        assert!(used.admit(budget, 7));
        assert!(!used.admit(budget, 4));
        assert!(used.admit(budget, 3));
        assert!(!used.admit(budget, 0));
        assert_eq!((used.bytes, used.operations, used.deferred), (10, 2, 3));
    }
}
