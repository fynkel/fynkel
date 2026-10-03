//! Owns the asset store, residency tokens and cancellable background work.
use crate::load;
use fynkel_contracts::Entity;
#[derive(Default)]
pub(crate) struct ResourceServices {
    pub(crate) managed_runtime_clips: std::collections::HashSet<fynkel_contracts::AssetRef>,
    transient_roots: std::collections::HashSet<fynkel_contracts::Uuid>,
    transient_scratch: std::collections::HashSet<fynkel_contracts::Uuid>,
    transient_residency: Option<fynkel_assets::Residency>,
    animation_stamp: Option<fynkel_core::world::TransientAnimationStamp>,
    animation_roots: std::collections::HashSet<fynkel_contracts::Uuid>,
    #[cfg(test)]
    animation_scans: usize,
    resource_cursor: u64,
    resource_evictions: u64,
    resource_edits: (u64, u64),
    object_residency: std::collections::HashMap<Entity, fynkel_assets::Residency>,
    /// The cache the last load opened (a renderer drawing this world adopts
    /// it, so both see one residency store).
    pub(crate) assets: Option<fynkel_assets::Assets>,
    /// One token per loaded scene (keyed by its id): the closure stays
    /// resident until the scene is unloaded, a replace load empties the
    /// world, or the world is destroyed (M3-12).
    pub(crate) residency: Vec<(Option<fynkel_contracts::Uuid>, fynkel_assets::Residency)>,
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
            if !old.same_store(&assets) {
                self.transient_roots.clear();
                self.animation_stamp = None;
                self.animation_roots.clear();
                self.transient_residency = None;
                self.resource_cursor = 0;
                self.object_residency.clear();
            }
        }
        self.assets = Some(assets);
    }
}

/// Acquire new live-object ownership before releasing any old token. This runs
/// at publication/step/load boundaries, never inside a pending gameplay edit.
pub(crate) fn synchronize(world: &mut crate::FynkelWorld) {
    let Some(assets) = world.services.assets.clone() else {
        return;
    };
    let Some((cursor, reset, mut dirty)) = world
        .core()
        .resource_changes(world.services.resource_cursor)
    else {
        return;
    };
    let edits = (assets.live_edits(), assets.material_edits());
    if reset || edits != world.services.resource_edits {
        dirty = world.core().ecs().entities().collect();
        let live: std::collections::HashSet<_> = dirty.iter().copied().collect();
        world
            .services
            .object_residency
            .retain(|e, _| live.contains(e));
    }
    world.services.resource_cursor = cursor;
    world.services.resource_edits = edits;
    let mut acquired = Vec::new();
    for entity in dirty {
        let refs = world.core().resource_references(entity);
        let ids: std::collections::HashSet<_> =
            refs.iter().flat_map(|r| assets.closure(r.asset)).collect();
        let ids: Vec<_> = ids.into_iter().collect();
        acquired.push((entity, (!ids.is_empty()).then(|| assets.retain(&ids))));
    }
    for (entity, lease) in acquired {
        match lease {
            Some(lease) => {
                world.services.object_residency.insert(entity, lease);
            }
            None => {
                world.services.object_residency.remove(&entity);
            }
        }
    }
    let mut roots = std::mem::take(&mut world.services.transient_scratch);
    roots.clear();
    let animation_stamp = world.core().transient_animation_stamp();
    if animation_stamp.is_none() || animation_stamp != world.services.animation_stamp {
        let mut animation_roots = std::mem::take(&mut world.services.animation_roots);
        animation_roots.clear();
        world.core().visit_transient_animation_resources(|r| {
            if !r.is_nil() {
                animation_roots.insert(r.asset);
            }
        });
        world.services.animation_roots = animation_roots;
        world.services.animation_stamp = animation_stamp;
        #[cfg(test)]
        {
            world.services.animation_scans += 1;
        }
    }
    roots.extend(world.services.animation_roots.iter().copied());
    world.core().visit_pending_and_audio_resources(|r| {
        if !r.is_nil() {
            roots.insert(r.asset);
        }
    });
    if roots != world.services.transient_roots {
        let ids: std::collections::HashSet<_> =
            roots.iter().flat_map(|id| assets.closure(*id)).collect();
        let lease = (!ids.is_empty()).then(|| assets.retain(&ids.into_iter().collect::<Vec<_>>()));
        world.services.transient_residency = lease;
        std::mem::swap(&mut roots, &mut world.services.transient_roots);
    }
    world.services.transient_scratch = roots;
    world.inner.release_published_resources();
    // Unknown IDs are explicit host/runtime registrations, outside the cooked
    // cache's ownership. Active one-shots are retained by the audio adapter.
    let evictions = assets.evictions();
    if reset
        || evictions != world.services.resource_evictions
        || !world.services.managed_runtime_clips.is_empty()
    {
        world.services.resource_evictions = evictions;
        let mut managed = std::mem::take(&mut world.services.managed_runtime_clips);
        world.core_mut().retain_resource_registrations(|r| {
            assets.is_resident(r.asset)
                || if managed.contains(r) {
                    assets.is_runtime_clip(r)
                } else {
                    assets.entry(r.asset).is_none()
                }
        });
        managed.retain(|r| world.core().has_clip(r));
        world.services.managed_runtime_clips = managed;
    }
    // Audio has a fixed clip-slot limit. A finished one-shot can now release a
    // registration that survived the last scene eviction while it was playing.
    world
        .core_mut()
        .retain_audio_resources(|r| assets.entry(r.asset).is_none() || assets.is_resident(r.asset));
}

#[cfg(test)]
mod tests {
    use super::*;
    use fynkel_core::{world::AnimatorRun, AssetRef, Transform, World, WorldConfig};

    #[test]
    fn animation_ownership_cache_ignores_pose_edits_but_tracks_runtime_and_lifetimes() {
        let cache =
            std::env::temp_dir().join(format!("fynkel-animation-roots-{}", std::process::id()));
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../fynkel_document/fixtures/project");
        let cooked =
            fynkel_import::cook_project(&fixture, Some(&cache), &Default::default()).unwrap();
        assert!(!cooked.has_errors(), "{}", cooked.text());
        let assets = fynkel_assets::Assets::open(&cache).unwrap();
        let mut host = crate::FynkelWorld::with_core(World::new(WorldConfig::default()));
        host.services.adopt(assets);
        let a = AssetRef::new("aaaaaaaa-0000-4000-8000-000000000001".parse().unwrap());
        let b = AssetRef::new("aaaaaaaa-0000-4000-8000-000000000002".parse().unwrap());
        let entity =
            fynkel_scene::spawn(host.core_mut().ecs_mut(), Transform::IDENTITY, None).unwrap();
        host.core_mut()
            .ecs_mut()
            .insert(
                entity,
                AnimatorRun {
                    resolved: Some(a.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        synchronize(&mut host);
        let scans = host.services.animation_scans;
        assert_eq!(host.services.transient_roots, [a.asset].into());
        for _ in 0..300 {
            host.core_mut()
                .edit_gameplay(|edit| {
                    let mut pose = edit.local_transform(entity)?;
                    pose.position.x += 1.0;
                    edit.set_transform(entity, pose)
                })
                .unwrap();
            synchronize(&mut host);
            host.core_mut().edit_gameplay(|_| Ok(())).unwrap();
            synchronize(&mut host);
        }
        assert_eq!(
            host.services.animation_scans, scans,
            "pose/empty callbacks must not rescan animations"
        );
        host.core_mut()
            .ecs_mut()
            .get_mut::<AnimatorRun>(entity)
            .unwrap()
            .resolved = Some(b.clone());
        synchronize(&mut host);
        assert_eq!(host.services.transient_roots, [b.asset].into());
        assert_eq!(host.services.animation_scans, scans + 1);
        host.core_mut()
            .edit_gameplay(|edit| edit.destroy(entity))
            .unwrap();
        synchronize(&mut host);
        assert!(host.services.transient_roots.is_empty());
        assert_eq!(host.services.animation_scans, scans + 2);
        // New world identity must invalidate even when its numeric revisions match.
        let mut replacement = World::new(WorldConfig::default());
        let entity = fynkel_scene::spawn(replacement.ecs_mut(), Transform::IDENTITY, None).unwrap();
        replacement
            .ecs_mut()
            .insert(
                entity,
                AnimatorRun {
                    resolved: Some(a.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        host.inner = crate::gameplay::WorldSlot::new(replacement);
        synchronize(&mut host);
        assert_eq!(host.services.transient_roots, [a.asset].into());
        assert_eq!(host.services.animation_scans, scans + 3);
        #[cfg(feature = "animation")]
        {
            // A queued fade changes ownership before AnimatorRun changes.
            host.core_mut()
                .ecs_mut()
                .insert(
                    entity,
                    fynkel_core::Animator {
                        clip: a.clone(),
                        ..Default::default()
                    },
                )
                .unwrap();
            let stamp = host.core().transient_animation_stamp();
            host.core_mut()
                .edit_gameplay(|edit| {
                    edit.animate_control(
                        entity,
                        fynkel_core::AnimateOp::Crossfade {
                            clip: b.clone(),
                            looping: true,
                            speed: 1.0,
                            seconds: 1.0,
                        },
                    )
                })
                .unwrap();
            assert_eq!(host.core().transient_animation_stamp(), stamp);
            synchronize(&mut host);
            assert_eq!(host.services.animation_scans, scans + 3);
            assert_eq!(
                host.services.transient_roots,
                [a.asset, b.asset].into(),
                "queued effects must be observed even with a cached animation set"
            );
        }
        drop(host);
        std::fs::remove_dir_all(cache).unwrap();
    }
}
