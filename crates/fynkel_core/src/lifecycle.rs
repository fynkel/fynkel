//! Scene ownership metadata has a separate revision from gameplay storage.
//! Mutable leases advance it before exposing state, including metadata-only
//! operations such as persistence and authored identity attachment.
use crate::LoadedScene;
use fynkel_contracts::Entity;
#[derive(Default)]
pub(crate) struct SceneLifecycle {
    scenes: Vec<LoadedScene>,
    persistent: Vec<Entity>,
    revision: u64,
}
impl SceneLifecycle {
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    fn changed(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("scene revision exhausted");
    }
    pub(crate) fn scenes(&self) -> &[LoadedScene] {
        &self.scenes
    }
    pub(crate) fn scenes_mut(&mut self) -> &mut Vec<LoadedScene> {
        self.changed();
        &mut self.scenes
    }
    pub(crate) fn persistent(&self) -> &[Entity] {
        &self.persistent
    }
    pub(crate) fn persistent_mut(&mut self) -> &mut Vec<Entity> {
        self.changed();
        &mut self.persistent
    }
}
