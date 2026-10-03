//! Incremental resource ownership observation for hosts with an asset store.
use super::*;
use std::any::TypeId;

/// Opaque, world-scoped revision of running animation ownership. Includes
/// entity membership so despawn and world replacement cannot reuse stale roots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransientAnimationStamp(fynkel_ecs::ComponentStamp);

impl World {
    pub fn transient_animation_stamp(&self) -> Option<TransientAnimationStamp> {
        self.ecs()
            .component_stamp::<super::animation::AnimatorRun>()
            .map(TransientAnimationStamp)
    }

    /// Dirty resource owners since a token from this world. Transform/name
    /// animation does not dirty this domain. `reset` requires a full rebuild.
    pub fn resource_changes(&self, token: u64) -> Option<(u64, bool, Vec<Entity>)> {
        let base = [
            TypeId::of::<crate::MeshRenderer>(),
            TypeId::of::<crate::Collider>(),
            TypeId::of::<crate::Animator>(),
            TypeId::of::<crate::Skin>(),
            TypeId::of::<crate::AudioSource>(),
            TypeId::of::<crate::Emitter>(),
            TypeId::of::<crate::Environment>(),
            TypeId::of::<crate::Volume>(),
        ];
        let mut registered = self.gameplay.registered.asset_domains();
        let domains = if registered.is_empty() {
            &base[..]
        } else {
            registered.extend(base);
            &registered[..]
        };
        let changes =
            self.ecs()
                .changes_since(self.ecs().cursor_from_token(token), domains, false)?;
        Some((changes.cursor.token(), changes.reset, changes.entities))
    }

    /// Resource references of one object, including registered components.
    pub fn resource_references(&self, entity: Entity) -> Vec<AssetRef> {
        let mut refs = Vec::new();
        for value in crate::ComponentValue::all_of(self, entity) {
            match value {
                crate::ComponentValue::MeshRenderer(v) => refs.extend([v.mesh, v.material]),
                crate::ComponentValue::Collider(v) => {
                    refs.extend(v.mesh.into_iter().chain(v.material))
                }
                crate::ComponentValue::Animator(v) => {
                    refs.push(v.clip);
                    refs.extend(v.controller);
                }
                crate::ComponentValue::Skin(v) => refs.push(v.skin),
                crate::ComponentValue::AudioSource(v) => refs.push(v.clip),
                crate::ComponentValue::Emitter(v) => refs.push(v.prefab),
                crate::ComponentValue::Environment(v) => refs.extend(v.sky),
                crate::ComponentValue::Volume(v) => refs.push(v.profile),
                _ => {}
            }
        }
        for schema in self.registered_schemas() {
            if let Ok(registration) = self.gameplay.registered.get(&schema.id) {
                if let Some(values) = registration.read(self.ecs(), entity) {
                    refs.extend(values.into_iter().filter_map(|v| match v {
                        crate::ScalarValue::Asset(r) => Some(r),
                        _ => None,
                    }));
                }
            }
        }
        refs.retain(|r| !r.is_nil());
        refs
    }

    /// All ownership that outlives authored component values.
    pub fn visit_transient_resources(&self, mut visit: impl FnMut(&AssetRef)) {
        self.visit_pending_and_audio_resources(&mut visit);
        self.visit_transient_animation_resources(visit);
    }

    /// Queued effects and audio voices can change independently of the ECS
    /// animation stamp and must be sampled at every ownership boundary.
    pub fn visit_pending_and_audio_resources(&self, mut visit: impl FnMut(&AssetRef)) {
        for effect in &self.gameplay.gameplay_effects {
            match effect {
                super::gameplay::CommittedEffect::Sound { clip, .. } => visit(clip),
                super::gameplay::CommittedEffect::Animation { op, before, .. } => {
                    visit(&before.clip);
                    if let Some(controller) = &before.controller {
                        visit(controller);
                    }
                    match op {
                        crate::AnimateOp::Play { clip, .. }
                        | crate::AnimateOp::Crossfade { clip, .. } => visit(clip),
                        crate::AnimateOp::PlayOneShot { motion, .. } => visit(motion),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        for clip in self.audio.active_clips() {
            visit(clip);
        }
    }

    /// Runtime animation ownership only. Hosts may reuse this set while
    /// `transient_animation_stamp` is unchanged and settled. Queued effects
    /// and active audio voices must still be observed at every boundary.
    pub fn visit_transient_animation_resources(&self, mut visit: impl FnMut(&AssetRef)) {
        for run in self
            .gameplay
            .ecs
            .query::<&super::animation::AnimatorRun>()
            .iter()
        {
            if let Some(key) = &run.resolved {
                visit(key);
            }
            if let Some(fade) = &run.fade {
                visit(&fade.clip_ref);
            }
            for slot in &run.blend {
                visit(&slot.clip_ref);
            }
            if let Some(play) = &run.controller {
                visit(&play.controller);
                for shot in play
                    .slots
                    .iter()
                    .flat_map(|s| s.current.iter().chain(s.fading.iter()))
                {
                    visit(&shot.motion);
                }
            }
        }
    }

    /// Retire registrations whose owner released them. In-flight sound voices
    /// retain their registration until playback finishes.
    pub fn retain_resource_registrations(&mut self, mut keep: impl FnMut(&AssetRef) -> bool) {
        self.anim.clips.retain(|r, _| keep(r));
        self.anim.blends.retain(|r, _| keep(r));
        self.anim.controllers.retain(|r, _| keep(r));
        self.anim.skins.retain(|r, _| keep(r));
        self.prefabs.retain(|id, _| keep(&AssetRef::new(*id)));
        self.physics.retain_resources(&mut keep);
    }

    pub fn retain_audio_resources(&mut self, mut keep: impl FnMut(&AssetRef) -> bool) {
        let pending = &self.gameplay.gameplay_effects;
        self.audio.retain_clips(|r| keep(r) || pending.iter().any(|effect| matches!(effect, super::gameplay::CommittedEffect::Sound { clip, .. } if clip == r)));
    }
}
