//! Incremental owners for derived scene state. Stateless helpers in lib.rs
//! remain the reference kernels; a running world retains this owner.
use super::*;
use fynkel_ecs::ChangeCursor;
use std::any::TypeId;

#[derive(Default)]
pub struct SceneSync {
    transforms: Option<ChangeCursor>,
    activity: Option<ChangeCursor>,
    pub transform_candidates: usize,
    pub activity_candidates: usize,
}

impl SceneSync {
    pub fn propagate(&mut self, world: &mut World) -> usize {
        let inputs = world
            .changes_since(
                self.transforms,
                &[
                    TypeId::of::<Transform>(),
                    TypeId::of::<Parent>(),
                    TypeId::of::<Children>(),
                ],
                false,
            )
            .expect("settled scene storage");
        let flags = world
            .changes_since(self.transforms, &[TypeId::of::<WorldTransform>()], false)
            .expect("settled scene storage");
        let reset = inputs.reset || flags.reset;
        let mut candidates = if reset {
            world.entities().collect::<Vec<_>>()
        } else {
            inputs.entities
        };
        // Local pose/hierarchy writes must invalidate derived matrices even if
        // the native caller did not explicitly set WorldTransform.dirty.
        let clean: Vec<Entity> = {
            let mut spatial = world.query::<(&Transform, &WorldTransform)>();
            let view = spatial.view();
            candidates
                .iter()
                .copied()
                .filter(|&e| {
                    world
                        .raw(e)
                        .and_then(|raw| view.get(raw))
                        .is_some_and(|(_, wt)| !wt.dirty)
                })
                .collect()
        };
        world
            .update_each::<&mut WorldTransform>(&clean, |_, wt| {
                wt.dirty = true;
                true
            })
            .expect("spatial entities checked above");
        if !reset {
            candidates.extend(flags.entities);
        }
        // Two ascending runs: the stable sort merges them.
        candidates.sort();
        candidates.dedup();
        self.transform_candidates = candidates.len();
        let dirty = {
            let mut inputs = world.query::<(&Transform, &WorldTransform, Option<&Parent>)>();
            let view = inputs.view();
            candidates
                .into_iter()
                .filter_map(|entity| {
                    let (_, wt, parent) = world.raw(entity).and_then(|raw| view.get(raw))?;
                    wt.dirty.then_some((entity, parent.map(|p| p.0)))
                })
                .collect()
        };
        let count = super::propagate_dirty(world, dirty);
        self.transforms = world.change_cursor();
        count
    }

    pub fn refresh_active(&mut self, world: &mut World) -> usize {
        let changes = world
            .changes_since(
                self.activity,
                &[
                    TypeId::of::<InactiveSelf>(),
                    TypeId::of::<Parent>(),
                    TypeId::of::<Children>(),
                    TypeId::of::<Inactive>(),
                ],
                false,
            )
            .expect("settled scene storage");
        self.activity_candidates = if changes.reset {
            world.len() as usize
        } else {
            changes.entities.len()
        };
        let count = if changes.reset {
            super::refresh_active(world)
        } else {
            // Only highest changed ancestors need traversal. Parent markers
            // may be stale until that ancestor has been refreshed.
            let changed: BTreeSet<_> = changes
                .entities
                .into_iter()
                .filter(|e| world.contains(*e))
                .collect();
            let mut count = 0;
            for &e in &changed {
                let mut up = parent_of(world, e);
                let mut covered = false;
                while let Some(p) = up {
                    if changed.contains(&p) {
                        covered = true;
                        break;
                    }
                    up = parent_of(world, p);
                }
                if !covered {
                    count += refresh_active_subtree(world, e);
                }
            }
            count
        };
        self.activity = world.change_cursor();
        count
    }
}
